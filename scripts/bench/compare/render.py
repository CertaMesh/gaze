#!/usr/bin/env python3
"""Render the public competitor page from a measured report."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
BENCH = REPO / "scripts/bench"
import compare  # noqa: E402

ORDER = ("gaze", *compare.TOOLS)


def digest_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_current(report: dict[str, object]) -> None:
    """Reject a published comparison when benchmark inputs have changed."""
    corpus = report["corpus"]
    expected = {
        "scorer": (report["scorer_sha256"], digest_file(BENCH / "gaze_bench_score.py")),
        "dataset loader": (report["dataset_loader_sha256"], digest_file(BENCH / "dataiku_en_de_gaze_bench.py")),
        "benchmark runner": (report["runner_sha256"], digest_file(BENCH / "run_no_opf_benchmark.py")),
        "mapping": (report["mapping_sha256"], digest_file(Path(__file__).with_name("label-map.json"))),
        "model pins": (report.get("model_pins_sha256"), digest_file(compare.MODEL_PINS_PATH)),
        "comparison adapter": (report.get("compare_sha256"), digest_file(Path(compare.__file__))),
        "OPF adapter": (report.get("opf_adapter_sha256"), digest_file(BENCH / "opf_daemon.py")),
        "dependency pins": (report.get("requirements_sha256"), digest_file(Path(__file__).with_name("requirements.lock"))),
        "main dataset": (corpus["main_dataset"]["sha256"], compare.dataiku.DATASET_SHA256),
        "negative corpus": (
            corpus["negative_corpus_sha256"],
            digest_file(REPO / "crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl"),
        ),
        "agentic layers": (
            corpus["agentic"]["corpus_sha256"],
            compare.agentic.prepare(REPO).manifest["corpus_sha256"],
        ),
        "agentic scored labels": (
            report["contracts"]["agentic"], compare.agentic.load_contract(REPO).sha256,
        ),
    }
    for version, path in compare.CONTRACTS.items():
        expected[f"{version} scored labels"] = (
            report["contracts"][version], compare.runner.load_scored_label_contract(REPO, path).sha256,
        )
    for name, (recorded, current) in expected.items():
        if recorded != current:
            raise ValueError(f"{name} changed; rerun affected competitor rows on the same corpus")
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
    if report.get("harness_dirty") is not False:
        raise ValueError("public comparison needs a clean harness")
    required = {"presidio-all", "presidio-en", "presidio-en-de", "gliner"}
    if not required.issubset(tools) or ("opf" not in tools and "opf" not in report.get("skipped", {})):
        raise ValueError("public comparison needs every configured competitor or an explicit OPF skip")
    layer_ids = list(report["corpus"]["layers"])
    revisions = {row["gaze_revision"] for row in gaze.values()}
    if len(revisions) != 1:
        raise ValueError("Gaze contracts have different measured revisions")
    if "opf" in tools and tools["opf"]["provenance"]["runtime"].get("source_dirty") is not False:
        raise ValueError("OPF source was dirty or unverified during measurement")
    for name, tool in tools.items():
        if name.startswith("presidio"):
            models = tool["provenance"]["models"]
            if not models or any(not model.get("sha256") or not model.get("wheel_sha256") for model in models.values()):
                raise ValueError(f"{name} lacks a model hash")
        elif not tool["provenance"].get("model_sha256" if name == "gliner" else "checkpoint_sha256"):
            raise ValueError(f"{name} lacks a model hash")
    for version in versions:
        for layer in layer_ids:
            if layer not in gaze[version]["layers"]:
                raise ValueError(f"Gaze lacks layer {layer} under {version}")
            count = report["corpus"]["layers"][layer]["documents"]
            if gaze[version]["layers"][layer]["documents"] != count:
                raise ValueError(f"Gaze document count differs in {layer} under {version}")
            for name, tool in tools.items():
                if layer not in tool["contracts"][version]:
                    raise ValueError(f"{name} lacks layer {layer} under {version}")
                row = tool["contracts"][version][layer]
                if row["documents"] != count or row["processed_documents"] + row["skipped_documents"] != count:
                    raise ValueError(f"{name} document count differs in {layer} under {version}")
                if row["latency"]["samples"] != row["processed_documents"]:
                    raise ValueError(f"{name} latency includes skipped documents")

    opf_description = "OpenAI Privacy Filter (OPF) skipped."
    if "opf" in tools:
        runtime = tools["opf"]["provenance"]["runtime"]
        opf_source = runtime.get("source_revision")
        opf_description = f"OpenAI Privacy Filter (OPF) {runtime['version']}" + (f" at source `{opf_source}`" if opf_source else "") + "."
    latency_pairs = [
        (gaze[version]["layers"][layer]["latency"]["p50_ms"],
         tools["presidio-all"]["contracts"][version][layer]["latency"]["p50_ms"])
        for version in versions for layer in layer_ids
    ]
    latency_pairs = [(g, p) for g, p in latency_pairs if g is not None and p is not None]
    slower_rows = sum(g > p for g, p in latency_pairs)
    latency_note = (
        f"Gaze p50 exceeds Presidio all in {slower_rows}/{len(latency_pairs)} "
        "comparable layer-contract rows on this host. "
        if latency_pairs else "No comparable Gaze and Presidio all latency rows. "
    )
    skipped_example = tools["presidio-en"]["contracts"]["v3"].get("A")
    skipped_example_note = (
        f"For example, Presidio English-only v3 A leaks {skipped_example['leaked_bytes']:,} B, "
        f"including {skipped_example['skipped_gold_bytes']:,} B of scored gold from "
        f"{skipped_example['skipped_documents']:,} skipped non-English documents. "
        if skipped_example and skipped_example["skipped_documents"] else ""
    )
    lines = [
        "# Competitor comparison",
        "",
        "Same corpus and scorer; tools run with documented configurations. "
        "UTF-8 byte counts use the Gaze scorer. For v3, FP is the scorer's "
        "false-positive count after its audited gold-gap credit. CPU-host p50/p95 "
        "is warm per-document wall-clock inference/clean time on the same machine. "
        "Presidio all runs English, German, Dutch, French, and Portuguese spaCy models "
        "with the documented German recognizers. Presidio English default is a secondary row. "
        "Latency includes processed documents only. " + latency_note +
        "This measures detection; competitor restore and manifest behavior is not scored.",
        "",
        "Leaked and false-positive byte counts are class-agnostic. A skipped document's "
        "scored gold counts in full as leaked. Subtract Skipped gold B from Leaked B to "
        "get leakage on processed documents. " + skipped_example_note +
        "The reviewed label map affects only v3's repeated-gold credit.",
        "",
        f"Gaze measured at `{gaze['v3']['gaze_revision']}` "
        f"(release `{report['latest_release_at_measurement']['version']}`). "
        f"Home-normalized setup policy SHA-256: `{report['policy_sha256_home_normalized']}`. "
        "The measured call scopes differ by tool, so latency is descriptive.",
        "",
        "Competitor runtimes: Presidio "
        f"{tools['presidio-en']['provenance']['analyzer_version']} with spaCy "
        f"{tools['presidio-en']['provenance']['spacy_version']}; GLiNER "
        f"{tools['gliner']['provenance']['gliner_version']} at model snapshot "
        f"`{tools['gliner']['provenance']['model_snapshot']}`. "
        f"GLiNER uses model-card labels, library threshold 0.5. "
        f"{opf_description}",
        "",
        f"Aggregate source: [`{source}`]({source}). Raw document outputs are not published.",
        "",
        "| Contract | Layer | Tool | Leaked B | FP B | Processed | Skipped | Skipped gold B | CPU-host p50 ms | CPU-host p95 ms |",
        "|---|---|---|---:|---:|---:|---:|---:|---:|---:|",
    ]
    lower_leak = []
    lower_fp_at_equal_leak = []
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
                p50 = "n/a" if latency["p50_ms"] is None else f"{latency['p50_ms']:.1f}"
                p95 = "n/a" if latency["p95_ms"] is None else f"{latency['p95_ms']:.1f}"
                lines.append(
                    f"| {version} | {layer} | {name} | {row['leaked_bytes']:,} | {fp:,} | "
                    f"{row.get('processed_documents', row['documents']):,} | "
                    f"{row.get('skipped_documents', 0):,} | {row.get('skipped_gold_bytes', 0):,} | "
                    f"{p50} | {p95} |"
                )
                if name != "gaze" and version == "v3":
                    gaze_fp = gaze_row["false_positive_bytes_after_gold_gap"]
                    if gaze_fp is None:
                        gaze_fp = gaze_row["false_positive_bytes"]
                    if row["leaked_bytes"] < gaze_row["leaked_bytes"]:
                        lower_leak.append(
                            f"{layer}: {name} leaks {row['leaked_bytes']:,} B versus Gaze "
                            f"{gaze_row['leaked_bytes']:,} B"
                        )
                    elif row["leaked_bytes"] == gaze_row["leaked_bytes"] and fp < gaze_fp:
                        lower_fp_at_equal_leak.append(
                            f"{layer}: {name} has {fp:,} FP B versus Gaze {gaze_fp:,} FP B"
                        )
    lines.extend([
        "", "**Where Gaze leaks more under v3:** "
        + ("; ".join(lower_leak) if lower_leak else "none in these measured rows") + ".",
        "", "**Where a competitor has fewer false positives at equal v3 leakage:** "
        + ("; ".join(lower_fp_at_equal_leak) if lower_fp_at_equal_leak else "none in these measured rows") + ".",
    ])
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
