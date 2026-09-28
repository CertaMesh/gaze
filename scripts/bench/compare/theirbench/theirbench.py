#!/usr/bin/env python3
"""Score Gaze and every compared tool on competitors' own benchmarks.

Report-only: these sets never inform Gaze rule design or tuning. The byte
and entity metrics come from the same `ComparisonMetrics` code as the main
comparison; tool configurations come from `compare.build_backend`, so a
competitor row here is the exact configuration of its main-corpus row.
Every gold label counts (no scored-label contract), as in the sealed sets.

Outputs: the aggregate JSON (--output) and, for scoring with each
benchmark's own evaluator, per-document character spans under
--predictions-dir. Predictions contain offsets and labels only, never text,
and stay out of the repository.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import os
import platform
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable, Mapping, Sequence

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

import compare  # noqa: E402
import backends  # noqa: E402
from cpu_contention import ForeignCpuSampler  # noqa: E402
import loaders  # noqa: E402
from comparison_metrics import ComparisonMetrics  # noqa: E402

score = loaders.score
BENCHMARKS = ("presidio-research", "piibench-commercial")
GAZE_ROWS = ("gaze-rules-only", "gaze-rules-ner", "gaze-full")


def tool_family(name: str) -> str:
    for family in ("presidio", "gliner", "datafog-core", "scrubadub", "opf"):
        if name.startswith(family):
            return family
    if name.startswith("datafog-"):
        return "datafog-python"
    if name.startswith("gaze"):
        return "gaze"
    raise ValueError(f"unknown tool {name}")


def load_benchmark(args: argparse.Namespace) -> tuple[dict[str, list[score.Document]], dict[str, object]]:
    """Return {"test": documents} plus "validation" when the set publishes one."""
    if args.benchmark == "presidio-research":
        documents, identity = loaders.load_presidio_research(args.presidio_research_checkout)
        return {"test": documents}, identity
    splits, identity = loaders.load_piibench_commercial(args.piibench_data)
    return splits, identity


def preflight_sample(documents: Sequence[score.Document], per_source: int) -> list[score.Document]:
    taken: dict[str, int] = {}
    sample = []
    for document in documents:
        if taken.get(document.source_dataset, 0) < per_source:
            taken[document.source_dataset] = taken.get(document.source_dataset, 0) + 1
            sample.append(document)
    return sample


def common_intersection(composed: Mapping[str, Mapping[str, Sequence[str]]]) -> frozenset[str]:
    """Gold labels every compared system claims; unsupported classes drop out."""
    covered = [frozenset(label for labels in table.values() for label in labels) for table in composed.values()]
    return frozenset.intersection(*covered)


class Cells:
    def __init__(self, mapping: Mapping[str, Sequence[str]], common: frozenset[str]) -> None:
        typed = backends.typed_mapping(dict(mapping))
        self.views = {
            "product_coverage": ComparisonMetrics(mapping, None, typed),
            "common_intersection": ComparisonMetrics(mapping, common, typed),
        }

    def add(self, document: score.Document, predictions: Sequence[score.Span]) -> None:
        for view in self.views.values():
            view.add(document, predictions)

    def result(self) -> dict[str, object]:
        return {name: view.result() for name, view in self.views.items()}


def char_spans(document: score.Document, predictions: Sequence[score.Span]) -> list[list[object]]:
    """Byte offsets back to character offsets, for the benchmarks' own scorers."""
    offsets = score.char_to_byte_offsets(document.text)
    to_char = {byte: index for index, byte in enumerate(offsets)}
    return [[to_char[span.start], to_char[span.end], span.label] for span in predictions]


def measure_tool(
    name: str, predictor: Callable[[score.Document], list[score.Span]],
    splits: Mapping[str, Sequence[score.Document]], mapping: Mapping[str, Sequence[str]],
    common: frozenset[str], predictions_dir: Path,
) -> dict[str, object]:
    result: dict[str, object] = {}
    for split, documents in splits.items():
        cells, timers = Cells(mapping, common), []
        with (predictions_dir / f"{name}.{split}.jsonl").open("w", encoding="utf-8") as sink:
            for index, document in enumerate(documents):
                started = time.perf_counter()
                predictions = predictor(document)
                timers.append((time.perf_counter() - started) * 1000)
                if name == "opf":
                    # As compare.measure does: OPF's native secret is scored as custom:secret.
                    predictions = [dataclasses.replace(s, label="custom:secret") if s.label == "secret" else s
                                   for s in predictions]
                compare.validate_labels(predictions, mapping)
                cells.add(document, predictions)
                sink.write(json.dumps({"index": index, "spans": char_spans(document, predictions)}) + "\n")
        result[split] = {**cells.result(), "latency": latency(timers)}
    return result


def measure_gaze(
    name: str, args: argparse.Namespace, policy: Path,
    splits: Mapping[str, Sequence[score.Document]], mapping: Mapping[str, Sequence[str]],
    common: frozenset[str], predictions_dir: Path, diagnostics: Path,
) -> dict[str, object]:
    result: dict[str, object] = {}
    for split, documents in splits.items():
        cells = Cells(mapping, common)
        spans: dict[str, list[list[object]]] = {}

        def record(_config: str, document: score.Document, response: dict[str, object], _m: object) -> None:
            if "pipeline_error_code" in response:
                raise RuntimeError(f"{name} refused {document.uid}; refusal handling must be disclosed first")
            predictions = score.final_trace_predictions(document, response)
            compare.validate_labels(predictions, mapping)
            cells.add(document, predictions)
            spans[document.uid] = char_spans(document, predictions)

        run = score.run_config(
            compare.REPO, args.gaze_binary, "policy-file", documents, args.gaze_model_dir,
            None, None, None, 0.5, diagnostics / name / split,
            policy_path=policy, record_document=record,
        )
        with (predictions_dir / f"{name}.{split}.jsonl").open("w", encoding="utf-8") as sink:
            for index, document in enumerate(documents):
                sink.write(json.dumps({"index": index, "spans": spans[document.uid]}) + "\n")
        clean = run["latency_ms"]["clean_ms"]
        result[split] = {**cells.result(), "latency": {
            "p50_ms": round(clean["median"], 3), "p95_ms": round(clean["p95"], 3), "samples": len(documents)}}
    return result


# Tool families whose typed cells stay held: their label maps pass through labels
# the comparison's typed-scoring fix touched (collision families, secrets).
TYPED_HOLD_PATTERN = ("custom:family:", "secret", "password", "token", "credential")


def typed_hold(mappings: Mapping[str, Mapping[str, Sequence[str]]]) -> list[str]:
    return sorted(family for family, table in mappings.items()
                  if any(marker in label for label in table for marker in TYPED_HOLD_PATTERN))


def rescore(report: dict, splits: Mapping[str, Sequence[score.Document]],
            composed: Mapping[str, Mapping[str, Sequence[str]]], common: frozenset[str],
            predictions_dir: Path) -> None:
    """Recompute every row from stored predictions; only typed cells may change."""
    for name, result in report["rows"].items():
        mapping = composed[tool_family(name)]
        for split, documents in splits.items():
            cells = Cells(mapping, common)
            rows = (predictions_dir / f"{name}.{split}.jsonl").read_text(encoding="utf-8").splitlines()
            if len(rows) != len(documents):
                raise SystemExit(f"{name}/{split}: stored predictions do not cover the documents")
            for document, line in zip(documents, rows):
                stored = json.loads(line)["spans"]
                cells.add(document, compare.byte_spans(document.text, [tuple(span) for span in stored]))
            fresh = cells.result()
            for view, values in fresh.items():
                old = result[split][view]
                moved = sorted(key for key in values if key != "typed_entities" and values[key] != old[key])
                if moved:
                    raise SystemExit(f"rescore moved non-typed metrics for {name}/{split}/{view}: {moved}")
                old["typed_entities"] = values["typed_entities"]


def latency(timers: Sequence[float]) -> dict[str, object]:
    return {"p50_ms": round(score.percentile(timers, 0.5), 3) if timers else None,
            "p95_ms": round(score.percentile(timers, 0.95), 3) if timers else None,
            "samples": len(timers)}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--benchmark", choices=BENCHMARKS, required=True)
    parser.add_argument("--presidio-research-checkout", type=Path)
    parser.add_argument("--piibench-data", type=Path, help="piibench_commercial.py --output-dir")
    parser.add_argument("--tool", action="append", choices=[*compare.TOOLS, *GAZE_ROWS])
    parser.add_argument("--gaze-binary", type=Path)
    parser.add_argument("--gaze-model-dir", type=Path)
    parser.add_argument("--gaze-policy-rules", type=Path)
    parser.add_argument("--gaze-policy-rules-ner", type=Path)
    parser.add_argument("--predictions-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rescore", action="store_true",
                        help="recompute metrics from stored predictions with the pinned metric code; no models run")
    parser.add_argument("--preflight", type=int, metavar="N",
                        help="STEER 4 label preflight: first N documents per source; never published")
    backends.add_tool_arguments(parser)
    args = parser.parse_args()
    pinned = backends.verify_pinned_comparison()
    splits, identity = load_benchmark(args)
    if args.preflight:
        splits = {split: preflight_sample(documents, args.preflight) for split, documents in splits.items()}
    mappings = compare.load_mapping()
    selected = args.tool or [*GAZE_ROWS, *compare.TOOLS]
    # The whole roster, not just --tool: the common intersection must not depend
    # on which subset one invocation runs (runs resume into one report).
    composed = {
        family: loaders.compose_mapping(family, mappings[family], args.benchmark)
        for family in sorted({tool_family(name) for name in (*GAZE_ROWS, *compare.TOOLS)})
    }
    common = common_intersection(composed)
    args.predictions_dir.mkdir(parents=True, exist_ok=True)
    for family, table in composed.items():
        # Read by the benchmarks' own scorers when they replay these spans.
        (args.predictions_dir / f"labels.{family}.json").write_text(
            json.dumps({label: list(labels) for label, labels in table.items()}, indent=2) + "\n",
            encoding="utf-8")
    report = json.loads(args.output.read_text(encoding="utf-8")) if args.output.exists() else {
        "schema_version": 1, "benchmark": args.benchmark, "identity": identity,
        "report_only": "never used to design or tune Gaze rules",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=compare.REPO, text=True).strip(),
        "harness_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=compare.REPO, text=True).strip()),
        "gaze_crates_tree": compare.crates_tree("HEAD"),
        "label_maps_sha256": compare.digest_file(loaders.LABEL_MAPS),
        "comparison_revision": backends.COMPARISON_REVISION, "comparison_sha256": pinned,
        "chart_configs": backends.chart_configs(),
        "mapping_sha256": compare.digest_file(compare.MAP_PATH),
        "hardware": platform.platform(), "device": "cpu",
        "common_intersection_labels": sorted(common),
        "splits": {split: {"documents": len(docs), "ids_sha256": score.document_ids_digest([d.uid for d in docs])}
                   for split, docs in splits.items()},
        "rows": {}, "provenance": {}, "preflight": args.preflight,
    }
    if report["identity"] != identity or report["common_intersection_labels"] != sorted(common):
        raise SystemExit("--output was produced for a different benchmark identity or roster")
    report["typed_hold"] = typed_hold(mappings)
    if args.rescore:
        if not report["rows"]:
            raise SystemExit("--rescore needs a measured --output report")
        rescore(report, splits, composed, common, args.predictions_dir)
        report["rescored_with"] = {
            "comparison_revision": backends.COMPARISON_REVISION, "comparison_sha256": pinned,
            "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=compare.REPO, text=True).strip(),
            "harness_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=compare.REPO, text=True).strip()),
            "at": datetime.now(timezone.utc).isoformat(),
        }
        compare.write_report(args.output, report)
        print(f"THEIRBENCH_RESCORED {args.benchmark} {len(report['rows'])} rows", file=sys.stderr)
        return 0
    with tempfile.TemporaryDirectory(prefix="gaze-theirbench-") as scratch:
        for name in selected:
            if name in report["rows"]:
                continue
            mapping = composed[tool_family(name)]
            if name in GAZE_ROWS:
                policy = {"gaze-rules-only": args.gaze_policy_rules,
                          "gaze-rules-ner": args.gaze_policy_rules_ner,
                          "gaze-full": args.gaze_policy}[name]
                if policy is None or args.gaze_binary is None or args.gaze_model_dir is None:
                    raise SystemExit(f"{name} needs --gaze-binary, --gaze-model-dir and its policy")
                with ForeignCpuSampler() as watch:
                    report["rows"][name] = measure_gaze(name, args, policy, splits, mapping, common,
                                                        args.predictions_dir, Path(scratch))
                report["provenance"][name] = {"policy_sha256_home_normalized": compare.normalized_policy_sha256(
                    policy, compare.digest_file(policy)), "cpu": watch.result()}
            else:
                backend, provenance, _ = backends.build_backend(name, args, mappings, Path(scratch))
                if backend is None:
                    report["provenance"][name] = provenance
                    continue
                try:
                    backend.predict(score.Document("warmup", "alice@example.invalid", "en", "", "synthetic", ()))
                    with ForeignCpuSampler() as watch:
                        report["rows"][name] = measure_tool(name, backend.predict, splits, mapping, common,
                                                            args.predictions_dir)
                    report["provenance"][name] = {**provenance, "cpu": watch.result()}
                finally:
                    if hasattr(backend, "close"):
                        backend.close()
            compare.write_report(args.output, report)
            print(f"THEIRBENCH_DONE {args.benchmark} {name}", file=sys.stderr, flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
