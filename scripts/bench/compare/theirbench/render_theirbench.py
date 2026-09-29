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
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Mapping

REPO = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPO / "scripts/bench"))
from markdown_table import table_header  # noqa: E402
from tagged_gaze import RELEASE_PINS, TAG, check_public, tag_commit  # noqa: E402
VENDOR_TUNED = Path(__file__).with_name("vendor-tuned.json")
DATA = REPO / "docs/reference/benchmarks/their-benchmarks.json"
DOC = REPO / "docs/reference/benchmarks/README.md"
BLOCK = "their-benchmarks"
TITLES = {
    "presidio-research": "Presidio Research synthetic set (synth_dataset_v2, 1,500 documents)",
    "piibench-commercial": "PIIBench-commercial (four permissively licensed PIIBench sources, test_5k)",
}
OWN_METRIC = {"presidio-research": ("f2", "F2, binary PII vs O, presidio-evaluator"),
              "piibench-commercial": ("f1", "span F1, exact span + type, PIIBench seqeval")}
#: Untagged main-tree rows. They stay in their-benchmarks.json as evidence but are
#: never published; a tagged run is stored as a `gaze-vX.Y.Z` row (tagged_gaze.py).
GAZE_ROWS = ("gaze-full", "gaze-rules-ner", "gaze-rules-only")
RELEASE_HISTORY = REPO / "docs/reference/benchmarks/release-history.json"
HELD = "held (typed-metric review)"
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


def resolve_tag(tag: str) -> str:
    return subprocess.check_output(["git", "rev-parse", f"{tag}^{{commit}}"], cwd=REPO, text=True).strip()


def checked_harness_tags(entry: Mapping[str, Any], tags: Mapping[str, str],
                         resolve: Callable[[str], str] = resolve_tag) -> dict[str, str]:
    """A tag is named on the page only if it points at the commit the report recorded."""
    expected = {"measured": entry["harness_revision"], "rescored": entry["rescored_with"]["harness_revision"]}
    for kind, tag in tags.items():
        if kind not in expected or resolve(tag) != expected[kind]:
            raise ValueError(f"{tag} does not point at the {kind} harness commit")
    return dict(tags)


def assemble(reports: list[Path], own: list[str], reproductions: list[str],
             historical: list[Path] = (), harness_tags: Mapping[str, str] | None = None) -> dict[str, Any]:
    benchmarks: dict[str, Any] = {}
    for path in reports:
        report = json.loads(path.read_text(encoding="utf-8"))
        if report.get("preflight"):
            raise ValueError(f"{path}: preflight results are never published")
        if "rescored_with" not in report or report["rescored_with"]["harness_dirty"]:
            raise ValueError(f"{path}: publish only after a clean --rescore on the fixed typed metrics")
        if report["harness_dirty"]:
            raise ValueError(f"{path}: measured on a dirty harness")
        name = report["benchmark"]
        rows = {tool: result["test"] for tool, result in report["rows"].items()}
        benchmarks[name] = {key: report[key] for key in (
            "identity", "harness_revision", "gaze_crates_tree", "label_maps_sha256", "mapping_sha256",
            "hardware", "common_intersection_labels", "splits", "provenance", "harness_revision",
            "comparison_revision", "comparison_sha256", "typed_hold", "rescored_with")}
        benchmarks[name]["gaze_crates_tree"] = report["gaze_crates_tree"]
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
    for path in historical:
        result = json.loads(path.read_text(encoding="utf-8"))
        if result.get("smoke_limit") or not result.get("reproduction_run"):
            raise ValueError(f"{path}: not a full historical reproduction")
        for config, scored in result["reproduced"].items():
            benchmarks["presidio-research"]["reproduction"].setdefault("historical", {})[config] = {
                **scored, "evaluator_commit": result["evaluator_commit"]}
    for name, entry in benchmarks.items():
        if harness_tags:
            entry["harness_tags"] = checked_harness_tags(entry, harness_tags)
        missing = sorted(set(entry["rows"]) - set(entry["own_metric"]))
        if missing:
            raise ValueError(f"{name}: no own-scorer result for {missing}")
        if not entry["reproduction"]:
            raise ValueError(f"{name}: the vendor number must be reproduced before anything is published")
    for path in reports:
        report = json.loads(path.read_text(encoding="utf-8"))
        # STEER 2: bars are the comparison's declared configurations, never the lowest-leak row.
        benchmarks[report["benchmark"]]["chart_rows"] = ["gaze-full", *report["chart_configs"].values()]
    return {"schema_version": 1, "report_only": "never used to design or tune Gaze rules",
            "not_run": NOT_RUN, "benchmarks": benchmarks}


def is_tagged_gaze_row(tool: str) -> bool:
    return tool.startswith("gaze-") and TAG.fullmatch(tool[len("gaze-"):]) is not None


def public_rows(rows: Mapping[str, Any]) -> list[str]:
    """Tagged Gaze rows first, then every competitor row; untagged Gaze rows never."""
    tagged = sorted(tool for tool in rows if is_tagged_gaze_row(tool))
    other = sorted(tool for tool in rows if family(tool) != "gaze")
    unknown = sorted(tool for tool in rows if family(tool) == "gaze" and tool not in tagged
                     and tool not in GAZE_ROWS)
    if unknown:
        raise ValueError(f"Gaze rows must be main-tree evidence or a gaze-vX.Y.Z tag: {unknown}")
    return [*tagged, *other]


def _crates_tree(commit: str) -> str:
    return subprocess.check_output(["git", "rev-parse", f"{commit}:crates"], cwd=REPO, text=True).strip()


def add_tagged(data: dict[str, Any], report: Mapping[str, Any], own: Mapping[str, Any],
               resolve: Callable[[str], tuple[str, str]] | None = None) -> str:
    """Merge one tagged Gaze row (theirbench.py --gaze-release-tag) into the aggregate.

    The row joins only if the report measured the same benchmark identity, roster labels,
    splits, label maps and pinned comparison code as the committed entry; came from a clean
    harness and a clean checkout of the tag, with a binary built from it, the release's pinned
    policy and model digests, and a second run that reproduced its predictions byte for byte.
    `resolve(tag)` returns (commit, crates tree) of the tag; tests inject it.
    """
    if report.get("preflight") or report.get("harness_dirty") or report.get("schema_version") != 1:
        raise ValueError("only a clean, full, schema-1 report can add a tagged row")
    name = report["benchmark"]
    entry = data["benchmarks"][name]
    for key in ("identity", "common_intersection_labels", "splits", "label_maps_sha256",
                "mapping_sha256", "typed_hold"):
        if report[key] != entry[key]:
            raise ValueError(f"{name}: the report's {key} differs from the committed entry")
    # The committed rows were rescored with today's pinned metric code; the new row was
    # measured with it, so it must equal the rescore's pins, not the original measurement's.
    if report["comparison_sha256"] != entry["rescored_with"]["comparison_sha256"]:
        raise ValueError(f"{name}: the report used different pinned comparison code")
    rows = [tool for tool in report["rows"] if is_tagged_gaze_row(tool)]
    if len(rows) != 1 or len(report["rows"]) != 1:
        raise ValueError("the report must hold exactly one gaze-vX.Y.Z row and nothing else")
    row = rows[0]
    release = report["provenance"][row].get("release")
    if not release or f"gaze-{release['tag']}" != row:
        raise ValueError(f"{row}: provenance does not name the release checkout it was measured from")
    if row in entry["rows"]:
        raise ValueError(f"{row} is already in {name}")
    if own["system"] != row:
        raise ValueError(f"own-scorer result is for {own['system']}, not {row}")
    _check_release(row, release, resolve or (lambda tag: (tag_commit(tag, REPO), _crates_tree(tag_commit(tag, REPO)))))
    scored = own.get("scored") or own["overall"]
    entry["rows"][row] = report["rows"][row]["test"]
    entry["own_metric"][row] = scored
    entry["provenance"][row] = report["provenance"][row]
    reproduced = release["reproduces"]
    entry.setdefault("tagged_measurements", {})[row] = {
        "harness_revision": report["harness_revision"], "harness_dirty": False,
        "hardware": report["hardware"], "generated_at": report["generated_at"],
        "runs": [
            {"prediction_sha256": reproduced["prediction_sha256"], "binary_sha256": reproduced["binary_sha256"],
             "harness_revision": reproduced["harness_revision"]},
            {"prediction_sha256": release["prediction_sha256"], "binary_sha256": release["build"]["binary_sha256"],
             "harness_revision": release["measured_with"]["harness_revision"]},
        ],
    }
    return row


def source_text(url: str) -> str:
    """`https://github.com/<org>/<repo>/blob/<sha>/<path>` as `<org>/<repo> <path>`.

    The page states the source as text: a published-docs scrub tokenizes a bare deep link,
    and the repository, path and commit identify it without one.
    """
    head, _, rest = url.removeprefix("https://github.com/").partition("/blob/")
    return f"`{head}` `{rest.partition('/')[2]}`"


def add_tuned(data: dict[str, Any], report: Mapping[str, Any], own: Mapping[str, Any],
              declaration: Mapping[str, Any]) -> str:
    """Merge a vendor's own tuned setup (theirbench.py --vendor-tuned) into the aggregate.

    The benchmark's `vendor_tuned` entry then replaces that family's declared bar on the panel:
    a vendor's own benchmark is charted against the vendor's best published setup. The row joins
    only if the report measured the same identity, labels, splits, label maps and pinned code as
    the committed entry, from a clean harness, and the provenance names the declared setup, its
    source and commit; the own-scorer result must be the notebook-5 replay of the same spans.
    """
    if report.get("preflight") or report.get("harness_dirty") or report.get("schema_version") != 1:
        raise ValueError("only a clean, full, schema-1 report can add a vendor-tuned row")
    name = report["benchmark"]
    entry = data["benchmarks"][name]
    for key in ("identity", "common_intersection_labels", "splits", "label_maps_sha256",
                "mapping_sha256", "typed_hold"):
        if report[key] != entry[key]:
            raise ValueError(f"{name}: the report's {key} differs from the committed entry")
    if report["comparison_sha256"] != entry["rescored_with"]["comparison_sha256"]:
        raise ValueError(f"{name}: the report used different pinned comparison code")
    row = declaration["row"]
    if list(report["rows"]) != [row]:
        raise ValueError(f"the report must hold exactly the vendor-tuned row {row} and nothing else")
    tuned = report["provenance"][row].get("vendor_tuned")
    if not tuned or any(tuned.get(key) != declaration[key] for key in ("setup", "source", "commit", "caption")):
        raise ValueError(f"{row}: provenance does not match the declared vendor setup")
    if tuned["producer"].get("smoke_limit") or tuned["producer"]["raw_sha256"] != tuned["raw_sha256"]:
        raise ValueError(f"{row}: the producer record is a smoke run or does not describe the findings")
    if row in entry["rows"] or "vendor_tuned" in entry:
        raise ValueError(f"{name} already has a vendor-tuned row")
    if own["system"] != row or not own.get("tuned_replay") or own.get("smoke_limit"):
        raise ValueError(f"the own-scorer result must be the full notebook-5 replay of {row}")
    if not entry["reproduction"].get("reproduced", {}).get("custom"):
        raise ValueError(f"{name}: the vendor's tuned number must be reproduced first")
    entry["rows"][row] = report["rows"][row]["test"]
    entry["own_metric"][row] = own["scored"]
    entry["provenance"][row] = report["provenance"][row]
    entry["vendor_tuned"] = {declaration["family"]: {key: declaration[key] for key in (
        "row", "bar_name", "caption", "setup", "source", "commit")}}
    entry["chart_rows"] = [row if chosen == f"{declaration['family']}-strong" else chosen
                           for chosen in entry["chart_rows"]]
    if row not in entry["chart_rows"]:
        raise ValueError(f"{name}: no declared {declaration['family']} chart row to replace")
    entry["tuned_measurement"] = {
        "harness_revision": report["harness_revision"], "hardware": report["hardware"],
        "generated_at": report["generated_at"], "own_evaluator_commit": own["evaluator_commit"]}
    return row


def _check_release(row: str, release: Mapping[str, Any], resolve: Callable[[str], tuple[str, str]]) -> None:
    """The release provenance must tie the numbers to the tag, one build, the pins and a reproduction."""
    tag = release["tag"]
    commit, tree = resolve(tag)
    if release["commit"] != commit or release["crates_tree"] != tree:
        raise ValueError(f"{row}: provenance names {release['commit'][:12]}/{release['crates_tree'][:12]}, "
                         f"the tag is {commit[:12]}/{tree[:12]}")
    pins = RELEASE_PINS.get(tag)
    if pins is None or any(release.get(key) != value for key, value in pins.items()):
        raise ValueError(f"{row}: policy or model digests differ from the pinned ones for {tag}")
    build = release.get("build")
    if not build or not build.get("binary_sha256") or "safety-net-nym" not in build.get("command", ""):
        raise ValueError(f"{row}: no build record: the binary must be built from the tag by the harness")
    if release.get("measured_with", {}).get("harness_dirty") is not False:
        raise ValueError(f"{row}: measured with a dirty or unrecorded harness")
    reproduced = release.get("reproduces")
    if (not reproduced or reproduced["prediction_sha256"] != release.get("prediction_sha256")
            or reproduced.get("harness_dirty") is not False):
        raise ValueError(f"{row}: no clean earlier run reproduces these predictions")


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


def held(entry: Mapping[str, Any], tool: str) -> bool:
    return family(tool) in entry.get("typed_hold", ())


@dataclass(frozen=True)
class Metric:
    """One table column; the same list drives the table and the not-best lines."""
    title: str
    value: Callable[[str, Mapping[str, Any], str], float]
    display: Callable[[float], str]
    lower_is_better: bool
    held: Callable[[str, Mapping[str, Any], str], bool] = lambda name, entry, tool: False


def metrics(name: str) -> list[Metric]:
    field, label = OWN_METRIC[name]

    def product(key: str) -> Callable[[str, Mapping[str, Any], str], float]:
        return lambda n, e, t: e["rows"][t]["product_coverage"][key]

    def typed(key: str) -> Callable[[str, Mapping[str, Any], str], float]:
        return lambda n, e, t: e["rows"][t]["product_coverage"]["typed_entities"][key]

    def typed_held(n: str, e: Mapping[str, Any], t: str) -> bool:
        return held(e, t)

    def own_held(n: str, e: Mapping[str, Any], t: str) -> bool:
        # Presidio Research's binary PII-vs-O F2 ignores types; PIIBench's seqeval F1 does not.
        return n == "piibench-commercial" and held(e, t)

    return [
        Metric("Leaked B", product("leaked_bytes"), lambda v: f"{v:,}", True),
        Metric("FP B", product("false_positive_bytes"), lambda v: f"{v:,}", True),
        Metric("Doc leak rate", product("document_leak_rate"), pct, True),
        Metric("Typed F1", typed("f1"), lambda v: f"{v:.3f}", False, typed_held),
        Metric("Typed F2", typed("f2"), lambda v: f"{v:.3f}", False, typed_held),
        Metric("Leaked B, common", lambda n, e, t: e["rows"][t]["common_intersection"]["leaked_bytes"],
               lambda v: f"{v:,}", True),
        Metric(f"Own metric ({label})", lambda n, e, t: e["own_metric"][t][field],
               lambda v: f"{v:.3f}", False, own_held),
    ]


def cell(metric: Metric, name: str, entry: Mapping[str, Any], tool: str) -> str:
    return HELD if metric.held(name, entry, tool) else metric.display(metric.value(name, entry, tool))


def render(data: Mapping[str, Any]) -> str:
    lines = [
        "Report-only: these sets are never used to design or tune Gaze rules. Every gold "
        "label counts (no scored-label contract). Leaked and false-positive bytes use the "
        "same scorer code as the main comparison; each benchmark's own metric comes from "
        "its own evaluator, fed the same spans. Lower leaked bytes is better. The table lists every "
        "measured competitor row and every tagged Gaze release; untagged builds are not shown. "
        "No latency is published here: the machine was shared during these runs, and "
        "per-row foreign-CPU samples are kept in their-benchmarks.json. Competitor rows use the "
        "main comparison's configurations; Presidio's default rows keep score threshold 0.0, so "
        "they differ from the notebook's vanilla configuration (threshold 0.4). Both sets are "
        "English only, so Presidio's three language configurations give identical rows.",
        "",
    ]
    for name, entry in data["benchmarks"].items():
        rows, metric, metric_label = entry["rows"], *OWN_METRIC[name]
        gold = gold_bytes(rows[public_rows(rows)[0]]["product_coverage"])
        chart_rows(entry)  # every declared competitor configuration was measured
        lines += [f"#### {TITLES[name]}", ""]
        repro = entry["reproduction"]
        if name == "presidio-research":
            for config, published in repro["published"].items():
                old = repro.get("historical", {}).get(config)
                if old is None:
                    raise ValueError(f"presidio-research {config}: no historical reproduction")
                lines.append(
                    f"- Presidio {config} ({published['source']}): published F2 {published['f2']}; "
                    f"reproduced {old['f2']} with the evaluator at `{old['evaluator_commit'][:8]}`, the "
                    f"version that produced the published number; {repro['reproduced'][config]['f2']} with "
                    f"the pinned evaluator, which scores every row below.")
        else:
            published = repro["published_full_mix"]
            lines.append(f"- Published Presidio span F1 {published['f1']} is on the full ten-source mix "
                         f"({published['records']:,} records) and is quoted, not reproduced. PIIBench's own "
                         f"harness gives Presidio {repro['reproduced_commercial']['f1']} on this commercial subset.")
            used = ", ".join(source["dataset"] for source in entry["identity"]["sources"])
            excluded = "; ".join(f"{dataset}: {why}" for dataset, why in entry["identity"]["excluded"].items())
            lines.append(f"- Only these sources run: {used}. Excluded for their licences: {excluded}.")
            lines.append("- PIIBench's current code keeps 71 label types where its paper reports 48: its "
                         "normaliser maps only ai4privacy-style names. `MISC` and `FINANCIAL_ENTITY` are gold.")
        lines += ["", f"Gold PII bytes: {gold:,}. Common-intersection labels: "
                  f"{', '.join(entry['common_intersection_labels']) or 'none'}.", "",
                  *table_header([("Tool", False), *((metric.title, True) for metric in metrics(name))])]
        for tool in public_rows(rows):
            lines.append(f"| {tool} | " + " | ".join(cell(metric, name, entry, tool)
                                                    for metric in metrics(name)) + " |")
        rescored = entry["rescored_with"]
        for tool in (t for t in public_rows(rows) if is_tagged_gaze_row(t)):
            measured = entry["tagged_measurements"][tool]
            release = entry["provenance"][tool]["release"]
            lines += ["", f"Row {tool}: a clean checkout of tag `{release['tag']}` (crates tree "
                          f"`{release['crates_tree'][:8]}`, benchmark binary `{release['build']['binary_sha256'][:8]}`, reproduced by a second run) "
                          f"scored with harness `{measured['harness_revision'][:8]}`; no timing is published."]
        for family_name, choice in entry.get("vendor_tuned", {}).items():
            lines += ["", f"Row {choice['row']}: {choice['caption']}. Setup: {choice['setup']} "
                          f"(source {source_text(choice['source'])}, commit `{choice['commit'][:8]}`). "
                          f"It replaces the declared {family_name} configuration on the chart panel; "
                          "the other Presidio rows stay in this table."]
        if not any(is_tagged_gaze_row(tool) for tool in rows):
            latest = json.loads(RELEASE_HISTORY.read_text(encoding="utf-8"))["releases"][-1]["version"]
            lines += ["", f"Gaze {latest}: not yet measured on this set, so no Gaze row is shown."]
        lines += ["", f"Typed cells read \"{HELD}\" for tools whose labels pass through collision-family "
                  "or secret/password/token mappings, which the comparison's typed-scoring fix changed; "
                  "leaked and false-positive bytes do not depend on labels and are unaffected.",
                  "", f"Hardware: {entry['hardware']}. Measured with comparison code `{entry['comparison_revision']}`, "
                  f"typed metrics rescored with `{rescored['comparison_revision']}`; harness "
                  f"`{rescored['harness_revision'][:8]}`."
                  + ("".join(f" The {kind} harness commit is kept as signed tag `{tag}`."
                             for kind, tag in sorted(entry.get("harness_tags", {}).items()))), ""]
    lines += ["Not run:", ""] + [f"- {name}: {reason}." for name, reason in data["not_run"].items()]
    return check_public("\n".join(lines), "their-benchmarks block")


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
    build.add_argument("--harness-tag", action="append", default=[],
                       help="measured=<tag> or rescored=<tag>; must resolve to the recorded harness commit")
    build.add_argument("--historical", type=Path, action="append", default=[],
                       help="presidio_research_repro.py --reproduction result")
    tagged_cmd = sub.add_parser("add-tagged", help="merge one tagged Gaze row into their-benchmarks.json")
    tagged_cmd.add_argument("--report", type=Path, required=True)
    tagged_cmd.add_argument("--own", type=Path, required=True, help="the row's own-scorer result")
    tagged_cmd.add_argument("--data", type=Path, default=DATA)
    tuned_cmd = sub.add_parser("add-tuned", help="merge a vendor's own tuned setup into their-benchmarks.json")
    tuned_cmd.add_argument("--report", type=Path, required=True)
    tuned_cmd.add_argument("--own", type=Path, required=True, help="presidio_research_repro.py --tuned result")
    tuned_cmd.add_argument("--data", type=Path, default=DATA)
    show = sub.add_parser("render")
    show.add_argument("--check", action="store_true")
    show.add_argument("--data", type=Path, default=DATA)
    show.add_argument("--doc", type=Path, default=DOC)
    args = parser.parse_args(argv)
    if args.command == "assemble":
        DATA.write_text(json.dumps(assemble(args.report, args.own, args.reproduction, args.historical,
                                       dict(item.split("=", 1) for item in args.harness_tag)), indent=2,
                                   sort_keys=True) + "\n", encoding="utf-8")
        return 0
    if args.command == "add-tagged":
        data = json.loads(args.data.read_text(encoding="utf-8"))
        row = add_tagged(data, json.loads(args.report.read_text(encoding="utf-8")),
                         json.loads(args.own.read_text(encoding="utf-8")))
        args.data.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"added {row}")
        return 0
    if args.command == "add-tuned":
        data = json.loads(args.data.read_text(encoding="utf-8"))
        report = json.loads(args.report.read_text(encoding="utf-8"))
        declaration = json.loads(VENDOR_TUNED.read_text(encoding="utf-8"))[report["benchmark"]]
        row = add_tuned(data, report, json.loads(args.own.read_text(encoding="utf-8")), declaration)
        args.data.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"added {row}")
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
