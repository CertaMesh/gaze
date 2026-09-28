#!/usr/bin/env python3
"""Assemble and render Gaze's results on competitors' own benchmarks.

`assemble` merges theirbench.py reports and each benchmark's own-scorer
results into docs/reference/benchmarks/their-benchmarks.json (aggregates
only). `render` writes the `their-benchmarks` generated block of
docs/reference/benchmarks/README.md; `--check` fails on drift.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Mapping

REPO = Path(__file__).resolve().parents[4]
DATA = REPO / "docs/reference/benchmarks/their-benchmarks.json"
DOC = REPO / "docs/reference/benchmarks/README.md"
BLOCK = "their-benchmarks"
TITLES = {
    "presidio-research": "Presidio Research synthetic set (synth_dataset_v2, 1,500 documents)",
    "piibench-commercial": "PIIBench-commercial (four permissively licensed PIIBench sources, test_5k)",
}
OWN_METRIC = {"presidio-research": ("f2", "F2, binary PII vs O, presidio-evaluator"),
              "piibench-commercial": ("f1", "span F1, exact span + type, PIIBench seqeval")}
GAZE_ROWS = ("gaze-full", "gaze-rules-ner", "gaze-rules-only")
# STEER 2: chart bars are configurations declared before measuring, never
# the lowest-leak row. Replaced by the comparison's declared chart list once
# Track B publishes it; copied into the JSON at assemble time.
DECLARED_CHART_ROWS = ("gaze-full", "presidio-strong", "gliner", "opf", "datafog-core",
                       "datafog-gliner", "scrubadub-spacy")
QUIET = "not measured under a quiet machine"
NOT_RUN = {
    "PIIBench full ten-source mix": "five sources carry non-commercial or custom-academic licences and "
                                    "WikiANN's licence is unknown; not downloaded or run",
    "ai4privacy/pii-masking-300k (OPF's published set)": "custom licence; commercial use requires a "
                                                         "licence from ai4privacy; not downloaded or run",
}


def family(tool: str) -> str:
    for prefix in ("gaze", "presidio", "gliner", "datafog-core", "scrubadub", "opf"):
        if tool.startswith(prefix):
            return prefix
    return "datafog-python"


def tagged(values: list[str]) -> list[tuple[str, Path]]:
    """Parse `<benchmark>=<path>` arguments."""
    pairs = []
    for value in values:
        name, _, path = value.partition("=")
        if name not in TITLES or not path:
            raise ValueError(f"expected <benchmark>=<path> with benchmark in {sorted(TITLES)}: {value}")
        pairs.append((name, Path(path)))
    return pairs


def assemble(reports: list[Path], own: list[str], reproductions: list[str]) -> dict[str, Any]:
    benchmarks: dict[str, Any] = {}
    for path in reports:
        report = json.loads(path.read_text(encoding="utf-8"))
        if report["harness_dirty"]:
            raise ValueError(f"{path}: measured on a dirty harness")
        name = report["benchmark"]
        rows = {tool: result["test"] for tool, result in report["rows"].items()}
        benchmarks[name] = {key: report[key] for key in (
            "identity", "harness_revision", "gaze_crates_tree", "label_maps_sha256", "mapping_sha256",
            "hardware", "common_intersection_labels", "splits", "provenance")}
        benchmarks[name]["rows"] = rows
        benchmarks[name]["own_metric"] = {}
        benchmarks[name]["reproduction"] = {}
    for name, path in tagged(own):
        result = json.loads(path.read_text(encoding="utf-8"))
        if result.get("smoke_limit"):
            raise ValueError(f"{path}: smoke results are never published")
        scored = result.get("scored") or result["overall"]
        benchmarks[name]["own_metric"][result["system"]] = scored
    for name, path in tagged(reproductions):
        result = json.loads(path.read_text(encoding="utf-8"))
        if result.get("smoke_limit"):
            raise ValueError(f"{path}: smoke results are never published")
        benchmarks[name]["reproduction"] = (
            {"published": result["published"], "reproduced": result["reproduced"], "versions": result["versions"]}
            if name == "presidio-research"
            else {"published_full_mix": result["published_full_mix"], "reproduced_commercial": result["overall"],
                  "versions": result["versions"]})
    for name, entry in benchmarks.items():
        missing = sorted(set(entry["rows"]) - set(entry["own_metric"]))
        if missing:
            raise ValueError(f"{name}: no own-scorer result for {missing}")
        if not entry["reproduction"]:
            raise ValueError(f"{name}: the vendor number must be reproduced before anything is published")
    for entry in benchmarks.values():
        entry["chart_rows"] = [tool for tool in DECLARED_CHART_ROWS if tool in entry["rows"]]
    return {"schema_version": 1, "report_only": "never used to design or tune Gaze rules",
            "not_run": NOT_RUN, "benchmarks": benchmarks}


def pct(value: float) -> str:
    return f"{value * 100:.1f}%"


def gold_bytes(cell: Mapping[str, Any]) -> int:
    return cell["true_positive_bytes"] + cell["leaked_bytes"]


def chart_rows(entry: Mapping[str, Any]) -> list[str]:
    """The declared rows, in declared order; measured leak never chooses."""
    missing = sorted(set(entry["chart_rows"]) - set(entry["rows"]))
    if missing:
        raise ValueError(f"declared chart rows were not measured: {missing}")
    return list(entry["chart_rows"])


def latency_cell(entry: Mapping[str, Any], tool: str) -> str:
    load = entry["provenance"].get(tool, {}).get("load")
    if load is None or load["contended"]:
        return QUIET
    p50 = entry["rows"][tool]["latency"]["p50_ms"]
    return "n/a" if p50 is None else f"{p50:.1f}"


def render(data: Mapping[str, Any]) -> str:
    lines = [
        "Report-only: these sets are never used to design or tune Gaze rules. Every gold "
        "label counts (no scored-label contract). Leaked and false-positive bytes use the "
        "same scorer code as the main comparison; each benchmark's own metric comes from "
        "its own evaluator, fed the same spans. Lower leaked bytes is better. Chart bars are "
        "configurations declared before measuring; the table lists every measured row. "
        f"Latency reads \"{QUIET}\" when load1 exceeded 2.0 during that row's run.",
        "",
    ]
    for name, entry in data["benchmarks"].items():
        rows, metric, metric_label = entry["rows"], *OWN_METRIC[name]
        gold = gold_bytes(rows["gaze-full"]["product_coverage"])
        lines += [f"#### {TITLES[name]}", ""]
        repro = entry["reproduction"]
        if name == "presidio-research":
            for config, published in repro["published"].items():
                got = repro["reproduced"][config]
                lines.append(f"- Vendor number reproduced first, Presidio {config}: published F2 "
                             f"{published['f2']}, reproduced {got['f2']} ({published['source']}).")
        else:
            published = repro["published_full_mix"]
            lines.append(f"- Published Presidio span F1 {published['f1']} is on the full ten-source mix "
                         f"({published['records']:,} records) and is quoted, not reproduced. PIIBench's own "
                         f"harness gives Presidio {repro['reproduced_commercial']['f1']} on this commercial subset.")
        lines += ["", "```mermaid", "xychart-beta horizontal",
                  f'    title "Leaked PII bytes, {name} - lower is better"']
        chart = chart_rows(entry)
        labels = ", ".join(f'"{tool} ({pct(rows[tool]["product_coverage"]["leaked_bytes"] / gold)})"' for tool in chart)
        top = max(rows[tool]["product_coverage"]["leaked_bytes"] for tool in chart)
        lines += [f"    x-axis [{labels}]", f'    y-axis "Leaked PII bytes" 0 --> {top + max(1, top // 10)}',
                  f"    bar [{', '.join(str(rows[t]['product_coverage']['leaked_bytes']) for t in chart)}]",
                  "```", "",
                  f"Gold PII bytes: {gold:,}. Common-intersection labels: "
                  f"{', '.join(entry['common_intersection_labels']) or 'none'}.", "",
                  f"| Tool | Leaked B | FP B | Doc leak rate | Typed F1 | Typed F2 | Leaked B, common | "
                  f"Own metric ({metric_label}) | p50 ms |",
                  "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"]
        order = [*GAZE_ROWS, *sorted(tool for tool in rows if tool not in GAZE_ROWS)]
        for tool in order:
            if tool not in rows:
                continue
            product, common = rows[tool]["product_coverage"], rows[tool]["common_intersection"]
            typed = product["typed_entities"]
            lines.append(
                f"| {tool} | {product['leaked_bytes']:,} | {product['false_positive_bytes']:,} | "
                f"{pct(product['document_leak_rate'])} | {typed['f1']:.3f} | {typed['f2']:.3f} | "
                f"{common['leaked_bytes']:,} | {entry['own_metric'][tool][metric]:.3f} | "
                f"{latency_cell(entry, tool)} |")
        lines += ["", f"Hardware: {entry['hardware']}. Harness `{entry['harness_revision'][:8]}`.", ""]
    lines += ["Not run:", ""] + [f"- {name}: {reason}." for name, reason in data["not_run"].items()]
    return "\n".join(lines)


def apply(document: str, body: str) -> str:
    begin, end = f"<!-- BEGIN GENERATED: {BLOCK} -->", f"<!-- END GENERATED: {BLOCK} -->"
    start, stop = document.find(begin), document.find(end)
    if start < 0 or stop < start:
        raise ValueError(f"README is missing the {BLOCK} block")
    return document[: start + len(begin)] + "\n\n" + body + "\n\n" + document[stop:]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    build = sub.add_parser("assemble")
    build.add_argument("--report", type=Path, action="append", required=True)
    build.add_argument("--own", action="append", required=True, help="<benchmark>=<own-scorer result>")
    build.add_argument("--reproduction", action="append", required=True, help="<benchmark>=<vendor reproduction>")
    show = sub.add_parser("render")
    show.add_argument("--check", action="store_true")
    show.add_argument("--data", type=Path, default=DATA)
    show.add_argument("--doc", type=Path, default=DOC)
    args = parser.parse_args(argv)
    if args.command == "assemble":
        DATA.write_text(json.dumps(assemble(args.report, args.own, args.reproduction), indent=2,
                                   sort_keys=True) + "\n", encoding="utf-8")
        return 0
    data = json.loads(args.data.read_text(encoding="utf-8"))
    current = args.doc.read_text(encoding="utf-8")
    expected = apply(current, render(data))
    if args.check:
        if expected != current:
            print(f"{args.doc}: {BLOCK} block is stale; run render_theirbench.py render", file=sys.stderr)
            return 1
        return 0
    args.doc.write_text(expected, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
