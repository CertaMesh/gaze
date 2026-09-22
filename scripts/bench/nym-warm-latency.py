#!/usr/bin/env python3
"""Measure warm per-document latency of the opt-in Nym-small arm.

Drives the canonical harness binary (`clean_for_bench`, the production pipeline)
twice: `pass2-ner` (rules plus NER, no net) and `full-stack-nym-resolve` (the
same plus the in-process Nym net under Resolve). Each process loads its models,
cleans one warm-up document, then cleans every measured document once per
repetition. The sample is the binary's own `timing.clean_ms`, so process start,
pipe transport and JSON handling are excluded. The difference between the two
arms is what the net adds.

Documents: the committed coverage-loop corpus plus two synthetic German
documents of exactly 512 and 1,024 Nym tokenizer pieces (one and two model
windows).

Needs the pinned bundles from `gaze setup --safety-net nym` (Nym plus Davlan
NER). Usage:

    uv run --with tokenizers python scripts/bench/nym-warm-latency.py --repo-root .

The synthetic piece counts are checked against the pinned tokenizer before any timing;
`--skip-verify-pieces` skips that check and the output then records
`"synthetic_pieces_verified": false`.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import re
import statistics
import subprocess
import sys
from pathlib import Path

ARMS = ("pass2-ner", "full-stack-nym-resolve")
SYNTHETIC_SENTENCE = (
    "Sehr geehrte Damen und Herren, wir bestätigen den Eingang Ihrer Unterlagen vom "
    "12. März und melden uns in den nächsten Tagen mit einer Rückmeldung zum weiteren "
    "Vorgehen. "
)
#: Word counts that give exactly 512 and 1,024 pieces with the pinned Nym tokenizer
#: (`tokenizer.json` of NYM_SMALL_INT8_BUNDLE_SHA256, no special tokens); every run checks
#: them against the installed tokenizer unless `--skip-verify-pieces` is passed.
SYNTHETIC_DOCUMENTS = {"synthetic-512-pieces": (395, 512), "synthetic-1024-pieces": (790, 1024)}
DEFAULT_CORPUS = Path("crates/gaze-recognizers/testdata/coverage-loop/corpus")
BINARY = Path("target/release/examples/clean_for_bench")


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--corpus-dir", type=Path, default=DEFAULT_CORPUS)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--nym-model-dir", type=Path, default=default_model_dir("GAZE_NYM_MODEL_DIR", "nym-small-int8"))
    parser.add_argument("--ner-model-dir", type=Path, default=default_model_dir("GAZE_NER_MODEL_DIR", "davlan-mbert-ner-hrl"))
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--skip-verify-pieces", action="store_true")
    return parser.parse_args(argv)


def default_model_dir(env: str, name: str) -> Path:
    """The `gaze setup` install location unless the environment names one."""
    if os.environ.get(env):
        return Path(os.environ[env])
    data_home = os.environ.get("XDG_DATA_HOME") or str(Path.home() / ".local" / "share")
    return Path(data_home) / "gaze" / "models" / name


def latency_summary(samples: list[float]) -> dict[str, float | int]:
    """Warm p50, nearest-rank p95 and mean in milliseconds."""
    if not samples:
        raise ValueError("no latency samples")
    ordered = sorted(samples)
    p95_index = max(0, math.ceil(0.95 * len(ordered)) - 1)
    return {
        "warm_p50_ms": round(statistics.median(ordered), 3),
        "warm_p95_ms": round(ordered[p95_index], 3),
        "warm_mean_ms": round(statistics.mean(ordered), 3),
        "samples": len(ordered),
    }


def synthetic_text(words: int) -> str:
    repeated = (SYNTHETIC_SENTENCE * (words // 10 + 1)).split()
    return " ".join(repeated[:words])


def ort_version(cargo_lock: str) -> str:
    match = re.search(r'name = "ort"\nversion = "([^"]+)"', cargo_lock)
    if match is None:
        raise ValueError("ort is not in Cargo.lock")
    return match.group(1)


def bundle_sha(artifacts_rs: str) -> str:
    match = re.search(r'NYM_SMALL_INT8_BUNDLE_SHA256: &str =\s*"([0-9a-f]{64})"', artifacts_rs)
    if match is None:
        raise ValueError("NYM_SMALL_INT8_BUNDLE_SHA256 not found")
    return match.group(1)


def hardware_line(info: dict[str, str]) -> str:
    return (
        f"{info['chip']}, {info['cores']} cores, {info['ram_gib']} GiB RAM, {info['os']}, "
        f"ort {info['ort']}, Nym bundle {info['bundle_sha'][:12]}…, "
        f"intra-op threads {info['intra_threads']}"
    )


def sysctl(name: str) -> str | None:
    try:
        return subprocess.check_output(["sysctl", "-n", name], text=True, stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def host_info(root: Path) -> dict[str, str]:
    memory = sysctl("hw.memsize")
    if memory is None and hasattr(os, "sysconf"):
        memory = str(os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES"))
    return {
        "chip": sysctl("machdep.cpu.brand_string") or platform.processor() or "unknown",
        "cores": str(os.cpu_count()),
        "ram_gib": str(round(int(memory) / 2**30)) if memory else "unknown",
        "os": platform.platform(),
        "ort": ort_version((root / "Cargo.lock").read_text(encoding="utf-8")),
        "bundle_sha": bundle_sha(
            (root / "crates/gaze-recognizers/src/safety_net/nym/artifacts.rs").read_text(encoding="utf-8")
        ),
        "intra_threads": os.environ.get("GAZE_NYM_INTRA_THREADS", "1"),
    }


def load_documents(root: Path, corpus_dir: Path) -> list[dict[str, object]]:
    corpus = corpus_dir if corpus_dir.is_absolute() else root / corpus_dir
    documents = []
    for labels_path in sorted(corpus.glob("*.labels.json")):
        labels = json.loads(labels_path.read_text(encoding="utf-8"))
        fixture_id = labels["fixture_id"]
        documents.append(
            {
                "fixture_id": fixture_id,
                "locale_chain": [str(locale) for locale in labels["locale_chain"]],
                "text": labels_path.with_name(f"{fixture_id}.txt").read_text(encoding="utf-8"),
            }
        )
    if not documents:
        raise ValueError(f"no coverage-loop fixtures in {corpus}")
    for fixture_id, (words, _pieces) in SYNTHETIC_DOCUMENTS.items():
        documents.append({"fixture_id": fixture_id, "locale_chain": ["de-DE"], "text": synthetic_text(words)})
    return documents


def verify_pieces(nym_model_dir: Path) -> None:
    try:
        from tokenizers import Tokenizer
    except ImportError as error:
        raise SystemExit(
            "the `tokenizers` package is needed to check the synthetic piece counts; run with "
            "`uv run --with tokenizers` or pass --skip-verify-pieces"
        ) from error

    tokenizer = Tokenizer.from_file(str(nym_model_dir / "tokenizer.json"))
    for fixture_id, (words, pieces) in SYNTHETIC_DOCUMENTS.items():
        actual = len(tokenizer.encode(synthetic_text(words), add_special_tokens=False).ids)
        if actual != pieces:
            raise ValueError(f"{fixture_id}: {actual} pieces, expected {pieces}")


def measure_arm(binary: Path, arm: str, documents: list[dict[str, object]], repetitions: int, env: dict[str, str]) -> dict[str, list[float]]:
    """Per-document warm `clean_ms` samples; any pipeline error fails the run."""
    samples: dict[str, list[float]] = {str(document["fixture_id"]): [] for document in documents}
    with subprocess.Popen(
        [str(binary), "--config", arm],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        env=env,
    ) as process:
        assert process.stdin is not None and process.stdout is not None

        def clean(document: dict[str, object]) -> float:
            process.stdin.write(json.dumps(document) + "\n")
            process.stdin.flush()
            line = process.stdout.readline()
            if not line:
                raise RuntimeError(f"{arm}: harness exited (code {process.poll()})")
            response = json.loads(line)
            if "pipeline_error_stage" in response:
                raise RuntimeError(f"{arm}: {response['fixture_id']} failed at {response['pipeline_error_stage']}")
            return float(response["timing"]["clean_ms"])

        clean(documents[0])
        for _ in range(repetitions):
            for document in documents:
                samples[str(document["fixture_id"])].append(clean(document))
        process.stdin.close()
        if process.wait() != 0:
            raise RuntimeError(f"{arm}: harness exited {process.returncode}")
    return samples


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    root = args.repo_root.resolve()
    for label, path in (("Nym", args.nym_model_dir), ("NER", args.ner_model_dir)):
        if not path.is_dir():
            raise SystemExit(f"{label} bundle missing at {path}; run `gaze setup --safety-net nym`")
    if not args.skip_verify_pieces:
        verify_pieces(args.nym_model_dir)
    if not args.skip_build:
        subprocess.run(
            ["cargo", "build", "--release", "-q", "-p", "gaze-recognizers", "--example",
             "clean_for_bench", "--features", "safety-net-nym"],
            cwd=root,
            check=True,
        )
    env = {
        **os.environ,
        "GAZE_NYM_MODEL_DIR": str(args.nym_model_dir),
        "GAZE_NER_MODEL_DIR": str(args.ner_model_dir),
    }
    documents = load_documents(root, args.corpus_dir)
    info = host_info(root)
    # Latency on a loaded host is not a latency claim; record the load next to it.
    result: dict[str, object] = {
        "hardware": hardware_line(info),
        "host": info,
        "load_average_1_5_15_at_start": [round(value, 2) for value in os.getloadavg()],
        "synthetic_pieces_verified": not args.skip_verify_pieces,
        "arms": {},
    }
    for arm in ARMS:
        samples = measure_arm(root / BINARY, arm, documents, args.repetitions, env)
        synthetic = {fixture_id: latency_summary(samples.pop(fixture_id)) for fixture_id in SYNTHETIC_DOCUMENTS}
        corpus = [value for values in samples.values() for value in values]
        result["arms"][arm] = {"corpus": latency_summary(corpus), **synthetic}
    json.dump(result, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
