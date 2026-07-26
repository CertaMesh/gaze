#!/usr/bin/env python3
"""Benchmark Gaze on the pinned synthetic OpenPII validation corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import urllib.request
from collections import Counter
from pathlib import Path

import gaze_bench_score as score


DATASET_REPO = "ai4privacy/pii-masking-micro-100k"
DATASET_REVISION = "3cd59c65631280839f830d3ba96dcdfe1785cab1"
DATASET_FILE = "data/validation.jsonl"
DATASET_URL = (
    f"https://huggingface.co/datasets/{DATASET_REPO}/resolve/"
    f"{DATASET_REVISION}/{DATASET_FILE}"
)
DATASET_SHA256 = "bb15da1b5fbb11b3cc6fd4c95eca256197573ecd066230eb3c1fe6898f27a578"
DATASET_ROWS = 9_990
DATASET_BYTES = 32_536_978

Span = score.Span
Document = score.Document
MetricAccumulator = score.MetricAccumulator
RecallAccumulator = score.RecallAccumulator
ContractAccumulator = score.ContractAccumulator
ResponseValidationError = score.ResponseValidationError
DEFAULT_CONFIGS = score.DEFAULT_CONFIGS
char_to_byte_offsets = score.char_to_byte_offsets
merge_intervals = score.merge_intervals
interval_length = score.interval_length
intersection_length = score.intersection_length
interval_is_covered = score.interval_is_covered
interval_overlaps = score.interval_overlaps
validate_response = score.validate_response
run_config = score.run_config
git_metadata = score.git_metadata


def fetch_dataset(path: Path) -> None:
    if path.exists():
        verify_dataset_file(path)
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    print(f"fetching pinned OpenPII validation split to {path}", file=sys.stderr)
    request = urllib.request.Request(
        DATASET_URL,
        headers={"User-Agent": "gaze-openpii-benchmark/1"},
    )
    digest = hashlib.sha256()
    byte_count = 0
    with urllib.request.urlopen(request, timeout=120) as response:
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as output:
            temporary = Path(output.name)
            try:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    output.write(chunk)
                    digest.update(chunk)
                    byte_count += len(chunk)
            except BaseException:
                temporary.unlink(missing_ok=True)
                raise
    if digest.hexdigest() != DATASET_SHA256 or byte_count != DATASET_BYTES:
        temporary.unlink(missing_ok=True)
        raise RuntimeError(
            "downloaded dataset failed its pinned SHA-256 or byte-size check"
        )
    os.replace(temporary, path)


def verify_dataset_file(path: Path) -> None:
    size = path.stat().st_size
    digest = score.sha256_file(path)
    if size != DATASET_BYTES or digest != DATASET_SHA256:
        raise RuntimeError(
            f"dataset integrity mismatch for {path}: size={size}, sha256={digest}"
        )


def load_dataset(
    path: Path,
    languages: frozenset[str] | None,
) -> tuple[list[Document], dict[str, object]]:
    verify_dataset_file(path)
    documents: list[Document] = []
    seen_uids: set[str] = set()
    label_counts: Counter[str] = Counter()
    language_counts: Counter[str] = Counter()
    region_counts: Counter[str] = Counter()
    source_counts: Counter[str] = Counter()
    total_rows = 0
    total_spans = 0

    with path.open(encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            total_rows += 1
            row = json.loads(line)
            if row.get("split") != "validation":
                raise ValueError(f"line {line_number}: expected validation split")
            uid = str(row["uid"])
            if uid in seen_uids:
                raise ValueError(f"line {line_number}: duplicate uid {uid}")
            seen_uids.add(uid)

            text = row["source_text"]
            offsets = score.char_to_byte_offsets(text)
            spans: list[Span] = []
            for entity in row["privacy_mask"]:
                start = entity["start"]
                end = entity["end"]
                if not isinstance(start, int) or not isinstance(end, int):
                    raise ValueError(f"line {line_number}: non-integer entity offset")
                if start < 0 or end <= start or end > len(text):
                    raise ValueError(f"line {line_number}: invalid entity bounds")
                if text[start:end] != entity["value"]:
                    raise ValueError(f"line {line_number}: entity value/offset mismatch")
                label = entity["label"]
                spans.append(Span(offsets[start], offsets[end], label))
                label_counts[label] += 1
                total_spans += 1

            language = row["language"]
            region = row["region"]
            language_counts[language] += 1
            region_counts[f"{language}-{region}"] += 1
            source_counts[row["source_dataset"]] += 1
            if languages is not None and language not in languages:
                continue
            documents.append(
                Document(
                    uid=uid,
                    text=text,
                    language=language,
                    region=region,
                    source_dataset=row["source_dataset"],
                    spans=tuple(spans),
                )
            )

    if total_rows != DATASET_ROWS:
        raise ValueError(f"expected {DATASET_ROWS} rows, found {total_rows}")
    if len(seen_uids) != DATASET_ROWS:
        raise ValueError("dataset UID cardinality mismatch")
    if not documents:
        raise ValueError("dataset filters selected zero documents")
    report: dict[str, object] = {
        "integrity": {
            "rows": total_rows,
            "unique_uids": len(seen_uids),
            "entities": total_spans,
            "sha256": DATASET_SHA256,
            "bytes": DATASET_BYTES,
            "label_counts": dict(sorted(label_counts.items())),
            "language_counts": dict(sorted(language_counts.items())),
            "region_counts": dict(sorted(region_counts.items())),
            "source_counts": dict(sorted(source_counts.items())),
        },
        "selection": score.population_summary(documents),
    }
    return documents, report


def build_binary(repo_root: Path, configs: tuple[str, ...]) -> Path:
    command = [
        "cargo",
        "build",
        "-q",
        "-p",
        "gaze-recognizers",
        "--example",
        "clean_for_bench",
    ]
    features = []
    if any("kiji" in config for config in configs):
        features.append("safety-net-kiji")
    if any("opf" in config for config in configs):
        features.append("safety-net-openai")
    if features:
        command.extend(["--features", ",".join(features)])
    subprocess.run(command, cwd=repo_root, check=True)
    binary = repo_root / "target/debug/examples/clean_for_bench"
    if not binary.is_file():
        raise FileNotFoundError(f"benchmark runner is missing: {binary}")
    return binary


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--dataset",
        type=Path,
        default=Path("target/bench-data/openpii-micro/validation.jsonl"),
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("target/bench-data/openpii-micro/gaze-benchmark.json"),
    )
    parser.add_argument(
        "--model-dir",
        type=Path,
        default=Path(
            os.environ.get(
                "GAZE_NER_MODEL_DIR",
                "~/.local/share/gaze/models/davlan-mbert-ner-hrl",
            )
        ).expanduser(),
    )
    parser.add_argument("--threshold", type=float, default=0.3)
    parser.add_argument(
        "--kiji-model-dir",
        type=Path,
        default=Path(
            os.environ.get(
                "GAZE_KIJI_DISTILBERT_MODEL_DIR",
                "~/.local/share/gaze/models/kiji-distilbert",
            )
        ).expanduser(),
    )
    parser.add_argument(
        "--config",
        action="append",
        choices=(
            "rule-floor-core",
            "rule-floor-extended",
            "pass2-ner",
            "full-stack-kiji-resolve",
            "full-stack-opf-resolve",
            "pass3-kiji",
            "pass3-opf",
            "pass3-locale-aware",
        ),
        help="repeat to benchmark more than one pipeline config",
    )
    parser.add_argument(
        "--language",
        action="append",
        help="ISO language code; repeat to select multiple languages",
    )
    parser.add_argument("--max-documents", type=int)
    parser.add_argument(
        "--sampling-seed", type=int, default=score.DEFAULT_SAMPLE_SEED
    )
    parser.add_argument(
        "--opf-command",
        type=Path,
        default=(
            Path(os.environ["GAZE_OPENAI_FILTER_OPF"])
            if os.environ.get("GAZE_OPENAI_FILTER_OPF")
            else None
        ),
    )
    parser.add_argument(
        "--opf-checkpoint",
        type=Path,
        default=(
            Path(os.environ["OPF_CHECKPOINT"])
            if os.environ.get("OPF_CHECKPOINT")
            else None
        ),
    )
    parser.add_argument(
        "--opf-daemon-socket",
        type=Path,
        default=(
            Path(os.environ["GAZE_OPF_DAEMON_SOCKET"])
            if os.environ.get("GAZE_OPF_DAEMON_SOCKET")
            else None
        ),
    )
    parser.add_argument("--no-download", action="store_true")
    parser.add_argument("--fetch-only", action="store_true")
    parser.add_argument("--skip-build", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = Path(__file__).resolve().parents[2]
    dataset_path = args.dataset if args.dataset.is_absolute() else repo_root / args.dataset
    output_path = args.output if args.output.is_absolute() else repo_root / args.output
    if args.no_download:
        verify_dataset_file(dataset_path)
    else:
        fetch_dataset(dataset_path)
    if args.fetch_only:
        print(
            json.dumps(
                {
                    "path": str(dataset_path),
                    "sha256": DATASET_SHA256,
                    "rows": DATASET_ROWS,
                    "bytes": DATASET_BYTES,
                },
                indent=2,
            )
        )
        return 0

    languages = frozenset(args.language) if args.language else None
    available_documents, dataset_report = load_dataset(dataset_path, languages)
    documents, sampling_report = score.stratified_sample(
        available_documents, args.max_documents, args.sampling_seed
    )
    configs = tuple(args.config) if args.config else DEFAULT_CONFIGS
    if any(
        config
        in {"pass2-ner", "full-stack-kiji-resolve", "full-stack-opf-resolve"}
        for config in configs
    ) and not args.model_dir.is_dir():
        raise FileNotFoundError(f"NER model directory does not exist: {args.model_dir}")
    if any("kiji" in config for config in configs) and not args.kiji_model_dir.is_dir():
        raise FileNotFoundError(
            f"Kiji model directory does not exist: {args.kiji_model_dir}"
        )
    if any("opf" in config for config in configs):
        if args.opf_command is None or not args.opf_command.is_file():
            raise FileNotFoundError("OPF command does not exist; pass --opf-command")
        if args.opf_checkpoint is None or not args.opf_checkpoint.is_dir():
            raise FileNotFoundError(
                "OPF checkpoint does not exist; pass --opf-checkpoint"
            )

    binary = (
        repo_root / "target/debug/examples/clean_for_bench"
        if args.skip_build
        else build_binary(repo_root, configs)
    )
    if not binary.is_file():
        raise FileNotFoundError(f"benchmark runner is missing: {binary}")
    runs = []
    for config in configs:
        print(f"running {config} on {len(documents)} documents", file=sys.stderr)
        runs.append(
            run_config(
                repo_root,
                binary,
                config,
                documents,
                args.model_dir,
                args.kiji_model_dir,
                args.opf_command,
                args.opf_checkpoint,
                args.opf_daemon_socket,
                args.threshold,
                output_path.parent,
            )
        )

    result = score.assemble_scorecard(
        repo_root=repo_root,
        dataset_metadata={
            "repository": DATASET_REPO,
            "revision": DATASET_REVISION,
            "file": DATASET_FILE,
            "url": DATASET_URL,
            "license": "CC-BY-4.0",
            "synthetic_only": True,
            "reserved_from_training": True,
        },
        dataset_report=dataset_report,
        sampling_report=sampling_report,
        parameters={
            "configs": list(configs),
            "languages": sorted(languages) if languages else "all",
            "max_documents": args.max_documents,
            "sampling_seed": args.sampling_seed,
            "ner_model_dir": str(args.model_dir),
            "kiji_model_dir": str(args.kiji_model_dir),
            "opf_command": str(args.opf_command) if args.opf_command else None,
            "opf_checkpoint": str(args.opf_checkpoint) if args.opf_checkpoint else None,
            "opf_daemon_socket": (
                str(args.opf_daemon_socket) if args.opf_daemon_socket else None
            ),
            "ner_threshold": args.threshold,
        },
        runs=runs,
    )
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {output_path}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
