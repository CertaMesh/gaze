#!/usr/bin/env python3
"""Render the generated sections of docs/reference/benchmarks/README.md.

Two jobs, deliberately split so CI never needs the benchmark corpus:

``--append-history``
    Reads one ``scorecard-vX.Y.Z.json`` (schema v4), extracts the headline
    fields into ``release-history.json``, then re-renders the document. This is
    the per-release step and runs on the machine that produced the scorecard.

``--check``
    Re-renders from ``release-history.json`` alone and fails if the committed
    document has drifted. Stdlib-only and corpus-free, so it runs in CI.

``release-history.json`` is the source of truth for everything the document
shows. The scorecard JSON stays committed as the machine-readable evidence and
is the *input* to ``--append-history``; keeping the rendered numbers in the
history file is what lets ``--check`` run without it.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import sys
from pathlib import Path
from typing import Any, Mapping, Sequence

REPO_ROOT = Path(__file__).resolve().parents[2]
BENCH_DIR = REPO_ROOT / "docs" / "reference" / "benchmarks"
DEFAULT_DOC = BENCH_DIR / "README.md"
DEFAULT_HISTORY = BENCH_DIR / "release-history.json"

HISTORY_SCHEMA_VERSION = 1
SCORECARD_SCHEMA_VERSION = 4

#: The shipped default arm. Its row drives the release-over-release trend chart.
SHIPPED_DEFAULT_ARM = "full-stack-kiji-resolve"

VERSION_RE = re.compile(r"^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")

# Scorecard JSON path -> history field. This table IS the contract between the
# harness output and every number the document prints; `test_render_benchmark_doc`
# mutates each source path and asserts the extracted value moves.
ARM_FIELD_SOURCES: dict[str, tuple[str, ...]] = {
    "gold_pii_utf8_bytes": ("metrics", "utf8_bytes", "pii"),
    "surviving_pii_utf8_bytes": ("metrics", "utf8_bytes", "leaked"),
    "leak_rate": ("metrics", "utf8_bytes", "leak_rate"),
    "false_positive_utf8_bytes": ("metrics", "utf8_bytes", "false_positive"),
    "byte_precision": ("metrics", "utf8_bytes", "precision"),
    "zero_leak_document_rate": ("metrics", "zero_leak_document_rate"),
    "restore_exact_rate": ("pipeline_contract", "restore_exact_rate"),
    "manifest_valid_document_rate": ("pipeline_contract", "manifest_valid_document_rate"),
    "availability_completion_rate": ("pipeline_availability", "completion_rate"),
    "failed_closed_documents": ("pipeline_availability", "failed_closed_documents"),
    "clean_ms_p95": ("latency_ms", "clean_ms", "p95"),
}

# Rendered table layout: (header, history field, formatter key).
ARM_COLUMNS: tuple[tuple[str, str, str], ...] = (
    ("Gold PII bytes info", "gold_pii_utf8_bytes", "int"),
    ("Surviving PII bytes ↓", "surviving_pii_utf8_bytes", "int"),
    ("Leak rate ↓", "leak_rate", "pct"),
    ("False-positive bytes ↔", "false_positive_utf8_bytes", "int"),
    ("Byte precision ↑", "byte_precision", "rate"),
    ("Zero-leak documents ↑", "zero_leak_document_rate", "pct"),
    ("Restore exact ↑", "restore_exact_rate", "pct"),
    ("Manifest valid ↑", "manifest_valid_document_rate", "pct"),
    ("Availability ↑", "availability_completion_rate", "pct"),
    ("Failed closed ↓", "failed_closed_documents", "int"),
    ("clean p95 ms ↓", "clean_ms_p95", "ms"),
)

BLOCK_NAMES = ("current-release", "charts", "history")


class RenderError(Exception):
    """Raised for malformed input; the CLI turns it into exit code 2."""


# --------------------------------------------------------------------------
# formatting helpers
# --------------------------------------------------------------------------


def _fmt(kind: str, value: Any) -> str:
    """Format one cell. Fixed widths keep `--check` byte-stable across hosts."""
    if value is None:
        return "n/a"
    if kind == "int":
        return f"{int(value):,}"
    if kind == "pct":
        return f"{float(value) * 100:.4f}%"
    if kind == "rate":
        return f"{float(value):.6f}"
    if kind == "ms":
        return f"{float(value):.2f}"
    raise RenderError(f"unknown format kind {kind!r}")


def _axis_max(values: Sequence[float]) -> int:
    """Round the chart ceiling up to 1 significant figure so ticks stay round."""
    peak = max([float(v) for v in values] + [0.0])
    if peak <= 0:
        return 1
    step = 10 ** max(0, int(math.floor(math.log10(peak))) - 1)
    return int(math.ceil(peak * 1.1 / step) * step)


def _mermaid_labels(labels: Sequence[str]) -> str:
    return "[" + ", ".join(f'"{label}"' for label in labels) + "]"


# --------------------------------------------------------------------------
# history file
# --------------------------------------------------------------------------


def empty_history() -> dict[str, Any]:
    return {
        "schema_version": HISTORY_SCHEMA_VERSION,
        "comment": (
            "Release-keyed benchmark history. One entry per released version, "
            "appended by scripts/bench/render_benchmark_doc.py --append-history "
            "from that release's committed scorecard-vX.Y.Z.json."
        ),
        "shipped_default_arm": SHIPPED_DEFAULT_ARM,
        "releases": [],
    }


def load_history(path: Path) -> dict[str, Any]:
    if not path.exists():
        raise RenderError(f"history file is missing: {path}")
    try:
        history = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise RenderError(f"history file is not valid JSON: {path}: {error}") from error
    validate_history(history)
    return history


def validate_history(history: Mapping[str, Any]) -> None:
    if not isinstance(history, Mapping):
        raise RenderError("history must be a JSON object")
    if history.get("schema_version") != HISTORY_SCHEMA_VERSION:
        raise RenderError(
            f"history schema_version must be {HISTORY_SCHEMA_VERSION}, "
            f"got {history.get('schema_version')!r}"
        )
    releases = history.get("releases")
    if not isinstance(releases, list):
        raise RenderError("history releases must be an array")
    seen: set[str] = set()
    for entry in releases:
        if not isinstance(entry, Mapping):
            raise RenderError("every history release must be an object")
        version = entry.get("version")
        if not isinstance(version, str) or not VERSION_RE.match(version):
            raise RenderError(f"invalid release version {version!r}")
        if version in seen:
            raise RenderError(f"duplicate release version {version}")
        seen.add(version)
        arms = entry.get("arms")
        if not isinstance(arms, Mapping) or not arms:
            raise RenderError(f"{version}: arms must be a non-empty object")
        for arm, block in arms.items():
            if not isinstance(block, Mapping):
                raise RenderError(f"{version}/{arm}: arm block must be an object")
            missing = [field for field in ARM_FIELD_SOURCES if field not in block]
            if missing:
                raise RenderError(f"{version}/{arm}: missing fields {missing}")


def write_history(path: Path, history: Mapping[str, Any]) -> None:
    path.write_text(json.dumps(history, indent=2) + "\n", encoding="utf-8")


# --------------------------------------------------------------------------
# scorecard -> history entry
# --------------------------------------------------------------------------


def _dig(node: Any, path: Sequence[str], where: str) -> Any:
    for key in path:
        if not isinstance(node, Mapping) or key not in node:
            raise RenderError(f"{where}: scorecard is missing {'.'.join(path)}")
        node = node[key]
    return node


def history_entry_from_scorecard(
    scorecard: Mapping[str, Any],
    *,
    version: str,
    machine: str,
    scorecard_filename: str,
    scorecard_sha256: str,
    provisional: bool = False,
    note: str = "",
) -> dict[str, Any]:
    """Project one schema-v4 scorecard onto the fields the document prints."""
    if not VERSION_RE.match(version):
        raise RenderError(f"--version must look like v1.2.3, got {version!r}")
    if scorecard.get("schema_version") != SCORECARD_SCHEMA_VERSION:
        raise RenderError(
            f"scorecard schema_version must be {SCORECARD_SCHEMA_VERSION}, "
            f"got {scorecard.get('schema_version')!r}"
        )
    runs = scorecard.get("runs")
    if not isinstance(runs, list) or not runs:
        raise RenderError("scorecard runs must be a non-empty array")

    gaze = scorecard.get("gaze")
    if not isinstance(gaze, Mapping) or not isinstance(gaze.get("revision"), str):
        raise RenderError("scorecard gaze.revision is missing")
    if gaze.get("dirty"):
        raise RenderError(
            "refusing a scorecard produced from a dirty tree "
            "(gaze.dirty is true); a release row must be reproducible"
        )

    dataset = scorecard.get("dataset")
    if not isinstance(dataset, Mapping):
        raise RenderError("scorecard dataset must be an object")
    parameters = scorecard.get("parameters")
    if not isinstance(parameters, Mapping):
        raise RenderError("scorecard parameters must be an object")
    provenance = scorecard.get("runner_provenance")
    if not isinstance(provenance, Mapping):
        raise RenderError("scorecard runner_provenance must be an object")

    arms: dict[str, Any] = {}
    for run in runs:
        if not isinstance(run, Mapping) or not isinstance(run.get("config"), str):
            raise RenderError("every scorecard run needs a string config")
        config = run["config"]
        if config in arms:
            raise RenderError(f"duplicate scorecard config {config}")
        arms[config] = {
            field: _dig(run, path, f"run {config}")
            for field, path in ARM_FIELD_SOURCES.items()
        }

    integrity = dataset.get("integrity")
    return {
        "version": version,
        "commit": gaze["revision"],
        "date": str(scorecard.get("generated_at", ""))[:10],
        "scorecard": scorecard_filename,
        "scorecard_sha256": scorecard_sha256,
        "machine": machine,
        "harness_entry": provenance.get("entry_point"),
        "dataset": {
            "repository": dataset.get("repository"),
            "revision": dataset.get("revision"),
            "integrity": integrity if isinstance(integrity, Mapping) else {},
        },
        "parameters": {
            "profile": parameters.get("profile"),
            "sampling_seed": parameters.get("sampling_seed"),
            "ner_threshold": parameters.get("ner_threshold"),
        },
        "provisional": bool(provisional),
        "note": note,
        "arms": arms,
    }


# --------------------------------------------------------------------------
# rendering
# --------------------------------------------------------------------------

_NO_RELEASES = (
    "> **No release has been measured yet.** The table and charts below fill in "
    "when a release runs the harness and appends its row. Produce one with the "
    "commands in [How to reproduce](#how-to-reproduce)."
)


def render_current_release(history: Mapping[str, Any]) -> str:
    releases = history["releases"]
    if not releases:
        return _NO_RELEASES
    entry = releases[-1]
    lines: list[str] = []
    if entry.get("provisional"):
        claim = "**(provisional)** — *not* measured on the released tree."
    else:
        claim = "— measured on the released tree."
    lines.append(f"**{entry['version']}** {claim}")
    lines.append("")
    if entry.get("note"):
        lines.append(f"> {entry['note']}")
        lines.append("")

    dataset = entry.get("dataset") or {}
    integrity = dataset.get("integrity") or {}
    parameters = entry.get("parameters") or {}
    lines.extend(
        [
            "| Provenance info | Value info |",
            "| --- | --- |",
            f"| Release | `{entry['version']}` |",
            f"| Commit | `{entry['commit']}` |",
            f"| Measured | {entry['date']} |",
            f"| Machine | {entry['machine']} |",
            f"| Harness | [`{entry['harness_entry']}`]"
            f"(../../../{entry['harness_entry']}) |",
            f"| Scorecard | [`{entry['scorecard']}`]({entry['scorecard']}) |",
            f"| Scorecard sha256 | `{entry['scorecard_sha256']}` |",
            f"| Corpus | `{dataset.get('repository')}` @ `{dataset.get('revision')}` |",
            f"| Corpus {integrity.get('algorithm', 'sha256')} | "
            f"`{integrity.get('value', 'n/a')}` |",
            f"| Profile | `{parameters.get('profile')}` |",
            f"| Seed | `{parameters.get('sampling_seed')}` |",
            f"| NER threshold | `{parameters.get('ner_threshold')}` |",
            "",
        ]
    )

    headers = ["Arm info"] + [column[0] for column in ARM_COLUMNS]
    lines.append("| " + " | ".join(headers) + " |")
    lines.append("| --- | " + " | ".join(["---:"] * len(ARM_COLUMNS)) + " |")
    for arm, block in entry["arms"].items():
        cells = [_fmt(kind, block[field]) for _, field, kind in ARM_COLUMNS]
        label = f"`{arm}`"
        if arm == history.get("shipped_default_arm", SHIPPED_DEFAULT_ARM):
            label += " **(shipped default)**"
        lines.append("| " + " | ".join([label] + cells) + " |")
    return "\n".join(lines)


def render_charts(history: Mapping[str, Any]) -> str:
    releases = history["releases"]
    if not releases:
        return (
            "> Charts render once at least one release row exists in "
            "[`release-history.json`](release-history.json)."
        )
    entry = releases[-1]
    arms = list(entry["arms"].items())
    surviving = [block["surviving_pii_utf8_bytes"] for _, block in arms]

    lines = [
        f"**Surviving PII bytes per arm — {entry['version']}.** Lower is better; "
        "the goal is zero.",
        "",
        "```mermaid",
        "xychart-beta",
        f'    title "Surviving PII bytes per arm - {entry["version"]}"',
        f"    x-axis {_mermaid_labels([arm for arm, _ in arms])}",
        f'    y-axis "Surviving PII bytes (lower is better)" 0 --> {_axis_max(surviving)}',
        f"    bar [{', '.join(str(int(value)) for value in surviving)}]",
        "```",
    ]

    default_arm = history.get("shipped_default_arm", SHIPPED_DEFAULT_ARM)
    trend = [
        (item["version"], item["arms"][default_arm]["surviving_pii_utf8_bytes"])
        for item in releases
        if default_arm in item["arms"]
    ]
    lines.extend(["", f"**Trend across releases — `{default_arm}`.**"])
    if len(trend) < 2:
        lines.extend(
            [
                "",
                f"> One measured release so far ({len(trend)} point). The trend "
                "chart renders from two releases onward.",
            ]
        )
        return "\n".join(lines)
    lines.extend(
        [
            "",
            "```mermaid",
            "xychart-beta",
            f'    title "Surviving PII bytes on {default_arm} across releases"',
            f"    x-axis {_mermaid_labels([version for version, _ in trend])}",
            '    y-axis "Surviving PII bytes (lower is better)" 0 --> '
            f"{_axis_max([value for _, value in trend])}",
            f"    line [{', '.join(str(int(value)) for _, value in trend)}]",
            "```",
        ]
    )
    return "\n".join(lines)


def render_history(history: Mapping[str, Any]) -> str:
    releases = history["releases"]
    if not releases:
        return (
            "| Release | Measured | Commit | Machine | Scorecard | "
            "Surviving PII bytes ↓ |\n"
            "| --- | --- | --- | --- | --- | ---: |\n"
            "| *none yet* | — | — | — | — | — |"
        )
    default_arm = history.get("shipped_default_arm", SHIPPED_DEFAULT_ARM)
    lines = [
        "| Release | Measured | Commit | Machine | Scorecard | "
        "Surviving PII bytes ↓ |",
        "| --- | --- | --- | --- | --- | ---: |",
    ]
    for entry in releases:
        block = entry["arms"].get(default_arm)
        surviving = (
            _fmt("int", block["surviving_pii_utf8_bytes"]) if block else "n/a"
        )
        version = entry["version"]
        if entry.get("provisional"):
            version += " *(provisional)*"
        lines.append(
            f"| {version} | {entry['date']} | `{entry['commit'][:7]}` | "
            f"{entry['machine']} | [`{entry['scorecard']}`]({entry['scorecard']}) | "
            f"{surviving} |"
        )
    return "\n".join(lines)


RENDERERS = {
    "current-release": render_current_release,
    "charts": render_charts,
    "history": render_history,
}


def begin_marker(name: str) -> str:
    return f"<!-- BEGIN GENERATED: {name} -->"


def end_marker(name: str) -> str:
    return f"<!-- END GENERATED: {name} -->"


def apply_blocks(document: str, history: Mapping[str, Any]) -> str:
    """Replace each generated block in place, leaving all prose untouched."""
    for name in BLOCK_NAMES:
        begin, end = begin_marker(name), end_marker(name)
        start = document.find(begin)
        stop = document.find(end)
        if start < 0 or stop < 0:
            raise RenderError(f"document is missing the {name!r} generated block")
        if stop < start:
            raise RenderError(f"{name!r} markers are out of order")
        body = RENDERERS[name](history)
        document = (
            document[: start + len(begin)] + "\n\n" + body + "\n\n" + document[stop:]
        )
    return document


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--doc", type=Path, default=DEFAULT_DOC)
    parser.add_argument("--history", type=Path, default=DEFAULT_HISTORY)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if the committed document has drifted from the history file",
    )
    parser.add_argument(
        "--append-history",
        action="store_true",
        help="extract --scorecard into the history file before rendering",
    )
    parser.add_argument("--scorecard", type=Path)
    parser.add_argument("--version", dest="release_version")
    parser.add_argument(
        "--machine",
        help=(
            "hardware spec for this run. Required with --append-history: the "
            "scorecard schema does not capture the host, so this is the one "
            "hand-carried reproducibility field."
        ),
    )
    parser.add_argument(
        "--provisional",
        action="store_true",
        help="mark the row as not measured on the released tree",
    )
    parser.add_argument("--note", default="", help="caveat shown with the row")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        history = load_history(args.history)

        if args.append_history:
            if args.check:
                raise RenderError("--append-history and --check are mutually exclusive")
            missing = [
                flag
                for flag, value in (
                    ("--scorecard", args.scorecard),
                    ("--version", args.release_version),
                    ("--machine", args.machine),
                )
                if not value
            ]
            if missing:
                raise RenderError(f"--append-history requires {', '.join(missing)}")
            scorecard_path = args.scorecard
            if not scorecard_path.exists():
                raise RenderError(f"scorecard not found: {scorecard_path}")
            expected = f"scorecard-{args.release_version}.json"
            if scorecard_path.name != expected:
                raise RenderError(
                    f"scorecard must be named {expected} for {args.release_version}, "
                    f"got {scorecard_path.name}"
                )
            entry = history_entry_from_scorecard(
                json.loads(scorecard_path.read_text(encoding="utf-8")),
                version=args.release_version,
                machine=args.machine,
                scorecard_filename=scorecard_path.name,
                scorecard_sha256=_sha256(scorecard_path),
                provisional=args.provisional,
                note=args.note,
            )
            history["releases"].append(entry)
            history["releases"].sort(
                key=lambda item: [
                    int(part)
                    for part in item["version"].lstrip("v").split("-")[0].split(".")
                ]
            )
            validate_history(history)
            write_history(args.history, history)

        for entry in history["releases"]:
            evidence = args.history.parent / entry["scorecard"]
            if not evidence.exists():
                raise RenderError(
                    f"{entry['version']} names {entry['scorecard']}, which is not "
                    "committed; the machine-readable evidence must stay in the tree"
                )

        original = args.doc.read_text(encoding="utf-8")
        rendered = apply_blocks(original, history)

        if args.check:
            if rendered != original:
                sys.stderr.write(
                    f"{args.doc} is out of sync with {args.history}.\n"
                    "Re-run scripts/bench/render_benchmark_doc.py and commit the result.\n"
                )
                return 1
            print(f"render_benchmark_doc: {args.doc.name} is in sync")
            return 0

        args.doc.write_text(rendered, encoding="utf-8")
        print(
            f"render_benchmark_doc: wrote {args.doc.name} "
            f"({len(history['releases'])} release rows)"
        )
        return 0
    except RenderError as error:
        sys.stderr.write(f"render_benchmark_doc: {error}\n")
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
