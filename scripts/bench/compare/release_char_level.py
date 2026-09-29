#!/usr/bin/env python3
"""Character-level F-scores for tagged Gaze releases, from their committed records.

The comparison report scores an unreleased Gaze build and competitors; the panels
show tagged releases only. This tool measures each release the same way the
comparison scores a tool: it replays the release's committed observation record
(its final protection trace per document) over the same corpus, applies the
headline scored-label contract, and feeds the spans to `ComparisonMetrics`, so
the released Gaze and every competitor share one character-level scorer.

`record` needs the corpus (`--dataset`, the Dataiku holdout parquet) and writes
`docs/reference/benchmarks/release-char-level.json`. `check` needs neither corpus
nor model: it verifies each stored row against the committed record hash, the
release history's byte counts and the current metrics implementation hash.

    uv run --project scripts/bench python scripts/bench/compare/release_char_level.py \\
      record --dataset target/bench-data/dataiku-en-de/test.parquet
    python3 scripts/bench/compare/release_char_level.py check
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

BENCH = Path(__file__).resolve().parents[1]
REPO = BENCH.parents[1]
sys.path.insert(0, str(BENCH))
sys.path.insert(0, str(Path(__file__).resolve().parent))

import gaze_bench_score as score  # noqa: E402
import render_benchmark_doc as history_doc  # noqa: E402
import scorecard_record as record  # noqa: E402
from comparison_metrics import ComparisonMetrics  # noqa: E402
from tagged_gaze import require_tag  # noqa: E402

BENCH_DIR = REPO / "docs/reference/benchmarks"
OUTPUT = BENCH_DIR / "release-char-level.json"
METRICS = Path(__file__).with_name("comparison_metrics.py")
CONTRACT = "v3"
SCHEMA_VERSION = 1


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def observation_of(entry: dict) -> dict | None:
    """The release's committed observation record, wherever its row keeps it."""
    if entry.get("observation_record"):
        return entry["observation_record"]
    return next(
        (r["observation_record"] for r in entry.get("contract_results", ()) if r.get("observation_record")),
        None,
    )


def _release_records(history: dict) -> list[dict]:
    """Every non-provisional release that has a committed observation record."""
    return [
        entry for entry in history["releases"]
        if observation_of(entry) is not None and not entry.get("provisional")
    ]


def measure_release(entry: dict, layer_c: list[score.Document], mapping: dict,
                    typed_mapping: dict) -> dict:
    """Character-level metrics of the release's default arm on layer C under v3."""
    import compare  # noqa: E402  (imports the corpus stack lazily)

    contract = compare.runner.load_scored_label_contract(REPO, compare.CONTRACTS[CONTRACT])
    arm = history_doc.shipped_default_arm(entry)
    observation = observation_of(entry)
    path = BENCH_DIR / observation["file"]
    _, observations = record._read(path)
    by_id = {document.uid: document for document in layer_c}
    cell = ComparisonMetrics(mapping, None, typed_mapping)
    seen = 0
    for row in observations:
        if row["layer"] != "C" or row["config"] != arm:
            continue
        response = row["response"]
        if "pipeline_error_code" in response:
            raise ValueError(f"{entry['version']} refused {row['document_id']}")
        document = by_id[row["document_id"]]
        predictions = score.final_trace_predictions(document, response)
        compare.validate_labels(predictions, mapping)
        applied = score.apply_scored_label_contract([document], contract)[0]
        cell.add(applied, predictions)
        seen += 1
    if seen != len(layer_c):
        raise ValueError(f"{entry['version']}: record covers {seen} of {len(layer_c)} documents")
    result = cell.result()
    return {
        "arm": arm,
        "contract": CONTRACT,
        "documents": seen,
        "leaked_bytes": result["leaked_bytes"],
        "false_positive_bytes": result["false_positive_bytes"],
        "char_level": result["char_level"],
        "record_sha256": observation["sha256"],
    }


def build(dataset: Path) -> dict:
    import compare

    history = history_doc.load_history(history_doc.DEFAULT_HISTORY)
    layers, _ = compare.load_corpus(dataset, None)
    mapping = compare.load_mapping()["gaze"]
    typed = compare.typed_mapping_for_contract(mapping, CONTRACT)
    releases = {}
    for entry in _release_records(history):
        require_tag(entry["version"], "release char-level")
        releases[entry["version"]] = measure_release(entry, layers["C"], mapping, typed)
    return {
        "schema_version": SCHEMA_VERSION,
        "note": (
            "Character-level (Unicode code point), label-agnostic, micro P/R/F of each tagged "
            "release's shipped default on layer C under the v3 contract, scored by the same "
            "ComparisonMetrics as the competitors."
        ),
        "metrics_sha256": _sha256(METRICS),
        "releases": releases,
    }


def check(data: dict, history: dict) -> None:
    """Offline consistency: hashes, byte counts and tags. Needs no corpus."""
    if data["metrics_sha256"] != _sha256(METRICS):
        raise ValueError("release-char-level.json was measured with another comparison_metrics.py; rerun `record`")
    for version, row in data["releases"].items():
        require_tag(version, "release char-level")
        entry = next((e for e in history["releases"] if e["version"] == version), None)
        if entry is None:
            raise ValueError(f"{version} is not a release row in the history")
        if row["record_sha256"] != observation_of(entry)["sha256"]:
            raise ValueError(f"{version}: record hash differs from the release history")
        view = history_doc.contract_view(entry, history_doc.HEADLINE_CONTRACT)
        arm = view["arms"][history_doc.shipped_default_arm(entry)]
        if row["leaked_bytes"] != arm["surviving_pii_utf8_bytes"]:
            raise ValueError(
                f"{version}: leaked bytes {row['leaked_bytes']} differ from the history's "
                f"{arm['surviving_pii_utf8_bytes']}"
            )
        raw = history_doc.contract_view(entry, 2)  # v2 counts raw false positives, as ComparisonMetrics does
        if raw is not None:
            expected = raw["arms"][history_doc.shipped_default_arm(entry)]["false_positive_utf8_bytes"]
            if row["false_positive_bytes"] != expected:
                raise ValueError(
                    f"{version}: false-positive bytes {row['false_positive_bytes']} differ from "
                    f"the history's {expected}"
                )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    rec = sub.add_parser("record")
    rec.add_argument("--dataset", type=Path, required=True)
    sub.add_parser("check")
    args = parser.parse_args(argv)
    history = history_doc.load_history(history_doc.DEFAULT_HISTORY)
    if args.command == "record":
        data = build(args.dataset)
        check(data, history)
        OUTPUT.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"wrote {OUTPUT.relative_to(REPO)}")
        return 0
    check(json.loads(OUTPUT.read_text(encoding="utf-8")), history)
    print("release-char-level.json matches the release history and metrics implementation")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
