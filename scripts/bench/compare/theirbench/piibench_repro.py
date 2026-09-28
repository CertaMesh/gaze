#!/usr/bin/env python3
"""Score Presidio on PIIBench-commercial with PIIBench's own harness.

Calls pii-bench's `run_benchmarking.run_presidio` (default AnalyzerEngine,
en_core_web_lg, its PRESIDIO_LABEL_MAP) and `compute_metrics` (seqeval,
exact span + type) unchanged on the prepared test_5k split. With
--predictions it instead scores another system's precomputed character spans
through the same `spans_to_bio` + `compute_metrics` path, after mapping each
native label with --label-map (tool label -> PIIBench label or null).

Only aggregate numbers are written.
"""

from __future__ import annotations

import argparse
import importlib.metadata
import json
import platform
import subprocess
import sys
from pathlib import Path

import piibench_commercial as build


def harness(checkout: Path):
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip()
    if head != build.PIIBENCH["commit"]:
        raise SystemExit(f"pii-bench checkout is at {head}, expected {build.PIIBENCH['commit']}")
    sys.path.insert(0, str(checkout))
    sys.path.insert(0, str(checkout / "src"))
    import run_benchmarking

    return run_benchmarking


def replay_label(candidates: list[str], rb, label_order: list[str]) -> str | None:
    """One PIIBench label per tool label for the typed seqeval match.

    Prefer the names PIIBench's own harness targets for Presidio, then the
    order of its label_mapping.json. Never chosen from test statistics.
    """
    preferred = [name for name in rb.PRESIDIO_LABEL_MAP.values() if name]
    for order in (preferred, label_order):
        for name in order:
            if name in candidates:
                return name
    return candidates[0] if candidates else None


def summarize(metrics: dict) -> dict:
    keys = ("f1", "precision", "recall")
    found = {key: metrics.get(key, metrics.get(f"overall_{key}")) for key in keys}
    if any(value is None for value in found.values()):
        raise ValueError(f"unexpected metrics shape: {sorted(metrics)}")
    return {key: round(float(value), 4) for key, value in found.items()}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--checkout", type=Path, required=True)
    parser.add_argument("--data-dir", type=Path, required=True, help="piibench_commercial.py --output-dir")
    parser.add_argument("--predictions", type=Path, help="JSONL: {index, spans: [[start, end, label], ...]}")
    parser.add_argument("--labels", type=Path, help="theirbench.py composed map: tool label -> PIIBench labels")
    parser.add_argument("--system", default="presidio-default")
    parser.add_argument("--max-records", type=int, help="smoke only; never published")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    rb = harness(args.checkout)
    manifest = json.loads((args.data_dir / "manifest.json").read_text(encoding="utf-8"))
    test = args.data_dir / "data/test_5k.jsonl"
    if build.sha256(test) != manifest["files"]["test_5k.jsonl"]:
        raise SystemExit("test_5k.jsonl differs from the build manifest")
    records = rb.load_test_records(test, args.max_records)
    if args.predictions is None:
        true, predicted, seconds = rb.run_presidio(records)
        versions = {name: importlib.metadata.version(name) for name in
                    ("presidio-analyzer", "spacy", "en-core-web-lg", "seqeval")}
    else:
        composed = json.loads(args.labels.read_text(encoding="utf-8"))
        order = [label[2:] for label in json.loads(
            (args.data_dir / "data/label_mapping.json").read_text(encoding="utf-8"))["labels"] if label.startswith("B-")]
        mapping = {tool: replay_label(natives, rb, order) for tool, natives in composed.items()}
        # As the comparison's typed scoring: a collision-family token earns no typed credit.
        mapping.update({label: None for label in mapping if label.startswith("custom:family:")})
        if args.system.startswith("presidio"):
            # PIIBench's own harness decides Presidio's labels, including the ones it discards.
            mapping.update({label: rb.PRESIDIO_LABEL_MAP[label] for label in mapping if label in rb.PRESIDIO_LABEL_MAP})
        rows = [json.loads(line) for line in args.predictions.read_text(encoding="utf-8").splitlines()]
        if [row["index"] for row in rows] != list(range(len(records))):
            raise SystemExit("predictions must cover every record in order")
        true, predicted, seconds = [r["labels"] for r in records], [], None
        for record, row in zip(records, rows):
            unknown = {label for _, _, label in row["spans"]} - mapping.keys()
            if unknown:
                raise SystemExit(f"unmapped labels {sorted(unknown)}; review {args.labels}")
            spans = [(s, e, mapping[label]) for s, e, label in row["spans"] if mapping[label]]
            predicted.append(rb.spans_to_bio(record["tokens"], spans))
        versions = {"seqeval": importlib.metadata.version("seqeval")}
    metrics = rb.compute_metrics(true, predicted, args.system)
    if not metrics["per_entity"]:
        # compute_metrics turns a seqeval exception into all-zero scores.
        raise SystemExit("seqeval failed inside the PIIBench harness; refusing a silent zero")
    report = {
        "benchmark": "PIIBench-commercial test_5k", "manifest_files": manifest["files"],
        "system": args.system, "records": len(records), "smoke_limit": args.max_records,
        "scorer": "pii-bench run_benchmarking.compute_metrics (seqeval, exact span + type)",
        "overall": summarize(metrics), "versions": versions,
        "published_full_mix": build.PUBLISHED if args.system == "presidio-default" else None,
        "hardware": platform.platform(), "predict_seconds": seconds and round(seconds, 1),
    }
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report["overall"]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
