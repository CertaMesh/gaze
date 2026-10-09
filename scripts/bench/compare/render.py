#!/usr/bin/env python3
"""Render the public competitor page from a measured report."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
BENCH = REPO / "scripts/bench"
import compare  # noqa: E402
from layer_display import layer_display_name  # noqa: E402
from markdown_table import table_header  # noqa: E402
from tagged_gaze import check_public  # noqa: E402

sys.path.insert(0, str(Path(__file__).with_name("tuned_presidio")))
from space import CUSTOM_RECOGNIZERS as TUNED_CUSTOM  # noqa: E402
import verdict as tuned_verdict  # noqa: E402

#: Gaze is not listed: the comparison run measured an unreleased build, and public
#: pages show tagged releases only (tagged_gaze.py).
ORDER = tuple(compare.TOOLS)

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
        # A comparison measured on an older generator stays valid while the
        # current generator still rebuilds that corpus byte for byte and its
        # contract is committed; the page then names the older corpus.
        "agentic layers": (
            corpus["agentic"]["corpus_sha256"], agentic_identity(report)[0],
        ),
        "agentic scored labels": (
            report["contracts"]["agentic"], agentic_identity(report)[1],
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


def agentic_identity(report: dict[str, object]) -> tuple[str, str]:
    """(corpus, contract) SHA-256 of the generator version the report measured."""
    version = report["corpus"]["agentic"]["generator_version"]
    return compare.agentic.corpus_identity(REPO, version)


def agentic_corpus_note(report: dict[str, object]) -> str:
    """A plain statement when the agentic numbers predate the current generator.

    Display only: `validate_current` requires the agentic identity and fails
    closed without it.
    """
    agentic = report["corpus"].get("agentic")
    if agentic is None or agentic["generator_version"] == compare.agentic.GENERATOR_VERSION:
        return ""
    version = agentic["generator_version"]
    return (
        f" The agentic-layer rows were measured on generator v{version} "
        f"(test corpus `{report['corpus']['agentic']['corpus_sha256'][:12]}…`, "
        f"{report['corpus']['agentic']['documents']:,} documents). The current generator is "
        f"v{compare.agentic.GENERATOR_VERSION}; its added documents are not measured here."
    )


def render(report: dict[str, object], source: str, tuned: dict[str, object] | None = None,
           history_path: Path | None = None) -> str:
    gaze = report["gaze"]
    tools = report["tools"]
    versions = ("v4", "v3", "v2", "v1")
    if set(gaze) != set(versions):
        raise ValueError("public comparison needs a Gaze scorecard for v4, v3, v2, and v1")
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
    latency_note = (
        "" if latency_publishable
        else "Latency was not measured under a quiet machine; timing comparisons are withheld. "
    )
    skipped_example = tools["presidio-en"]["contracts"]["v4"].get("A")
    skipped_example_note = (
        f"For example, Presidio English-only v4 {layer_display_name('A')} leaks "
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
        "UTF-8 byte counts use the Gaze scorer. For v4 and v3, FP is the scorer's "
        "false-positive count after its audited gold-gap credit. "
        + ("CPU-host p50/p95 is warm per-document wall-clock inference/clean time on the same machine. "
           if latency_publishable else "") +
        "Presidio all runs English, German, Dutch, French, and Portuguese spaCy models "
        "with the documented German recognizers. Presidio English default is a secondary row. "
        + ("Latency includes processed documents only. " if latency_publishable else "") + latency_note +
        "This measures detection; competitor restore and manifest behavior is not scored."
        + agentic_corpus_note(report),
        "",
        "Leaked and false-positive byte counts are class-agnostic. A skipped document's "
        "scored gold counts in full as leaked. Subtract Skipped gold B from Leaked B to "
        "get leakage on processed documents. " + skipped_example_note +
        "The reviewed label map controls repeated-gold credit and the exact typed-span metrics below.",
        "",
        "Gaze is not listed: the comparison run measured an unreleased build, and this page "
        "shows tagged Gaze releases only. Released Gaze numbers are in the "
        "[release history](README.md#release-history) and the benchmark panels. "
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
    for version in versions:
        for layer in layer_ids:
            for name in ORDER:
                if name not in tools:
                    continue
                row = tools[name]["contracts"][version][layer]
                fp = row["false_positive_bytes"]
                if version in {"v4", "v3"} and row["false_positive_bytes_after_gold_gap"] is not None:
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
                        if name not in tools:
                            continue
                        row = tools[name]["contracts"][version][layer]
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
        lines.extend(["", "Threshold choice uses validation only: " + "; ".join(
            f"{group} → {selected}" for group, selected in report.get("selected_threshold_rows", {}).items()) + "."])
    if tuned is not None:
        lines.extend(render_tuned(tuned, report, history_path or REPO / "docs/reference/benchmarks/release-history.json"))
    skipped = report.get("skipped", {})
    if skipped:
        lines.extend(["", "**Skipped:** " + "; ".join(f"{name}: {reason}" for name, reason in skipped.items()) + "."])
    lines.append("")
    return check_public("\n".join(lines), "competitors.md")


TUNED_NAME = "presidio-tuned.json"
TUNED_CODE = ("space.py", "pool.py", "search.py", "corpus.py", "produce.py", "tune.py")
TUNED_LABELS = {
    "presidio-tuned-presidio-research": "Presidio (tuned by its authors)",
    "presidio-tuned-own-leak-first": "Presidio (tuned here, leak-first)",
    "presidio-tuned-own-f2": "Presidio (tuned here, F2)",
}
TUNED_BASELINES = ("presidio-all", "presidio-strong", "presidio-strong-high-recall")


def validate_tuned(tuned: dict[str, object], report_path: Path) -> None:
    """Reject a tuned report whose inputs or code changed, or whose replay is unproven."""
    here = Path(__file__).parent
    expected = {
        "comparison report": (tuned["comparison_sha256"], digest_file(report_path)),
        "comparison adapter": (tuned["compare_sha256"], digest_file(Path(compare.__file__))),
        "comparison metrics": (tuned["comparison_metrics_sha256"], digest_file(here / "comparison_metrics.py")),
        "scorer": (tuned["scorer_sha256"], digest_file(BENCH / "gaze_bench_score.py")),
        "selection": (tuned["selection_sha256"], digest_file(report_path.parent / "presidio-tuned-selection.json")),
        **{f"tuned_presidio/{name}": (tuned["code_sha256"][name], digest_file(here / "tuned_presidio" / name))
           for name in TUNED_CODE},
    }
    for name, (recorded, current) in expected.items():
        if recorded != current:
            raise ValueError(f"{name} changed; rerun tuned_presidio/tune.py measure")
    if tuned.get("harness_dirty") is not False or not tuned["anchor"]["equal"]:
        raise ValueError("tuned Presidio needs a clean harness and the reproduced presidio-all anchor")
    for name, provenance in tuned["provenance"].items():
        if "live_verification" in provenance and not tuned_verdict.byte_identical(provenance["live_verification"]):
            raise ValueError(f"{name}: its live run covered different bytes than its replay")


def _gaze_release_c(history_path: Path) -> tuple[str, dict[str, dict[str, float]]]:
    """Latest tagged Gaze release on layer C: leaked and false-positive bytes per contract, v4 F2."""
    import render_benchmark_doc as history_doc

    history = history_doc.load_history(history_path)
    entry = [e for e in history["releases"] if not e.get("provisional")][-1]
    arm = history_doc.shipped_default_arm(entry)
    cells = {}
    for version in (4, 3, 2, 1):
        view = history_doc.contract_view(entry, version)
        if view is not None:
            cells[f"v{version}"] = {"leaked": view["arms"][arm]["surviving_pii_utf8_bytes"],
                                    "fp": view["arms"][arm]["false_positive_utf8_bytes"]}
    char = json.loads((history_path.parent / "release-char-level.json").read_text(encoding="utf-8"))
    cells["v4"]["f2"] = char["releases"][entry["version"]]["char_level"]["f2"]
    return entry["version"], cells


def _fp(cell: dict[str, object]) -> int:
    after = cell.get("false_positive_bytes_after_gold_gap")
    return cell["false_positive_bytes"] if after is None else after


def _pct(item: dict[str, object] | None) -> str:
    if item is None:
        return "n/a"
    if item["of"] == 0:
        return "no gold"
    return f"{item['count']:,} of {item['of']:,} ({item['percent']:.1f} %)"


def _overlap_callout(overlap: dict[str, object]) -> list[str]:
    """The dependence between the halves, stated next to the tuned test-half rows."""
    coverage = overlap["layer_a_pattern_coverage"]
    lines = [
        "**The split holds out document ids, not values.** The tuned-here rows were fitted on the validation "
        "half, and the test half repeats much of it. No test document is identical to a validation document, "
        "but most share a template, a generator group or exact gold values with one:", "",
        *table_header([("Layer", False), ("Test docs", True), ("Share a template", True),
                       ("Share a generator group", True), ("Reuse a gold value", True),
                       ("Gold spans repeating a value", True)]),
    ]
    for layer in ("C", "A", "D", "R"):
        cell = overlap["layers"][layer]
        lines.append(
            f"| {layer_display_name(layer)} | {cell['test_documents']:,} | "
            f"{_pct(cell.get('share_a_validation_template'))} | {_pct(cell.get('share_a_validation_group'))} | "
            f"{_pct(cell['reuse_a_validation_gold_value'])} | "
            f"{_pct(cell['gold_spans_repeating_a_validation_value'])} |")
    lines.extend([
        "",
        f"The tuned F2 choice's custom pattern recognizers alone, with every NER model off, cover "
        f"{_pct(coverage['covered_by_custom_patterns_only'])} of {layer_display_name('A')} test gold bytes; "
        f"the full choice leaks {coverage['leaked_by_full_choice']:,} bytes there. These counts measure how "
        "far the test half depends on the validation half, not how much of any result is memorization. They "
        f"weigh most on the generated layers; {layer_display_name('C')} has no templates of that kind, and its "
        "comparison with Gaze below stands as measured. Templates and groups exist only in the generated "
        "layers (n/a above). Computed by "
        "[`overlap.py`](../../../scripts/bench/compare/tuned_presidio/overlap.py) into "
        "[`presidio-tuned-overlap.json`](presidio-tuned-overlap.json)."])
    return lines


def render_tuned(tuned: dict[str, object], report: dict[str, object], history_path: Path) -> list[str]:
    selection = json.loads((history_path.parent / "presidio-tuned-selection.json").read_text(encoding="utf-8"))
    overlap = json.loads((history_path.parent / "presidio-tuned-overlap.json").read_text(encoding="utf-8"))
    rows = tuned["rows"]
    authors = tuned["provenance"]["presidio-tuned-presidio-research"]
    coverage = authors["coverage"]
    budget = tuned["budget"]
    validation_docs = sum(budget["validation_documents"].values())
    lines = [
        "", "## Tuned Presidio", "",
        "Three more Presidio rows on the same documents, contracts and scorer as above. "
        "Every number in this section names its split; test-half numbers are the comparison.",
        "",
        f"- **Tuned by its authors:** Presidio Research's own tuned setup for its corpus ({authors['setup']}; "
        f"`notebooks/5_Evaluate_Custom_Presidio_Analyzer.ipynb` at presidio-research `{authors['commit'][:8]}`), "
        "applied unchanged. It supports English only, so every document "
        "is analyzed as English. Its entities without a label in this corpus: "
        f"{', '.join(coverage['entities_without_a_corpus_label'])}. Scored labels here that none of its "
        f"entities maps to: {', '.join(coverage['corpus_labels_without_an_entity'])}. Labels affect only the "
        "typed metrics; leaked and false-positive bytes ignore them.",
        "- **Tuned for this corpus:** a search over a space declared in code before any run "
        "([`space.py`](../../../scripts/bench/compare/tuned_presidio/space.py)): the NLP-engine NER "
        "(spaCy large, `dslim/bert-base-NER` or none), extra NER recognizers Presidio ships (the multilingual "
        "Davlan model Gaze itself installs, the OpenMed PII model Presidio Research chose, GLiNER), every "
        f"predefined recognizer, {len(TUNED_CUSTOM)} custom pattern and deny-list recognizers for this "
        "corpus's classes, per recognizer and entity thresholds, the context enhancer and an allow list learned "
        "from validation false positives. The search scored only the validation half "
        f"({validation_docs:,} documents) and evaluated "
        f"{budget['candidates_evaluated']:,} candidate configurations by coordinate descent. Two objectives: "
        "the comparison's own rule (fewest validation v3 leaked bytes, then fewest false-positive bytes) and "
        "the panels' headline (highest validation v3 character F2). The custom recognizers were written "
        "after reading validation-half gold examples; the search never received test-half text, gold or "
        "output. The committed choice was made by an earlier loader that built the whole corpus in memory "
        "and dropped the test half before the search; rerunning selection through per-half files, with the "
        "test-half file never opened, reproduces every choice and validation score. A guard test runs "
        "the current real loader with every test-half file unreadable. The NER models' training data "
        "is not fully published, so overlap with this "
        "synthetic corpus's style cannot be ruled out for them (nor for the NER model in Gaze's own setup). "
        "Both halves are synthetic and share their generators, and the split holds out document ids only, "
        "not templates or values: "
        f"{_pct(overlap['layers']['A']['reuse_a_validation_gold_value'])} of the "
        f"{layer_display_name('A')} test documents reuse a validation gold value (every layer is counted "
        "beside the test-half table below). Tuning on validation therefore "
        "learns those templates and values, and the test half measures fit to this corpus, not robustness "
        "to unseen phrasing or values. Gaze's rules were developed against the same corpus.",
        f"- **Budget:** Gaze's rules received {budget['gaze_rulepack_commits']} rulepack commits "
        f"({budget['first']} to {budget['last']}) at the original measurement lineage "
        f"`{budget.get('source_revision', tuned['harness_revision'])[:8]}`, made with the whole corpus "
        "visible, test half included. This budget is pinned to that lineage, not the current branch's "
        "rulepack history. "
        "The tuned Presidio search is at least as generous in iterations: "
        f"{budget['candidates_evaluated']:,} measured candidate configurations against "
        f"{budget['gaze_rulepack_commits']} rulepack commits, on top of a hand-written recognizer for the "
        "classes Gaze commits to. It saw the "
        "validation half only during scoring, so its test-half documents are held out, by id, while Gaze's are not.",
        "",
        "Validation choice (v3, C/A/D/R summed):", "",
        *table_header([("Objective", False), ("Start", False), ("Leaked B", True), ("FP B", True),
                       ("Char F2", True), ("Chosen", False)]),
    ]
    for objective, choice in selection["choices"].items():
        for start, final in choice["finals"].items():
            value = final["validation"]
            lines.append(f"| {objective} | {start} | {value['leaked_bytes']:,} | {value['false_positive_bytes']:,} | "
                         f"{value['char_f2']:.3f} | {'yes' if start == choice['start'] else ''} |")
    lines.extend(["", "Chosen configurations:", ""])
    for objective, choice in selection["choices"].items():
        config = choice["finals"][choice["start"]]["config"]
        units = sorted(unit for unit, scope in config["scope"].items() if scope != "off")
        extras = ", ".join(f"{key} {scope}" for key, scope in config["extra"].items())
        raised = sum(1 for value in config["thresholds"].values() if value > 0)
        lines.append(
            f"- **{objective}:** NLP-engine NER {config['artifact_ner']}; extra NER {extras}; context "
            f"{config['context']}; {len(units)} pattern recognizers on; {raised} raised thresholds; allow list "
            f"of {len(config['allow_list']):,} texts. Full configuration in "
            "[`presidio-tuned-selection.json`](presidio-tuned-selection.json).")
    lines.extend([
        "", *_overlap_callout(overlap), "",
        "Test half (product coverage):", "",
        *table_header([("Contract", False), ("Layer", False), ("Configuration", False), ("Leaked B", True),
                       ("FP B", True), ("Char F2", True), ("Entity F2", True)]),
    ])
    names = [*TUNED_LABELS, *TUNED_BASELINES]
    for version in ("v4", "v3", "v2", "v1"):
        for layer in report["corpus"]["layers"]:
            for name in names:
                source = rows[name] if name in rows else report["tools"][name]
                test = source["contracts"][version][layer]["metrics"]["product_coverage"]["test"]
                lines.append(
                    f"| {version} | {layer_display_name(layer)} | {TUNED_LABELS.get(name, name)} | "
                    f"{test['leaked_bytes']:,} | {test['false_positive_bytes']:,} | "
                    f"{test['char_level']['f2']:.3f} | {test['typed_entities']['f2']:.3f} |")
    version, gaze = _gaze_release_c(history_path)
    lines.extend([
        "", f"Against the latest Gaze release ({version}) on all of {layer_display_name('C')}, the only layer "
        "that release was measured on in this corpus's form. The tuned-here rows include the validation half "
        "they were selected on, which can only flatter them. False positives are after v4's gold-gap credit.", "",
        *table_header([("Contract", False), ("Configuration", False), ("Leaked B", True), ("FP B", True),
                       ("Char F2", True)]),
    ])
    wins = []
    for contract, cell in gaze.items():
        lines.append(f"| {contract} | Gaze {version} | {cell['leaked']:,} | {cell['fp']:,} | "
                     + (f"{cell['f2']:.3f} |" if "f2" in cell else "n/a |"))
        for name, label in TUNED_LABELS.items():
            full = rows[name]["contracts"][contract]["C"]
            f2 = full["metrics"]["product_coverage"]["full"]["char_level"]["f2"]
            lines.append(f"| {contract} | {label} | {full['leaked_bytes']:,} | {_fp(full):,} | "
                         + (f"{f2:.3f} |" if contract == "v4" else "n/a |"))
            if full["leaked_bytes"] < cell["leaked"]:
                wins.append(f"{label} leaks fewer {contract} bytes")
            if _fp(full) < cell["fp"]:
                wins.append(f"{label} has fewer {contract} false-positive bytes")
            if contract == "v4" and f2 > cell["f2"]:
                wins.append(f"{label} has the higher v4 character F2")
    lines.extend(["", "Where tuned Presidio beats Gaze " + version + " here: "
                  + ("; ".join(wins) if wins else "nowhere") + ".", "",
                  "Live check: each chosen configuration also ran live on a fixed sample (about one document "
                  "in eight, every layer, both halves); "
                  + "; ".join(
                      f"{TUNED_LABELS[name]}: {live['documents']:,} documents, {live['differing_documents']} differed "
                      f"on the first run, {live['persistent_differing_documents']} on every rerun"
                      + (f" ({tuned_verdict.label_only_count(live)} with identical bytes and a different entity "
                         "label, from Presidio's tie-break between equal-score results on one span)"
                         if tuned_verdict.label_only_count(live) else "")
                      for name, provenance in tuned["provenance"].items()
                      if (live := provenance.get("live_verification")))
                  + ".", "",
                  f"The panels' Presidio bar is **{TUNED_LABELS[tuned['chart']['row']]}**, the highest "
                  "test-half v3 layer C character F2 of the three: "
                  + ", ".join(f"{TUNED_LABELS[n]} {v:.3f}" for n, v in tuned["chart"]["test_f2"].items())
                  + ". The default rows above stay unchanged.",
                  "", f"Aggregate source: [`{TUNED_NAME}`]({TUNED_NAME}); reproduce with "
                  "[`tuned_presidio/README.md`](../../../scripts/bench/compare/tuned_presidio/README.md)."])
    return lines


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--page", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = json.loads(args.report.read_text(encoding="utf-8"))
    validate_current(report)
    tuned_path = args.report.with_name(TUNED_NAME)
    tuned = None
    if tuned_path.exists():
        tuned = json.loads(tuned_path.read_text(encoding="utf-8"))
        validate_tuned(tuned, args.report)
    page = render(report, args.report.name, tuned, args.report.with_name("release-history.json"))
    if args.check:
        if args.page.read_text(encoding="utf-8") != page:
            raise ValueError("competitor page is stale; rerender it")
        return
    args.page.write_text(page, encoding="utf-8")


if __name__ == "__main__":
    main()
