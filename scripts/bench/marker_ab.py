#!/usr/bin/env python3
"""Same-corpus A/B for the safety-net redaction marker (solo todo 3652).

Deleting a flagged span and writing a `[REDACTED:<class>]` marker over it must
redact the SAME spans. Only what stands in their place may differ. This script
checks that claim document by document, not only in aggregate:

  capture   run one arm's `clean_for_bench` over the full corpus (Dataiku EN/DE
            plus the A4 negative corpus) and keep every raw response, plus the
            terminal-admission probe lines the arm printed.
  compare   score both arms with EACH ARM'S OWN `gaze_bench_score` (the marker
            changed the trace/manifest contract, so one scorer cannot validate
            both), then diff the redacted set per document.

Why each arm needs its own scorer: under the marker, a redaction is a manifest
entry and the candidate scorer requires one per `redact` trace item. Base
responses have no such entry, so the candidate scorer would reject every base
document carrying a redaction -- which would read as a regression and is not.

The admission probe is an `eprintln!` applied to a THROWAWAY checkout of each
arm by `probe_patch()`, never committed. It prints one line per terminal
admission verdict so the seam-manufactured count can be measured before and
after rather than inferred. It executes only when a terminal verdict is
reached, so it does not move timing on documents that never reach one.

Two arms, both required:

  full-stack-nym-resolve  the SHIPPED default. On this corpus it resolves every
                          Nym suspect reversibly, so its `Redact` fallback -- the
                          only code the marker changes -- fires zero times. Its
                          A/B row is reported, labelled VACUOUS for the marker.
  full-stack-nym-redact   the same Nym model under `SafetyNetMode::Redact`, so
                          every suspect goes through `redact_safety_net_suspects`.
                          This is the row that tests the marker.

Reproduce (base = the parent of the marker change, candidate = the change):

  git worktree add --detach ../ab-base <base-sha>
  git worktree add --detach ../ab-cand <candidate-sha>
  # The redact arm is committed on the candidate. The base predates it, and the
  # two files it touches are byte-identical at both commits, so the same diff
  # applies to the base unchanged:
  git -C ../ab-base apply scripts/bench/marker_ab_nym_redact_arm.patch
  # Throwaway admission probe on BOTH arms (observability only, never commit):
  python3 scripts/bench/marker_ab.py probe-patch ../ab-base/crates/gaze/src/pipeline.rs
  python3 scripts/bench/marker_ab.py probe-patch ../ab-cand/crates/gaze/src/pipeline.rs
  # In each checkout (own CARGO_TARGET_DIR, pinned rustc):
  cargo build -p gaze-recognizers --example clean_for_bench --features safety-net-nym
  # Copy target/bench-data/dataiku-en-de/test.parquet (sha 916c6379...) into each.
  # Then, from each checkout's root, for each --config:
  uv run --project scripts/bench python <this script> capture \
      --binary target/debug/examples/clean_for_bench --config <arm> --out <arm>.jsonl
  uv run --project scripts/bench python <this script> compare \
      --base <base-arm>.jsonl --base-scripts ../ab-base/scripts/bench \
      --candidate <cand-arm>.jsonl --candidate-scripts ../ab-cand/scripts/bench \
      --report <arm>-report.json
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import platform
import statistics
import subprocess
import sys
import time
from collections import Counter
from pathlib import Path
from types import ModuleType

DEFAULT_CONFIG = "full-stack-nym-resolve"
CONTRACT = Path("docs/reference/benchmarks/scored-labels-v2.json")
DATASET = Path("target/bench-data/dataiku-en-de/test.parquet")
NEGATIVE = Path("crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl")
PROBE_TAG = "GAZE-AB-ADMISSION"

# Applied to a throwaway checkout only. Observability, no behaviour change: the
# probe runs inside `observe`, which already runs once per terminal verdict.
PROBE_ANCHOR = "    fn observe(&self, stage: &'static str) {\n"
PROBE_LINE = (
    "    fn observe(&self, stage: &'static str) {\n"
    '        eprintln!("' + PROBE_TAG + ' stage={stage} verdict={}", match self {\n'
    '            Self::Admit => "admit",\n'
    '            Self::FallbackIncomplete { .. } => "fallback_incomplete",\n'
    '            Self::SeamManufactured { .. } => "seam_manufactured",\n'
    '            Self::Unjudgeable { .. } => "unjudgeable",\n'
    "        });\n"
)


def probe_patch(pipeline_rs: Path) -> None:
    """Insert the admission probe into a throwaway checkout's pipeline.rs."""
    source = pipeline_rs.read_text(encoding="utf-8")
    if PROBE_TAG in source:
        return
    if source.count(PROBE_ANCHOR) != 1:
        raise SystemExit(f"probe anchor not unique in {pipeline_rs}")
    pipeline_rs.write_text(source.replace(PROBE_ANCHOR, PROBE_LINE), encoding="utf-8")


def load_module(name: str, scripts: Path) -> ModuleType:
    spec = importlib.util.spec_from_file_location(name, scripts / f"{name}.py")
    if spec is None or spec.loader is None:
        raise SystemExit(f"cannot load {name} from {scripts}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def load_documents(scripts: Path, repo: Path):
    sys.path.insert(0, str(scripts))
    score = load_module("gaze_bench_score", scripts)
    dataiku = load_module("dataiku_en_de_gaze_bench", scripts)
    runner = load_module("run_no_opf_benchmark", scripts)
    positive, _ = dataiku.load_documents(repo / DATASET)
    negative, _ = runner.load_negative_documents(repo / NEGATIVE)
    contract = score.load_scored_label_contract(repo / CONTRACT, display_path=str(CONTRACT))
    documents = score.apply_scored_label_contract(positive + negative, contract)
    return score, runner, documents


def host_line() -> dict[str, object]:
    def sysctl(key: str) -> str:
        try:
            return subprocess.run(
                ["sysctl", "-n", key], capture_output=True, text=True, check=True
            ).stdout.strip()
        except (OSError, subprocess.CalledProcessError):
            return "unknown"

    return {
        "chip": sysctl("machdep.cpu.brand_string"),
        "cores": sysctl("hw.ncpu"),
        "memory_bytes": sysctl("hw.memsize"),
        "os": platform.platform(),
        "load_average": os.getloadavg(),
    }


def capture(args: argparse.Namespace) -> int:
    repo = Path.cwd()
    scripts = repo / "scripts/bench"
    score, runner, documents = load_documents(scripts, repo)
    subprocess_module = load_module("bench_subprocess", scripts)

    environment = runner.build_no_opf_environment(os.environ)
    environment["GAZE_NER_MODEL_DIR"] = str(args.ner_model_dir.expanduser())
    environment["GAZE_NER_THRESHOLD"] = "0.3"
    environment["GAZE_NYM_MODEL_DIR"] = str(args.nym_model_dir.expanduser())

    probe_lines: list[str] = []
    original = subprocess_module.BenchSubprocess._stderr

    def keep_stderr(self, chunk):  # the harness counts stderr and drops it
        probe_lines.extend(
            line for line in chunk.decode("utf-8", "replace").splitlines()
            if PROBE_TAG in line
        )
        return original(self, chunk)

    subprocess_module.BenchSubprocess._stderr = keep_stderr
    load_before = os.getloadavg()
    records = []
    started = time.perf_counter()
    with subprocess_module.BenchSubprocess(
        [str(args.binary), "--config", args.config], cwd=repo, env=environment
    ) as process:
        for document in documents:
            request = {
                "fixture_id": document.uid,
                "locale_chain": document.locale_chain,
                "text": document.text,
            }
            t0 = time.perf_counter()
            response = process.exchange(request)
            records.append(
                {"uid": document.uid, "ms": (time.perf_counter() - t0) * 1000.0,
                 "response": response}
            )
    elapsed = time.perf_counter() - started

    args.out.write_text(
        "\n".join(json.dumps(record, ensure_ascii=False) for record in records) + "\n",
        encoding="utf-8",
    )
    meta = {
        "config": args.config,
        "documents": len(records),
        "elapsed_s": elapsed,
        "admission_probe": dict(Counter(
            line.split("verdict=", 1)[1].strip() for line in probe_lines
        )),
        "host_before": {"load_average": load_before},
        "host_after": host_line(),
        "git_head": subprocess.run(
            ["git", "rev-parse", "HEAD"], capture_output=True, text=True, cwd=repo
        ).stdout.strip(),
    }
    args.out.with_suffix(".meta.json").write_text(json.dumps(meta, indent=2) + "\n")
    print(json.dumps(meta, indent=2))
    return 0


def score_arm(score: ModuleType, documents, path: Path):
    by_uid = {document.uid: document for document in documents}
    metrics = score.MetricAccumulator()
    contract = score.ContractAccumulator()
    rejects: dict[str, str] = {}
    redacted: dict[str, list[tuple[int, int, str]]] = {}
    timings = []
    for line in path.read_text(encoding="utf-8").splitlines():
        record = json.loads(line)
        document = by_uid[record["uid"]]
        response = score.validate_response(document, record["response"])
        if "pipeline_error_code" in response:
            rejects[document.uid] = response["pipeline_error_code"]
            continue
        timings.append(record["ms"])
        metrics.add(document, score.final_trace_predictions(document, response))
        contract.add(response)
        redacted[document.uid] = sorted(
            (item["raw_start"], item["raw_end"], item["class"])
            for item in response["final_protection_trace"]
            if item["action"] == "redact"
        )
    return metrics.result(), contract.result(), rejects, redacted, timings


def p95(values: list[float]) -> float:
    return statistics.quantiles(values, n=100)[94] if len(values) > 1 else float("nan")


def compare(args: argparse.Namespace) -> int:
    repo = Path.cwd()
    # Documents and gold come from ONE loader: they are identical in both arms,
    # and loading them twice would only invite a silent mismatch.
    base_score, _, documents = load_documents(args.base_scripts, repo)
    base = score_arm(base_score, documents, args.base)
    for name in ("gaze_bench_score", "dataiku_en_de_gaze_bench", "run_no_opf_benchmark"):
        sys.modules.pop(name, None)
    sys.path.insert(0, str(args.candidate_scripts))
    candidate_score = load_module("gaze_bench_score", args.candidate_scripts)
    candidate = score_arm(candidate_score, documents, args.candidate)

    (b_metric, b_contract, b_rejects, b_redacted, b_ms) = base
    (c_metric, c_contract, c_rejects, c_redacted, c_ms) = candidate

    differing = []
    for uid in sorted(set(b_redacted) | set(c_redacted)):
        if b_redacted.get(uid) != c_redacted.get(uid):
            differing.append({
                "uid": uid,
                "base": b_redacted.get(uid),
                "candidate": c_redacted.get(uid),
                "base_rejected": b_rejects.get(uid),
                "candidate_rejected": c_rejects.get(uid),
            })

    report = {
        "leaked_labeled_utf8_bytes": {
            "base": b_metric["utf8_bytes"]["leaked"],
            "candidate": c_metric["utf8_bytes"]["leaked"],
        },
        "false_positive_utf8_bytes": {
            "base": b_metric["utf8_bytes"]["false_positive"],
            "candidate": c_metric["utf8_bytes"]["false_positive"],
        },
        "rejects": {"base": len(b_rejects), "candidate": len(c_rejects)},
        "rejects_candidate_only": sorted(set(c_rejects) - set(b_rejects)),
        "rejects_base_only": sorted(set(b_rejects) - set(c_rejects)),
        "restore_exact_documents": {
            "base": b_contract["restore_exact_documents"],
            "candidate": c_contract["restore_exact_documents"],
        },
        "documents_with_redactions": {"base": sum(bool(v) for v in b_redacted.values()),
                                      "candidate": sum(bool(v) for v in c_redacted.values())},
        "redacted_spans": {"base": sum(len(v) for v in b_redacted.values()),
                           "candidate": sum(len(v) for v in c_redacted.values())},
        "documents_where_redacted_set_differs": differing,
        "p95_ms": {"base": p95(b_ms), "candidate": p95(c_ms)},
        "p50_ms": {"base": statistics.median(b_ms), "candidate": statistics.median(c_ms)},
    }
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in report.items()
                      if k != "documents_where_redacted_set_differs"}, indent=2))
    print(f"documents where the redacted set differs: {len(differing)}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    cap = sub.add_parser("capture")
    cap.add_argument("--binary", type=Path, required=True)
    cap.add_argument("--out", type=Path, required=True)
    cap.add_argument("--config", default=DEFAULT_CONFIG,
                     choices=("full-stack-nym-resolve", "full-stack-nym-redact"))
    cap.add_argument("--ner-model-dir", type=Path,
                     default=Path("~/.local/share/gaze/models/davlan-mbert-ner-hrl"))
    cap.add_argument("--nym-model-dir", type=Path,
                     default=Path("~/.local/share/gaze/models/nym-small-int8"))
    cmp = sub.add_parser("compare")
    cmp.add_argument("--base", type=Path, required=True)
    cmp.add_argument("--base-scripts", type=Path, required=True)
    cmp.add_argument("--candidate", type=Path, required=True)
    cmp.add_argument("--candidate-scripts", type=Path, required=True)
    cmp.add_argument("--report", type=Path, required=True)
    patch = sub.add_parser("probe-patch")
    patch.add_argument("pipeline_rs", type=Path)
    args = parser.parse_args()
    if args.command == "capture":
        return capture(args)
    if args.command == "compare":
        return compare(args)
    probe_patch(args.pipeline_rs)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
