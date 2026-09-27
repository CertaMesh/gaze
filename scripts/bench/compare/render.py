#!/usr/bin/env python3
"""Render the public competitor page from a measured report."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

ORDER = ("gaze", "presidio-en", "presidio-en-de", "gliner", "opf")
REPO = Path(__file__).resolve().parents[3]
BENCH = REPO / "scripts/bench"


def digest_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_current(report: dict[str, object]) -> None:
    """Reject a published comparison when benchmark inputs have changed."""
    sys.path.insert(0, str(BENCH))
    import agentic_layers
    import dataiku_en_de_gaze_bench as dataiku
    import run_no_opf_benchmark as runner

    corpus = report["corpus"]
    expected = {
        "scorer": (report["scorer_sha256"], digest_file(BENCH / "gaze_bench_score.py")),
        "mapping": (report["mapping_sha256"], digest_file(Path(__file__).with_name("label-map.json"))),
        "main dataset": (corpus["main_dataset"]["sha256"], dataiku.DATASET_SHA256),
        "negative corpus": (
            corpus["negative_corpus_sha256"],
            digest_file(REPO / "crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl"),
        ),
        "agentic layers": (
            corpus["agentic"]["corpus_sha256"],
            agentic_layers.prepare(REPO).manifest["corpus_sha256"],
        ),
        "agentic scored labels": (
            report["contracts"]["agentic"], agentic_layers.load_contract(REPO).sha256,
        ),
    }
    for version, path in (("v1", None), ("v2", Path("docs/reference/benchmarks/scored-labels-v2.json")),
                          ("v3", Path("docs/reference/benchmarks/scored-labels-v3.json"))):
        expected[f"{version} scored labels"] = (
            report["contracts"][version], runner.load_scored_label_contract(REPO, path).sha256,
        )
    for name, (recorded, current) in expected.items():
        if recorded != current:
            raise ValueError(f"{name} changed; rerun every competitor")
    pack_dir = REPO / "docs/reference/benchmarks/variant-packs"
    current_packs = {
        path.relative_to(REPO).as_posix(): digest_file(path)
        for path in sorted(pack_dir.glob("*.jsonl"))
    }
    recorded_packs = {item["path"]: item["sha256"] for item in corpus["packs"]}
    if recorded_packs != current_packs:
        raise ValueError("variant packs changed; rerun every competitor, including sealed partitions")


def render(report: dict[str, object], source: str) -> str:
    gaze = report["gaze"]
    tools = report["tools"]
    versions = ("v3", "v2", "v1")
    if set(gaze) != set(versions):
        raise ValueError("public comparison needs a Gaze scorecard for v3, v2, and v1")
    layer_ids = list(report["corpus"]["layers"])
    for version in versions:
        for layer in layer_ids:
            if layer not in gaze[version]["layers"]:
                raise ValueError(f"Gaze lacks layer {layer} under {version}")
            for name, tool in tools.items():
                if layer not in tool["contracts"][version]:
                    raise ValueError(f"{name} lacks layer {layer} under {version}")

    lines = [
        "# Competitor comparison",
        "",
        "Same corpus and scorer; tools run with their documented defaults. "
        "UTF-8 byte counts use the Gaze scorer. For v3, FP is the scorer's "
        "false-positive count after its audited gold-gap credit. CPU p50/p95 "
        "is warm per-document inference/clean time on the same machine. "
        "Presidio English default leaves German documents unprocessed. "
        "This measures detection; competitor restore and manifest behavior is not scored.",
        "",
        f"Aggregate source: [`{source}`]({source}). Raw document outputs are not published.",
        "",
        "| Contract | Layer | Tool | Leaked B | FP B | CPU p50 ms | CPU p95 ms |",
        "|---|---|---|---:|---:|---:|---:|",
    ]
    better = []
    for version in versions:
        for layer in layer_ids:
            gaze_row = gaze[version]["layers"][layer]
            for name in ORDER:
                if name == "gaze":
                    row = gaze_row
                elif name in tools:
                    row = tools[name]["contracts"][version][layer]
                else:
                    continue
                fp = row["false_positive_bytes"]
                if version == "v3" and row["false_positive_bytes_after_gold_gap"] is not None:
                    fp = row["false_positive_bytes_after_gold_gap"]
                latency = row["latency"]
                lines.append(
                    f"| {version} | {layer} | {name} | {row['leaked_bytes']:,} | {fp:,} | "
                    f"{latency['p50_ms']:.1f} | {latency['p95_ms']:.1f} |"
                )
                if name != "gaze" and row["leaked_bytes"] < gaze_row["leaked_bytes"]:
                    better.append(f"{version}/{layer}: {name} leaks {row['leaked_bytes']:,} B versus Gaze {gaze_row['leaked_bytes']:,} B")
    lines.extend(["", "**Where Gaze leaks more:** " + ("; ".join(better) if better else "none in these measured rows") + "."])
    skipped = report.get("skipped", {})
    if skipped:
        lines.extend(["", "**Skipped:** " + "; ".join(f"{name}: {reason}" for name, reason in skipped.items()) + "."])
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--page", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = json.loads(args.report.read_text(encoding="utf-8"))
    validate_current(report)
    page = render(report, args.report.name)
    if args.check:
        if args.page.read_text(encoding="utf-8") != page:
            raise ValueError("competitor page is stale; rerender it")
        return
    args.page.write_text(page, encoding="utf-8")


if __name__ == "__main__":
    main()
