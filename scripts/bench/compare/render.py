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
from layer_display import layer_display_name  # noqa: E402
from markdown_table import table_header  # noqa: E402

ORDER = ("gaze", *compare.TOOLS)

LOSS_METRICS = (
    ("Leaked B", ("leaked_bytes",), "lower"),
    ("FP B", ("false_positive_bytes",), "lower"),
    ("Leaking docs", ("leaking_documents",), "lower"),
    ("Doc leak %", ("document_leak_rate",), "lower"),
    ("Leaking entities", ("leaking_entities",), "lower"),
    ("Entity leak %", ("leaked_entity_rate",), "lower"),
    ("Redaction load %", ("redaction_load",), "lower"),
    ("TP", ("typed_entities", "tp"), "higher"),
    ("FP", ("typed_entities", "fp"), "lower"),
    ("FN", ("typed_entities", "fn"), "lower"),
    ("Entity P", ("typed_entities", "precision"), "higher"),
    ("Entity R", ("typed_entities", "recall"), "higher"),
    ("F1", ("typed_entities", "f1"), "higher"),
    ("F2", ("typed_entities", "f2"), "higher"),
)


def _metric_value(row: dict[str, object], path: tuple[str, ...]) -> int | float:
    value = row
    for key in path:
        value = value[key]
    return value


def _display_metric(value: int | float, name: str) -> str:
    if name.endswith("%"):
        return f"{100 * value:.1f}%"
    if isinstance(value, float):
        return f"{value:.3f}"
    return f"{value:,}"


def enumerate_gaze_losses(report: dict[str, object], versions: tuple[str, ...],
                          layers: list[str]) -> tuple[dict[str, int], list[str]]:
    """List every scored metric on which a measured competitor beats Gaze."""
    counts: dict[str, int] = {}
    entries = []
    for version in versions:
        for layer in layers:
            gaze = report["gaze"][version]["layers"][layer]
            for name in ORDER[1:]:
                if name not in report["tools"]:
                    continue
                tool = report["tools"][name]["contracts"][version][layer]
                differences = []
                for metric in ("Leaked B", "FP B"):
                    key = "leaked_bytes" if metric == "Leaked B" else "false_positive_bytes"
                    left, right = tool[key], gaze[key]
                    if metric == "FP B" and version == "v3":
                        left = tool.get("false_positive_bytes_after_gold_gap")
                        right = gaze.get("false_positive_bytes_after_gold_gap")
                        if left is None:
                            left = tool[key]
                        if right is None:
                            right = gaze[key]
                    if left < right:
                        counts[f"full aggregate {metric}"] = counts.get(f"full aggregate {metric}", 0) + 1
                        differences.append(f"{metric} {_display_metric(left, metric)} vs {_display_metric(right, metric)}")
                if differences:
                    entries.append(f"{version} {layer_display_name(layer)} full aggregate {name}: "
                                   + "; ".join(differences))
                for view in ("product_coverage", "common_intersection"):
                    for split in ("full", "validation", "test"):
                        a = tool["metrics"][view][split]
                        b = gaze["metrics"][view][split]
                        differences = []
                        for metric, path, better in LOSS_METRICS:
                            left, right = _metric_value(a, path), _metric_value(b, path)
                            if (left < right if better == "lower" else left > right):
                                counts[metric] = counts.get(metric, 0) + 1
                                differences.append(
                                    f"{metric} {_display_metric(left, metric)} vs {_display_metric(right, metric)}"
                                )
                        if differences:
                            entries.append(
                                f"{version} {layer_display_name(layer)} {view} {split} {name}: "
                                + "; ".join(differences)
                            )
    return counts, entries


def digest_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_current(report: dict[str, object]) -> None:
    """Reject a published comparison when benchmark inputs have changed."""
    corpus = report["corpus"]
    expected = {
        "scorer": (report["scorer_sha256"], digest_file(BENCH / "gaze_bench_score.py")),
        "dataset loader": (report["dataset_loader_sha256"], digest_file(BENCH / "dataiku_en_de_gaze_bench.py")),
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
    if report.get("schema_version", 1) >= 2:
        expected["scrubadub dependency pins"] = (
            report.get("scrubadub_requirements_sha256"),
            digest_file(Path(__file__).with_name("requirements-scrubadub.lock")),
        )
        expected["comparison metrics"] = (
            report.get("comparison_metrics_sha256"),
            digest_file(Path(__file__).with_name("comparison_metrics.py")),
        )
        expected["foreign CPU sampler"] = (
            report.get("cpu_contention_sha256"),
            digest_file(Path(__file__).with_name("cpu_contention.py")),
        )
        expected["README chart configurations"] = (
            report.get("chart_config_sha256"),
            digest_file(Path(__file__).with_name("chart-configs.json")),
        )
        expected["report finalizer"] = (
            report.get("finalizer_sha256"),
            digest_file(Path(__file__).with_name("finalize_report.py")),
        )
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
    if report.get("schema_version", 1) >= 2:
        required.update(set(compare.TOOLS) - {"opf"})
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
            if name.startswith("presidio-strong") and not tool["provenance"].get("transformer", {}).get("sha256"):
                raise ValueError(f"{name} lacks a transformer hash")
        elif name.startswith("gliner") and not tool["provenance"].get("model_sha256"):
            raise ValueError(f"{name} lacks a model hash")
        elif name.startswith("gliner") and not tool["provenance"].get("tokenizer_sha256") and report.get("schema_version", 1) >= 2:
            raise ValueError(f"{name} lacks a tokenizer hash")
        elif name == "opf" and not tool["provenance"].get("checkpoint_sha256"):
            raise ValueError(f"{name} lacks a model hash")
        elif name == "datafog-gliner" and not tool["provenance"].get("model_sha256"):
            raise ValueError(f"{name} lacks a model hash")
        elif name == "datafog-gliner" and not tool["provenance"].get("tokenizer_sha256"):
            raise ValueError(f"{name} lacks a tokenizer hash")
        elif name == "datafog-spacy" and not tool["provenance"].get("spacy_model", {}).get("sha256"):
            raise ValueError(f"{name} lacks a model hash")
        elif name == "scrubadub-spacy" and not tool["provenance"].get("spacy_model", {}).get("sha256"):
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
    latency_publishable = report.get("latency_validity", {}).get("publishable", report.get("schema_version", 1) < 2)
    if report.get("schema_version", 1) >= 2 and "latency_validity" not in report:
        raise ValueError("comparison report lacks latency validity evidence")
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
    ) if latency_publishable else "Latency was not measured under a quiet machine; timing comparisons are withheld. "
    skipped_example = tools["presidio-en"]["contracts"]["v3"].get("A")
    skipped_example_note = (
        f"For example, Presidio English-only v3 {layer_display_name('A')} leaks "
        f"{skipped_example['leaked_bytes']:,} B, "
        f"including {skipped_example['skipped_gold_bytes']:,} B of scored gold from "
        f"{skipped_example['skipped_documents']:,} skipped non-English documents. "
        if skipped_example and skipped_example["skipped_documents"] else ""
    )
    summary_columns = [
        ("Contract", False), ("Layer", False), ("Tool", False),
        ("Leaked B", True), ("FP B", True), ("Processed", True),
        ("Skipped", True), ("Skipped gold B", True),
    ]
    summary_columns += (
        [("CPU-host p50 ms", True), ("CPU-host p95 ms", True)]
        if latency_publishable else [("Latency", False)]
    )
    lines = [
        "# Competitor comparison",
        "",
        "Same corpus and scorer; tools run with documented configurations. "
        "UTF-8 byte counts use the Gaze scorer. For v3, FP is the scorer's "
        "false-positive count after its audited gold-gap credit. "
        + ("CPU-host p50/p95 is warm per-document wall-clock inference/clean time on the same machine. "
           if latency_publishable else "") +
        "Presidio all runs English, German, Dutch, French, and Portuguese spaCy models "
        "with the documented German recognizers. Presidio English default is a secondary row. "
        + ("Latency includes processed documents only. " if latency_publishable else "") + latency_note +
        "This measures detection; competitor restore and manifest behavior is not scored.",
        "",
        "Leaked and false-positive byte counts are class-agnostic. A skipped document's "
        "scored gold counts in full as leaked. Subtract Skipped gold B from Leaked B to "
        "get leakage on processed documents. " + skipped_example_note +
        "The reviewed label map controls v3's repeated-gold credit and the exact typed-span metrics below.",
        "",
        f"Gaze measured at `{gaze['v3']['gaze_revision']}` "
        f"(release `{report['latest_release_at_measurement']['version']}`). "
        f"Home-normalized setup policy SHA-256: `{report['policy_sha256_home_normalized']}`."
        + (" The measured call scopes differ by tool, so latency is descriptive."
           if latency_publishable else ""),
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
        *table_header(summary_columns),
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
                if latency_publishable:
                    p50 = "n/a" if latency["p50_ms"] is None else f"{latency['p50_ms']:.1f}"
                    p95 = "n/a" if latency["p95_ms"] is None else f"{latency['p95_ms']:.1f}"
                    timing = f"{p50} | {p95} |"
                else:
                    timing = "not measured under a quiet machine |"
                lines.append(
                    f"| {version} | {layer_display_name(layer)} | {name} | {row['leaked_bytes']:,} | {fp:,} | "
                    f"{row.get('processed_documents', row['documents']):,} | "
                    f"{row.get('skipped_documents', 0):,} | {row.get('skipped_gold_bytes', 0):,} | "
                    + timing
                )
                if name != "gaze" and version == "v3":
                    gaze_fp = gaze_row["false_positive_bytes_after_gold_gap"]
                    if gaze_fp is None:
                        gaze_fp = gaze_row["false_positive_bytes"]
                    if row["leaked_bytes"] < gaze_row["leaked_bytes"]:
                        lower_leak.append(
                            f"{layer_display_name(layer)}: {name} leaks {row['leaked_bytes']:,} B versus Gaze "
                            f"{gaze_row['leaked_bytes']:,} B"
                        )
                    elif row["leaked_bytes"] == gaze_row["leaked_bytes"] and fp < gaze_fp:
                        lower_fp_at_equal_leak.append(
                            f"{layer_display_name(layer)}: {name} has {fp:,} FP B versus Gaze {gaze_fp:,} FP B"
                        )
    if report.get("schema_version", 1) < 2:
        lines.extend([
            "", "**Where Gaze leaks more under v3:** "
            + ("; ".join(lower_leak) if lower_leak else "none in these measured rows") + ".",
            "", "**Where a competitor has fewer false positives at equal v3 leakage:** "
            + ("; ".join(lower_fp_at_equal_leak) if lower_fp_at_equal_leak else "none in these measured rows") + ".",
        ])
    if report.get("schema_version", 1) >= 2:
        lines.extend([
            "", "## Heldout safety and entity metrics", "",
            "The validation/test split is fixed by document ID. Thresholds are selected on validation; "
            "the table below reports the disjoint test half. Product coverage scores unsupported gold as missed. "
            "Common intersection scores only classes claimed by every listed configuration. "
            "Entity scores require an exact UTF-8 byte span and a compatible reviewed label mapping.",
            "", f"Common classes: {', '.join(report['common_intersection_labels'])}.",
            "", *table_header([
                ("Contract", False), ("Layer", False), ("View", False), ("Tool", False),
                ("PII docs", True), ("Leaked B", True), ("FP B", True),
                ("Leaking docs", True), ("Doc leak %", True),
                ("Leaking entities", True), ("Entity leak %", True),
                ("Redaction load %", True), ("TP", True), ("FP", True),
                ("FN", True), ("Entity P", True), ("Entity R", True),
                ("F1", True), ("F2", True),
            ]),
        ])
        for version in versions:
            for layer in layer_ids:
                for view in ("product_coverage", "common_intersection"):
                    for name in ORDER:
                        if name == "gaze":
                            row = gaze[version]["layers"][layer]
                        elif name in tools:
                            row = tools[name]["contracts"][version][layer]
                        else:
                            continue
                        metric = row["metrics"][view]["test"]
                        entity = metric["typed_entities"]
                        lines.append(
                            f"| {version} | {layer_display_name(layer)} | {view} | {name} | "
                            f"{metric['pii_documents']:,} | "
                            f"{metric['leaked_bytes']:,} | {metric['false_positive_bytes']:,} | "
                            f"{metric['leaking_documents']:,} | {100 * metric['document_leak_rate']:.1f} | "
                            f"{metric['leaking_entities']:,} | {100 * metric['leaked_entity_rate']:.1f} | "
                            f"{100 * metric['redaction_load']:.1f} | {entity['tp']:,} | {entity['fp']:,} | "
                            f"{entity['fn']:,} | {entity['precision']:.3f} | {entity['recall']:.3f} | "
                            f"{entity['f1']:.3f} | {entity['f2']:.3f} |"
                        )
        loss_counts, losses = enumerate_gaze_losses(report, versions, layer_ids)
        summary = "; ".join(f"{name}: {count}" for name, count in loss_counts.items())
        gaze_c = gaze["v3"]["layers"]["C"]["metrics"]["common_intersection"]["test"]
        c_candidates = (
            (name, tool["contracts"]["v3"]["C"]["metrics"]["common_intersection"]["test"])
            for name, tool in tools.items()
        )
        best_c_name, best_c = min(c_candidates, key=lambda pair: pair[1]["leaked_bytes"])
        highlighted = (
            f"On the v3 {layer_display_name('C')} common-intersection test half, "
            f"Gaze leaks {gaze_c['leaked_bytes']:,} B "
            f"across {gaze_c['leaking_documents']:,}/{gaze_c['pii_documents']:,} PII documents; "
            f"{best_c_name} leaks {best_c['leaked_bytes']:,} B across "
            f"{best_c['leaking_documents']:,}/{best_c['pii_documents']:,}."
        )
        if best_c["leaked_bytes"] < gaze_c["leaked_bytes"]:
            highlighted += " This is a measured Gaze loss."
        lines.extend([
            "", "## Where Gaze trails", "",
            highlighted,
            "",
            "Competitors have a lower leak, false-positive, or redaction-load value, or a "
            "better typed-entity value, in the following measured cells. Lower redaction load "
            "alone can reflect missed PII. Each pair reads competitor vs Gaze. Full aggregate "
            "rows span both halves and use v3's audited gold-gap FP credit; detailed rows use "
            "raw FP and also show validation and test separately. Counts below are metric cells; "
            "overlapping views and splits must not be summed as independent cases.",
            "", f"Loss counts by metric: {summary or 'none'}.",
            "", "<details>", f"<summary>All {len(losses):,} losing rows</summary>", "",
            *(f"- {loss}" for loss in losses), "", "</details>",
        ])
        lines.extend(["", "## Gaze ablations", "",
                      "Rules only, rules plus NER, and full setup use the same test documents and scorer.", "",
                      *table_header([
                          ("Contract", False), ("Layer", False), ("Gaze configuration", False),
                          ("Leaked B", True), ("FP B", True), ("PII docs", True),
                          ("Leaking docs", True), ("Entity F1", True), ("Entity F2", True),
                      ])])
        for version in versions:
            for layer in layer_ids:
                for name in ("rules-only", "rules-ner", "full"):
                    row = (gaze[version]["layers"][layer] if name == "full"
                           else report["gaze_ablations"][name][version][layer])
                    metric = row["metrics"]["product_coverage"]["test"]
                    typed = metric["typed_entities"]
                    lines.append(f"| {version} | {layer_display_name(layer)} | {name} | "
                                 f"{metric['leaked_bytes']:,} | "
                                 f"{metric['false_positive_bytes']:,} | {metric['pii_documents']:,} | "
                                 f"{metric['leaking_documents']:,} | {typed['f1']:.3f} | {typed['f2']:.3f} |")
        lines.extend(["", "Threshold choice uses validation only: " + "; ".join(
            f"{group} → {selected}" for group, selected in report.get("selected_threshold_rows", {}).items()) + "."])
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
