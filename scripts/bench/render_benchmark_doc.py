#!/usr/bin/env python3
"""Render the generated sections of docs/reference/benchmarks/README.md.

The root README's leaked-bytes chart is a generated block too, rendered from the
same history whenever the committed history is the input.

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
DEFAULT_README = REPO_ROOT / "README.md"

HISTORY_SCHEMA_VERSION = 1
SCORECARD_SCHEMA_VERSION = 4

#: The shipped default arm for rows appended now. Each appended row records it
#: (`shipped_default_arm`), because the default changes between releases and a
#: historical row must keep naming the arm that was actually shipped. Must equal
#: `gaze_bench_score.PRODUCTION_CONFIG`; test_openpii_gaze_bench pins that.
SHIPPED_DEFAULT_ARM = "pass2-ner"

#: Rows appended before entries recorded their own shipped default. Keyed by
#: version and never extended: a new row records its arm instead. v0.14.0 shipped
#: the Kiji DistilBERT safety net, which was removed afterwards.
LEGACY_SHIPPED_DEFAULT_ARMS: dict[str, str] = {
    "v0.14.0": "full-stack-kiji-resolve",
}

VERSION_RE = re.compile(r"^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")
HEX64_RE = re.compile(r"^[0-9a-f]{64}$")

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

# Validator-backed label table for the shipped default arm: (header, history
# field, formatter key). Scorecard source per field lives in
# `_validator_row_from_scorecard`; a row exists only for an applicable label.
VALIDATOR_COLUMNS: tuple[tuple[str, str, str], ...] = (
    ("Gold", "gold_spans", "int"),
    ("Gold failing its validator", "validator_failed_gold_spans", "int"),
    ("Validator-backed recall", "validator_backed_full_coverage_recall", "rate"),
    ("Shape recall", "shape_only_full_coverage_recall", "rate"),
    ("Leaked bytes, valid gold", "leaked_utf8_bytes_validator_passed_gold", "int"),
    ("Leaked bytes, invalid gold", "leaked_utf8_bytes_validator_failed_gold", "int"),
)

# Contract v3 gold-gap diagnostic: (header, arm gold_gap field, formatter
# key). Scorecard source is `metrics.gold_gap`; rendered beside, never in place
# of, the v2 columns above.
GOLD_GAP_COLUMNS: tuple[tuple[str, str, str], ...] = (
    ("Gold-gap protected bytes", "gold_gap_protected_bytes", "int"),
    ("False-positive bytes after gold-gap", "false_positive_bytes_after_gold_gap", "int"),
    ("Adjusted byte precision", "adjusted_precision", "rate"),
)

BLOCK_NAMES = ("current-release", "charts", "history", "latency")
README_BLOCK_NAMES = ("readme-chart",)

#: Plain-English chart labels for the arms a released row can carry. The table
#: keeps the arm ids; a chart axis has no room for them. Unknown arms fall back
#: to their id, so a new arm still renders.
ARM_CHART_LABELS: dict[str, str] = {
    "rule-floor-extended": "rules only",
    "pass2-ner": "rules + NER",
    "full-stack-kiji-resolve": "rules + NER + Kiji",
    "full-stack-nym-resolve": "rules + NER + Nym",
    "policy-file": "gaze setup policy",
}


class RenderError(Exception):
    """Raised for malformed input; the CLI turns it into exit code 2."""


# --------------------------------------------------------------------------
# provenance predicates
#
# One vocabulary for all three doors. `--append-history` validates on the way
# in, `validate_history` re-validates what `--check` renders from, and the
# renderer reads its provenance cells through these same calls. A value that is
# not a digest therefore cannot reach the document by any path, and dropping a
# fallback back in at the render site stays a change the tests can see.
# --------------------------------------------------------------------------


def _require_hex64(value: Any, where: str) -> str:
    if not isinstance(value, str) or not HEX64_RE.match(value):
        raise RenderError(
            f"{where} must be a 64-character lowercase hex digest, got {value!r}; "
            "a released row names the evidence it was measured on, so this is "
            "refused rather than rendered as n/a"
        )
    return value


def _require_nonneg_int(value: Any, where: str) -> int:
    # `isinstance(True, int)` is True, and a stray `True` would publish as
    # `1 documents`, so bools are refused explicitly.
    if not isinstance(value, int) or isinstance(value, bool) or value < 0:
        raise RenderError(
            f"{where} must be a non-negative integer, got {value!r}; the "
            "document has to state the population its numbers describe"
        )
    return value


def _require_component_digests(integrity: Any, where: str) -> dict[str, str]:
    """Every per-component corpus digest, validated and normalised as one unit."""
    if not isinstance(integrity, Mapping):
        raise RenderError(f"{where} dataset.integrity must be an object")
    components = integrity.get("component_sha256", {})
    if not isinstance(components, Mapping):
        raise RenderError(
            f"{where} dataset.integrity.component_sha256 must be an object, "
            f"got {components!r}"
        )
    return {
        str(name): _require_hex64(
            digest, f"{where} dataset.integrity.component_sha256.{name}"
        )
        for name, digest in sorted(components.items())
    }


def _require_model_bundles(provenance: Any, where: str) -> list[dict[str, str]]:
    """The SHA pins a row ran against. Empty is a fact; vague is not."""
    if not isinstance(provenance, Mapping):
        raise RenderError(f"{where} provenance must be an object")
    bundles = provenance.get("model_bundles")
    if not isinstance(bundles, list):
        raise RenderError(
            f"{where} model_bundles must be an array (empty is fine for a "
            "rule-only run); this repo pins model bundles by SHA, so a released "
            "number names the ones it ran"
        )
    pinned: list[dict[str, str]] = []
    for bundle in bundles:
        if not isinstance(bundle, Mapping):
            raise RenderError(f"{where} every model bundle must be an object")
        model_id = bundle.get("model_id")
        if not isinstance(model_id, str) or not model_id:
            raise RenderError(
                f"{where} every model bundle needs a non-empty model_id, "
                f"got {model_id!r}"
            )
        pinned.append(
            {
                "model_id": model_id,
                "expected_sha256": _require_hex64(
                    bundle.get("expected_sha256"),
                    f"{where} model bundle {model_id} expected_sha256",
                ),
            }
        )
    return pinned


#: Provenance a released row may not omit, paired with the predicate its value
#: must satisfy. Presence alone was the hole: an entry whose `sha256` read
#: `"n/a"` published that string with `--check` green. A new required field is
#: one row here rather than a new branch at each of the three doors.
REQUIRED_PROVENANCE: tuple[tuple[tuple[str, ...], Any], ...] = (
    (("scorecard_sha256",), _require_hex64),
    (("dataset", "integrity", "sha256"), _require_hex64),
    (("dataset", "evaluated_population", "documents"), _require_nonneg_int),
    (("dataset", "evaluated_population", "entities"), _require_nonneg_int),
)


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


def _leak_pct_label(label: str, arm: Mapping[str, Any]) -> str:
    """Append the arm's leak rate to a chart axis label, e.g. `v0.15.0 (15.0%)`.

    GitHub's Mermaid renderer shows no data labels on xychart-beta, so the
    percentage rides in the category label. It is the history row's own
    `leak_rate` (leaked / gold bytes under the row's label contract), never a
    number computed or typed here.
    """
    return f"{label} ({float(arm['leak_rate']) * 100:.1f}%)"


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
            gold_gap = block.get("gold_gap")
            if gold_gap is not None and (
                not isinstance(gold_gap, Mapping)
                or any(field not in gold_gap for _, field, _ in GOLD_GAP_COLUMNS)
            ):
                raise RenderError(f"{version}/{arm}: malformed gold_gap diagnostic")
        # `--check` renders from this file alone, so the provenance guard has
        # to sit here as well as at extraction or the CI path stays ungated --
        # and it has to check the *values*. A key that is present but holds
        # `"n/a"` is exactly the published cell this document exists to refuse.
        for path, require in REQUIRED_PROVENANCE:
            node: Any = entry
            for key in path:
                if not isinstance(node, Mapping) or key not in node:
                    raise RenderError(
                        f"{version}: history entry is missing {'.'.join(path)}"
                    )
                node = node[key]
            require(node, f"{version}: history {'.'.join(path)}")
        _require_component_digests(
            (entry.get("dataset") or {}).get("integrity"), f"{version}: history"
        )
        _require_model_bundles(entry.get("provenance") or {}, f"{version}: history")
        _validate_validator_recall(entry.get("validator_recall"), version)
        shipped_default_arm(entry)
        _validate_contract_results(entry)


def _validate_contract_results(entry: Mapping[str, Any]) -> None:
    version = entry["version"]
    results = entry.get("contract_results")
    if results is None:
        return
    if not isinstance(results, list) or not results:
        raise RenderError(f"{version}: contract_results must be a non-empty array")
    seen = {_contract_key(entry)[0]}
    for result in results:
        if not isinstance(result, Mapping):
            raise RenderError(f"{version}: every contract result must be an object")
        contract = result.get("scored_label_contract")
        if not isinstance(contract, Mapping) or type(contract.get("version")) is not int:
            raise RenderError(f"{version}: contract result needs a scored_label_contract")
        number = contract["version"]
        if number in seen:
            raise RenderError(
                f"{version}: scored labels v{number} is recorded twice for this release"
            )
        seen.add(number)
        where = f"{version}: contract result v{number}"
        _require_hex64(contract.get("file_sha256"), f"{where} file_sha256")
        _require_hex64(result.get("scorecard_sha256"), f"{where} scorecard_sha256")
        expected = contract_scorecard_name(version, number)
        if result.get("scorecard") != expected:
            raise RenderError(f"{where}: scorecard must be named {expected}")
        arms = result.get("arms")
        if not isinstance(arms, Mapping) or not arms:
            raise RenderError(f"{where}: arms must be a non-empty object")
        for arm, block in arms.items():
            missing = [f for f in ARM_FIELD_SOURCES if not isinstance(block, Mapping) or f not in block]
            if missing:
                raise RenderError(f"{where}/{arm}: missing fields {missing}")
        if shipped_default_arm(entry) not in arms:
            raise RenderError(f"{where}: the shipped default arm was not measured")


def contract_scorecard_name(version: str, contract_version: int) -> str:
    return f"scorecard-{version}-scored-labels-v{contract_version}.json"


def contract_result_from_scorecard(
    scorecard: Mapping[str, Any],
    parent: Mapping[str, Any],
    *,
    scorecard_filename: str,
    scorecard_sha256: str,
) -> dict[str, Any]:
    """One release re-scored under another contract, checked against its row.

    The run must be the same measurement except for the contract: same commit,
    corpus and population, and the shipped arm must be in it. Anything else
    would put numbers from two different trees side by side as one release.
    """
    projected = history_entry_from_scorecard(
        scorecard,
        version=parent["version"],
        machine=parent["machine"],
        scorecard_filename=scorecard_filename,
        scorecard_sha256=scorecard_sha256,
        shipped_arm=shipped_default_arm(parent),
    )
    contract = projected.get("scored_label_contract")
    if contract is None or contract["version"] == _contract_key(parent)[0]:
        raise RenderError(
            f"{parent['version']}: the scorecard is scored under the row's own "
            "contract; a contract result must use a different one"
        )
    for label, value, expected in (
        ("commit", projected["commit"], parent["commit"]),
        (
            "corpus sha256",
            projected["dataset"]["integrity"]["sha256"],
            parent["dataset"]["integrity"]["sha256"],
        ),
        (
            "population",
            projected["dataset"]["evaluated_population"],
            parent["dataset"]["evaluated_population"],
        ),
    ):
        if value != expected:
            raise RenderError(
                f"{parent['version']}: contract result {label} {value!r} differs "
                f"from the release row's {expected!r}"
            )
    return {
        "scored_label_contract": contract,
        "scorecard": scorecard_filename,
        "scorecard_sha256": scorecard_sha256,
        "arms": projected["arms"],
    }


def _validate_validator_recall(value: Any, version: str) -> None:
    if value is None:
        return
    if not isinstance(value, Mapping) or not value:
        raise RenderError(f"{version}: validator_recall must be a non-empty object")
    for label, row in value.items():
        if not isinstance(row, Mapping):
            raise RenderError(f"{version}: validator_recall.{label} must be an object")
        for _, field, kind in VALIDATOR_COLUMNS:
            cell = row.get(field)
            if kind == "int":
                _require_nonneg_int(cell, f"{version}: validator_recall.{label}.{field}")
            elif cell is not None and (
                isinstance(cell, bool) or not isinstance(cell, (int, float))
            ):
                raise RenderError(
                    f"{version}: validator_recall.{label}.{field} must be a number"
                )


def shipped_default_arm(entry: Mapping[str, Any]) -> str:
    """The arm a release row shipped as default: recorded, else legacy-mapped.

    Falling back to today's `SHIPPED_DEFAULT_ARM` would silently re-label an old
    release with a default it never shipped, so an unmapped row is refused.
    """
    version = entry.get("version")
    recorded = entry.get("shipped_default_arm")
    if recorded is None:
        recorded = LEGACY_SHIPPED_DEFAULT_ARMS.get(version) if isinstance(version, str) else None
    if not isinstance(recorded, str) or not recorded:
        raise RenderError(
            f"{version}: history entry does not record shipped_default_arm and "
            "is not a known legacy release"
        )
    arms = entry.get("arms")
    if not isinstance(arms, Mapping) or recorded not in arms:
        raise RenderError(
            f"{version}: shipped default arm {recorded!r} is not among the "
            "row's measured arms"
        )
    return recorded


def version_sort_key(version: str) -> tuple[Any, ...]:
    """Order releases by semver, prerelease before its release.

    Comparing only the numeric core makes `v0.14.0-rc.1` and `v0.14.0` sort
    equal, and since `releases[-1]` drives the whole Current release section a
    tie would publish whichever happened to be appended last.
    """
    core, _, prerelease = version.lstrip("v").partition("-")
    numbers = tuple(int(part) for part in core.split("."))
    if not prerelease:
        return numbers + (1, ())
    # Numeric identifiers compare numerically and rank below alphanumeric ones;
    # the leading 0/1 keeps the two kinds comparable instead of raising.
    identifiers = tuple(
        (0, int(part), "") if part.isdigit() else (1, 0, part)
        for part in prerelease.split(".")
    )
    return numbers + (0, identifiers)


#: How many distinct-result groups the history table and charts show. The
#: history file keeps every release; only the display is capped.
DISPLAYED_GROUPS = 3


def result_key(entry: Mapping[str, Any]) -> tuple[Any, ...]:
    """What makes two releases' benchmark results the same.

    The shipped arm and its result numbers (including the contract v3
    gold-gap diagnostic), read under one scored-label contract on one corpus,
    and whether the row claims the released tree. The rule sentence above the
    history table in the benchmark doc must list the same fields.
    Latency, date, commit and machine are left out on purpose: p95 moves with
    host load, so v0.15.0 (124 ms) and v0.15.1 (139 ms) on identical detection
    output are one result. Leaked bytes on the common document set are derived
    from refused and leaked bytes on one corpus, so they need no slot of their own.
    """
    return tuple(
        _result_numbers(view)
        for view in (contract_view(entry, v) for v in measured_contracts(entry))
        if view is not None
    )


def _result_numbers(entry: Mapping[str, Any]) -> tuple[Any, ...]:
    arm_name = shipped_default_arm(entry)
    arm = entry["arms"][arm_name]
    dataset = entry["dataset"]
    return (
        _contract_key(entry),
        dataset["integrity"]["sha256"],
        dataset["evaluated_population"]["documents"],
        bool(entry.get("provisional")),
        arm_name,
        arm["failed_closed_documents"],
        arm["surviving_pii_utf8_bytes"],
        arm["false_positive_utf8_bytes"],
        arm["restore_exact_rate"],
        json.dumps(arm.get("gold_gap"), sort_keys=True),
    )


def release_groups(
    releases: Sequence[Mapping[str, Any]],
) -> list[list[Mapping[str, Any]]]:
    """Consecutive releases with an equal `result_key`, oldest first.

    Only neighbours merge: a release that returns to an older result after a
    different one starts a new group, so the display never hides a change.
    """
    groups: list[list[Mapping[str, Any]]] = []
    for entry in releases:
        if groups and result_key(groups[-1][-1]) == result_key(entry):
            groups[-1].append(entry)
        else:
            groups.append([entry])
    return groups


def displayed_groups(history: Mapping[str, Any]) -> list[list[Mapping[str, Any]]]:
    return release_groups(history["releases"])[-DISPLAYED_GROUPS:]


# --------------------------------------------------------------------------
# scored-label contracts per release
#
# A release row is measured under its own contract (absent = v1). The same
# release can also be scored under other contracts: each such run is one
# `contract_results` item carrying its own scorecard and arm numbers, measured
# on the same commit and corpus. `contract_view` presents either as a plain
# row, so every renderer below works on one contract at a time.
# --------------------------------------------------------------------------

#: The contract the document leads with: v2 scores the labels Gaze commits to
#: detect. v1 (every original corpus label) stays beside it, because releases
#: measured before v2 existed can only be compared under v1.
HEADLINE_CONTRACT = 2

CONTRACT_ROLES: dict[int, str] = {
    1: "all original gold labels, kept for comparison with earlier releases",
    2: "headline: the labels Gaze commits to detect",
}


def measured_contracts(entry: Mapping[str, Any]) -> list[int]:
    """Contract versions this row has numbers for, own contract first."""
    own = _contract_key(entry)[0]
    return [own] + [
        result["scored_label_contract"]["version"]
        for result in entry.get("contract_results", ())
    ]


def contract_view(entry: Mapping[str, Any], version: int) -> Mapping[str, Any] | None:
    """The row as measured under `version`, or None when it was not."""
    if _contract_key(entry)[0] == version:
        return entry
    for result in entry.get("contract_results", ()):
        if result["scored_label_contract"]["version"] == version:
            view = {
                key: value
                for key, value in entry.items()
                if key not in ("contract_results", "validator_recall")
            }
            view.update(
                scored_label_contract=result["scored_label_contract"],
                scorecard=result["scorecard"],
                scorecard_sha256=result["scorecard_sha256"],
                arms=result["arms"],
            )
            return view
    return None


def contract_order(versions: set[int] | Sequence[int]) -> list[int]:
    """Headline contract first, then the rest newest first."""
    return sorted(set(versions), key=lambda v: (v != HEADLINE_CONTRACT, -v))


def shown_contracts(history: Mapping[str, Any]) -> list[int]:
    """Contracts any displayed row was measured under, headline first."""
    return contract_order(
        {
            version
            for group in displayed_groups(history)
            for entry in group
            for version in measured_contracts(entry)
        }
    )


def contract_history(history: Mapping[str, Any], version: int) -> dict[str, Any]:
    """The displayed releases as measured under `version`; unmeasured rows drop out."""
    shown = [entry for group in displayed_groups(history) for entry in group]
    views = [view for view in (contract_view(e, version) for e in shown) if view]
    return {**history, "releases": views}


def contract_heading(version: int) -> str:
    role = CONTRACT_ROLES.get(version, "")
    return f"Scored labels v{version}" + (f" ({role})" if role else "")


def group_label(group: Sequence[Mapping[str, Any]]) -> str:
    """`v0.15.0` for one release, `v0.15.0 – v0.15.1` (oldest – newest) for more."""
    first, last = group[0]["version"], group[-1]["version"]
    return first if len(group) == 1 else f"{first} – {last}"


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


def _scored_label_contract(scorecard: Mapping[str, Any]) -> dict[str, Any] | None:
    """The scorecard's scored-label contract, or None for implicit contract v1."""
    scoring = scorecard.get("scoring")
    block = scoring.get("scored_label_contract") if isinstance(scoring, Mapping) else None
    if block is None:
        return None
    if not isinstance(block, Mapping):
        raise RenderError("scorecard scoring.scored_label_contract must be an object")
    if block.get("version") == 1:
        return None
    version = block.get("version")
    if type(version) is not int or version < 2:
        raise RenderError("scored-label contract version must be an integer >= 2")
    excluded = block.get("excluded_labels")
    if not isinstance(excluded, list) or not all(isinstance(x, str) for x in excluded):
        raise RenderError("scored-label contract excluded_labels must be strings")
    return {
        "id": str(block.get("id")),
        "version": version,
        "file_sha256": _require_hex64(
            block.get("file_sha256"), "scorecard scored_label_contract.file_sha256"
        ),
        "excluded_labels": list(excluded),
    }


def _contract_key(entry: Mapping[str, Any]) -> tuple[int, str | None]:
    """(version, file sha256); rows without a contract are implicit v1."""
    contract = entry.get("scored_label_contract")
    if not isinstance(contract, Mapping):
        return (1, None)
    return (contract["version"], contract["file_sha256"])


def contract_label(entry: Mapping[str, Any]) -> str:
    contract = entry.get("scored_label_contract")
    version = contract["version"] if isinstance(contract, Mapping) else 1
    return f"scored labels v{version}"


def _gold_gap_from_run(run: Mapping[str, Any], config: str) -> dict[str, Any] | None:
    """The contract v3 diagnostic for one arm, or None when the run has none."""
    metrics = run.get("metrics")
    block = metrics.get("gold_gap") if isinstance(metrics, Mapping) else None
    if block is None:
        return None
    where = f"run {config} metrics.gold_gap"
    if not isinstance(block, Mapping) or block.get("status") != "diagnostic":
        raise RenderError(f"{where} must be an object with status 'diagnostic'")
    by_label = block.get("gold_gap_protected_bytes_by_label")
    if not isinstance(by_label, Mapping):
        raise RenderError(f"{where}.gold_gap_protected_bytes_by_label must be an object")
    row = {
        "gold_gap_protected_bytes": _require_nonneg_int(
            block.get("gold_gap_protected_bytes"), f"{where}.gold_gap_protected_bytes"
        ),
        "false_positive_bytes_after_gold_gap": _require_nonneg_int(
            block.get("false_positive_bytes_after_gold_gap"),
            f"{where}.false_positive_bytes_after_gold_gap",
        ),
        "adjusted_precision": _dig(block, ("adjusted_precision",), where),
        "gold_gap_protected_bytes_by_label": {
            str(label): _require_nonneg_int(value, f"{where}.{label}")
            for label, value in sorted(by_label.items())
        },
    }
    if sum(row["gold_gap_protected_bytes_by_label"].values()) != row[
        "gold_gap_protected_bytes"
    ]:
        raise RenderError(f"{where}: per-label bytes do not sum to the total")
    return row


def render_gold_gap(entry: Mapping[str, Any]) -> list[str]:
    arms = {arm: block["gold_gap"] for arm, block in entry["arms"].items() if "gold_gap" in block}
    if not arms:
        return []
    lines = [
        "",
        "Gold-gap protection (contract v3, **diagnostic; v2 headline unchanged**): "
        "false-positive bytes that are an unlabelled, byte-identical repeat of a "
        "gold value in the same document. The columns above are the headline.",
        "",
        "| Arm | " + " | ".join(column[0] for column in GOLD_GAP_COLUMNS) + " |",
        "| --- | " + " | ".join(["---:"] * len(GOLD_GAP_COLUMNS)) + " |",
    ]
    for arm, block in arms.items():
        cells = [_fmt(kind, block[field]) for _, field, kind in GOLD_GAP_COLUMNS]
        lines.append("| " + " | ".join([f"`{arm}`"] + cells) + " |")
    return lines


def _validator_row_from_scorecard(block: Mapping[str, Any], where: str) -> dict[str, Any]:
    split = _dig(block, ("production_recall_by_gold_validity",), where)
    return {
        "validator_kinds": list(_dig(block, ("validator_kinds",), where)),
        "gold_spans": _dig(block, ("gold_spans",), where),
        "validator_failed_gold_spans": _dig(
            block, ("validator_failed_gold_spans",), where
        ),
        "validator_backed_full_coverage_recall": _dig(
            block, ("validator_backed_recall", "full_coverage_recall"), where
        ),
        "shape_only_full_coverage_recall": _dig(
            block, ("shape_only_recall", "full_coverage_recall"), where
        ),
        "leaked_utf8_bytes_validator_passed_gold": _dig(
            split, ("validator_passed_gold", "leaked_utf8_bytes"), where
        ),
        "leaked_utf8_bytes_validator_failed_gold": _dig(
            split, ("validator_failed_gold", "leaked_utf8_bytes"), where
        ),
    }


def validator_recall_from_run(run: Mapping[str, Any]) -> dict[str, Any] | None:
    """Applicable validator labels of one run, or None for pre-split scorecards.

    Scorecards measured before the gold-validity split carry no
    `production_recall_by_gold_validity`; their rows render exactly as before.
    """
    labels = run.get("validator_recall_by_label")
    if not isinstance(labels, Mapping):
        return None
    applicable = {
        label: block
        for label, block in labels.items()
        if isinstance(block, Mapping) and block.get("applicability") == "applicable"
    }
    if not any("production_recall_by_gold_validity" in b for b in applicable.values()):
        return None
    return {
        label: _validator_row_from_scorecard(
            block, f"run {run.get('config')} validator_recall_by_label.{label}"
        )
        for label, block in sorted(applicable.items())
    }


def history_entry_from_scorecard(
    scorecard: Mapping[str, Any],
    *,
    version: str,
    machine: str,
    scorecard_filename: str,
    scorecard_sha256: str,
    provisional: bool = False,
    note: str = "",
    shipped_arm: str = SHIPPED_DEFAULT_ARM,
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
        gold_gap = _gold_gap_from_run(run, config)
        if gold_gap is not None:
            arms[config]["gold_gap"] = gold_gap

    integrity = dataset.get("integrity")
    if not isinstance(integrity, Mapping):
        raise RenderError("scorecard dataset.integrity must be an object")
    corpus_sha256 = _require_hex64(
        integrity.get("sha256"), "scorecard dataset.integrity.sha256"
    )
    component_sha256 = _require_component_digests(integrity, "scorecard")

    population = dataset.get("evaluated_population")
    if not isinstance(population, Mapping):
        raise RenderError(
            "scorecard dataset.evaluated_population must be an object; the "
            "document has to state the population its numbers describe"
        )
    counts = {
        key: _require_nonneg_int(
            population.get(key), f"scorecard dataset.evaluated_population.{key}"
        )
        for key in ("documents", "entities")
    }

    model_bundles = _require_model_bundles(provenance, "scorecard runner_provenance")
    if shipped_arm not in arms:
        raise RenderError(
            f"scorecard has no run for the shipped default arm {shipped_arm}"
        )
    contract = _scored_label_contract(scorecard)
    has_gold_gap = any("gold_gap" in block for block in arms.values())
    if has_gold_gap != (contract is not None and contract["version"] >= 3):
        raise RenderError(
            "a gold_gap diagnostic belongs to scored-label contract v3 and only there"
        )
    default_run = next(run for run in runs if run["config"] == shipped_arm)
    validator_recall = validator_recall_from_run(default_run)

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
            "integrity": {
                "sha256": corpus_sha256,
                "component_sha256": component_sha256,
            },
            "evaluated_population": counts,
        },
        "provenance": {"model_bundles": model_bundles},
        "parameters": {
            "profile": parameters.get("profile"),
            "sampling_seed": parameters.get("sampling_seed"),
            "ner_threshold": parameters.get("ner_threshold"),
        },
        "provisional": bool(provisional),
        "note": note,
        # Absent means contract v1 (every corpus label scored), which keeps
        # the rows recorded before contracts existed byte-identical.
        **({"scored_label_contract": contract} if contract is not None else {}),
        "shipped_default_arm": shipped_arm,
        "arms": arms,
        # Absent on rows measured before the gold-validity split, so those rows
        # and the document they render stay byte-identical.
        **(
            {"validator_recall": validator_recall}
            if validator_recall is not None
            else {}
        ),
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
    if entry.get("scored_label_contract"):
        contract = entry["scored_label_contract"]
        excluded = ", ".join(contract["excluded_labels"]) or "none"
        lines.append(
            f"Measured under **{contract_label(entry)}** "
            f"(`{contract['id']}`; out of contract: {excluded})."
        )
        lines.append("")
    if entry.get("note"):
        lines.append(f"> {entry['note']}")
        lines.append("")

    dataset = entry.get("dataset") or {}
    integrity = dataset.get("integrity") or {}
    population = dataset.get("evaluated_population") or {}
    parameters = entry.get("parameters") or {}
    # Read every provenance cell through the guard that validates it, so a
    # fallback reintroduced here changes behaviour the tests can see rather
    # than silently publishing whatever the history file happens to hold.
    bundles = _require_model_bundles(entry.get("provenance") or {}, "history")
    scorecard_sha256 = _require_hex64(
        entry.get("scorecard_sha256"), "history scorecard_sha256"
    )
    corpus_sha256 = _require_hex64(
        integrity.get("sha256"), "history dataset.integrity.sha256"
    )
    components = _require_component_digests(integrity, "history")
    documents = _require_nonneg_int(
        population.get("documents"), "history dataset.evaluated_population.documents"
    )
    entities = _require_nonneg_int(
        population.get("entities"), "history dataset.evaluated_population.entities"
    )
    lines.extend(
        [
            "| Provenance | Value |",
            "| --- | --- |",
            f"| Release | `{entry['version']}` |",
            f"| Commit | `{entry['commit']}` |",
            f"| Measured | {entry['date']} |",
            f"| Machine | {entry['machine']} |",
            f"| Harness | [`{entry['harness_entry']}`]"
            f"(../../../{entry['harness_entry']}) |",
            f"| Scorecard | [`{entry['scorecard']}`]({entry['scorecard']}) |",
            f"| Scorecard sha256 | `{scorecard_sha256}` |",
            f"| Corpus | `{dataset.get('repository')}` @ `{dataset.get('revision')}` |",
            f"| Corpus sha256 | `{corpus_sha256}` |",
        ]
    )
    for component, digest in components.items():
        lines.append(f"| Corpus component `{component}` | `{digest}` |")
    lines.extend(
        [
            f"| Population | {documents:,} documents / {entities:,} entities |",
            f"| Profile | `{parameters.get('profile')}` |",
            f"| Seed | `{parameters.get('sampling_seed')}` |",
            f"| NER threshold | `{parameters.get('ner_threshold')}` |",
        ]
    )
    if bundles:
        for bundle in bundles:
            lines.append(
                f"| Model bundle `{bundle['model_id']}` | "
                f"`{bundle['expected_sha256']}` |"
            )
    else:
        lines.append("| Model bundles | *none — no neural backend in this run* |")
    for result in entry.get("contract_results", ()):
        label = f"scored labels v{result['scored_label_contract']['version']}"
        lines.append(
            f"| Scorecard, {label} | [`{result['scorecard']}`]({result['scorecard']}) |"
        )
        lines.append(
            f"| Scorecard sha256, {label} | "
            f"`{_require_hex64(result['scorecard_sha256'], 'history contract result')}` |"
        )
    lines.append("")

    versions = contract_order(measured_contracts(entry))
    for version in versions:
        view = contract_view(entry, version)
        if len(versions) > 1:
            gold = view["arms"][shipped_default_arm(view)]["gold_pii_utf8_bytes"]
            lines.append(
                f"**{contract_heading(version)}.** Gold PII bytes: {_fmt('int', gold)}."
            )
            lines.append("")
        lines.extend(_arm_table(view))
        lines.extend(render_gold_gap(view))
        lines.append("")
    lines.pop()
    if entry.get("validator_recall"):
        lines.extend(render_validator_recall(entry))
    return "\n".join(lines)


def _arm_table(entry: Mapping[str, Any]) -> list[str]:
    headers = ["Arm info"] + [column[0] for column in ARM_COLUMNS]
    lines = [
        "| " + " | ".join(headers) + " |",
        "| --- | " + " | ".join(["---:"] * len(ARM_COLUMNS)) + " |",
    ]
    for arm, block in entry["arms"].items():
        cells = [_fmt(kind, block[field]) for _, field, kind in ARM_COLUMNS]
        label = f"`{arm}`"
        if arm == shipped_default_arm(entry):
            label += " **(shipped default)**"
        lines.append("| " + " | ".join([label] + cells) + " |")
    return lines


def render_validator_recall(entry: Mapping[str, Any]) -> list[str]:
    lines = [
        "",
        f"Validator-backed labels on `{shipped_default_arm(entry)}`, "
        f"{contract_label(entry)}. Gold that "
        "fails its own checksum stays scored gold: the two leaked-bytes columns "
        "split the surviving bytes above, they do not replace them. Shape recall "
        "is what a shape-only match (validator ignored) would cover.",
        "",
        "| Label | Validator | " + " | ".join(c[0] for c in VALIDATOR_COLUMNS) + " |",
        "| --- | --- | " + " | ".join(["---:"] * len(VALIDATOR_COLUMNS)) + " |",
    ]
    for label, row in entry["validator_recall"].items():
        cells = [_fmt(kind, row[field]) for _, field, kind in VALIDATOR_COLUMNS]
        kinds = ", ".join(row["validator_kinds"])
        lines.append("| " + " | ".join([f"`{label}`", kinds] + cells) + " |")
    return lines


def comparison_bars(history: Mapping[str, Any]) -> list[tuple[str, int]]:
    """(label, leaked bytes) for the latest result group against the one before it.

    Each label ends with that arm's leak rate, e.g. `v0.15.0 default (15.0%)`.

    The latest group's default comes first, then the previous group's default,
    then every other measured arm by leaked bytes. A group is shown by its
    newest release. The previous group always has different results
    (`release_groups`), so an unchanged patch release never compares with
    itself. It joins only when it was scored on the same corpus under the same
    label contract; otherwise a contract or corpus change would read as a leak
    change.
    """
    groups = release_groups(history["releases"])
    latest = groups[-1]
    rows = [latest]
    if len(groups) > 1:
        previous = groups[-2]
        if _contract_key(previous[-1]) == _contract_key(latest[-1]) and (
            previous[-1]["dataset"]["integrity"]["sha256"]
            == latest[-1]["dataset"]["integrity"]["sha256"]
        ):
            rows.append(previous)
    bars: list[tuple[str, int]] = []
    for group in rows:
        row, label = group[-1], group_label(group)
        default_arm = shipped_default_arm(row)
        others = sorted(
            (arm for arm in row["arms"] if arm != default_arm),
            key=lambda arm: row["arms"][arm]["surviving_pii_utf8_bytes"],
        )
        bars.append(
            (
                _leak_pct_label(f"{label} default", row["arms"][default_arm]),
                row["arms"][default_arm]["surviving_pii_utf8_bytes"],
            )
        )
        for arm in others:
            bars.append(
                (
                    _leak_pct_label(
                        f"{label} {ARM_CHART_LABELS.get(arm, arm)}",
                        row["arms"][arm],
                    ),
                    row["arms"][arm]["surviving_pii_utf8_bytes"],
                )
            )
    return bars


def _leak_rate_caption(entry: Mapping[str, Any]) -> str:
    """Explain the axis-label percentage once, next to the chart it labels."""
    gold = entry["arms"][shipped_default_arm(entry)]["gold_pii_utf8_bytes"]
    return (
        "The percentage in each label is the leak rate: leaked bytes out of "
        f"{_fmt('int', gold)} gold PII bytes."
    )


def _comparison_chart(history: Mapping[str, Any]) -> list[str]:
    bars = comparison_bars(history)
    latest = history["releases"][-1]
    values = [value for _, value in bars]
    return [
        "```mermaid",
        # Horizontal: the labels carry the leak rate and outgrow a vertical
        # bar's slot at GitHub's ~800 px content width.
        "xychart-beta horizontal",
        f'    title "Leaked PII bytes, {contract_label(latest)} - lower is better"',
        f"    x-axis {_mermaid_labels([label for label, _ in bars])}",
        f'    y-axis "Leaked PII bytes" 0 --> {_axis_max(values)}',
        f"    bar [{', '.join(str(int(value)) for value in values)}]",
        "```",
    ]


def shipped_default_trend(
    history: Mapping[str, Any], field: str
) -> list[tuple[str, Any]]:
    """(group label, value) of each displayed group's OWN shipped default arm.

    Keyed per row, not by the latest default: the default changed between
    releases, and a row that never measured today's default arm still shipped
    one. A group reads its newest release. Only groups under the latest
    group's label contract are kept, because one line across two contracts
    would show a change in what counts as gold as a change in leaks.
    """
    groups = displayed_groups(history)
    latest_contract = _contract_key(groups[-1][-1])
    return [
        (group_label(group), group[-1]["arms"][shipped_default_arm(group[-1])][field])
        for group in groups
        if _contract_key(group[-1]) == latest_contract
    ]


#: Trend charts: (history field, chart title, y-axis title, leak rate in the
#: x-axis labels). The leaked line carries each release's leak rate; the
#: false-positive line stays in bytes.
TREND_CHARTS: tuple[tuple[str, str, str, bool], ...] = (
    (
        "surviving_pii_utf8_bytes",
        "Leaked PII bytes, shipped default",
        "Leaked PII bytes (lower is better)",
        True,
    ),
    (
        "false_positive_utf8_bytes",
        "False-positive bytes, shipped default",
        "False-positive bytes (lower is less over-redaction)",
        False,
    ),
)


def shipped_default_trend_labels(
    history: Mapping[str, Any], with_leak_rate: bool
) -> list[str]:
    """x-axis labels for the trend charts, aligned with `shipped_default_trend`."""
    return [
        _leak_pct_label(version, {"leak_rate": rate}) if with_leak_rate else version
        for version, rate in shipped_default_trend(history, "leak_rate")
    ]


def _unmeasured_note(history: Mapping[str, Any], version: int) -> list[str]:
    missing = [
        group_label(group)
        for group in displayed_groups(history)
        if contract_view(group[-1], version) is None
    ]
    if not missing:
        return []
    return [
        "",
        f"> Not measured under scored labels v{version}: {', '.join(missing)}. "
        "Those releases are compared under the other contract.",
    ]


def render_charts(history: Mapping[str, Any]) -> str:
    if not history["releases"]:
        return (
            "> Charts render once at least one release row exists in "
            "[`release-history.json`](release-history.json)."
        )
    versions = shown_contracts(history)
    if len(versions) == 1:
        return _render_contract_charts(history)
    sections = []
    for version in versions:
        sections.append(
            "\n".join(
                [
                    f"#### {contract_heading(version)}",
                    "",
                    _render_contract_charts(contract_history(history, version)),
                    *_unmeasured_note(history, version),
                ]
            )
        )
    return "\n\n".join(sections)


def _render_contract_charts(history: Mapping[str, Any]) -> str:
    releases = history["releases"]
    entry = releases[-1]
    groups = displayed_groups(history)
    lines = [
        f"**Leaked PII bytes — {group_label(groups[-1])} against the previous "
        "release with different results.** "
        f"Lower is better; the goal is zero. Scored under {contract_label(entry)}; "
        "every bar is a measured arm in "
        "[`release-history.json`](release-history.json). "
        f"{_leak_rate_caption(entry)}",
        "",
        *_comparison_chart(history),
    ]

    trend_rows = shipped_default_trend(history, TREND_CHARTS[0][0])
    lines.extend(
        [
            "",
            "**Trend across releases — each release's shipped default.** "
            f"Scored under {contract_label(entry)}. The shipped arm changes "
            "between releases; the history table names it per row.",
        ]
    )
    # Sections split by contract version; a different contract file with the
    # same version is still another contract, so the line leaves it out.
    if len(trend_rows) < len(groups):
        lines.extend(
            [
                "",
                f"> Only rows measured under {contract_label(entry)} are on "
                "these lines; "
                f"{len(groups) - len(trend_rows)} row(s) under another contract are "
                "in the history table.",
            ]
        )
    if len(trend_rows) < 2:
        lines.extend(
            [
                "",
                f"> One measured release so far ({len(trend_rows)} point). The trend "
                "charts render from two releases onward.",
            ]
        )
        return "\n".join(lines)
    for field, title, axis, with_leak_rate in TREND_CHARTS:
        trend = shipped_default_trend(history, field)
        labels = shipped_default_trend_labels(history, with_leak_rate)
        lines.extend(
            [
                "",
                "```mermaid",
                "xychart-beta",
                f'    title "{title} - {contract_label(entry)}"',
                f"    x-axis {_mermaid_labels(labels)}",
                f'    y-axis "{axis}" 0 --> '
                f"{_axis_max([value for _, value in trend])}",
                f"    line [{', '.join(str(int(value)) for _, value in trend)}]",
                "```",
            ]
        )
    return "\n".join(lines)


def render_readme_chart(history: Mapping[str, Any]) -> str:
    if not history["releases"]:
        return "> The chart renders once a release has been measured."
    versions = shown_contracts(history)
    if len(versions) == 1:
        return _readme_contract_chart(history)
    return "\n\n".join(
        _readme_contract_chart(contract_history(history, version))
        for version in versions
    )


def _readme_contract_chart(history: Mapping[str, Any]) -> str:
    releases = history["releases"]
    entry = releases[-1]
    return "\n".join(
        [
            f"Leaked PII bytes per setup, {contract_label(entry)}, lower is better "
            "(generated from "
            "[`release-history.json`](docs/reference/benchmarks/release-history.json)). "
            f"{_leak_rate_caption(entry)}",
            "",
            *_comparison_chart(history),
        ]
    )


def render_history(history: Mapping[str, Any]) -> str:
    releases = history["releases"]
    if not releases:
        return (
            "| Release | Measured | Commit | Machine | Scorecard | "
            "Surviving PII bytes ↓ |\n"
            "| --- | --- | --- | --- | --- | ---: |\n"
            "| *none yet* | — | — | — | — | — |"
        )
    # Rows that record their own shipped arm were appended with the refusal-aware
    # layout. A history of legacy rows alone keeps the original table byte for byte.
    groups = displayed_groups(history)
    versions = shown_contracts(history)
    if len(versions) > 1:
        return render_history_by_contract(groups, versions)
    if any("shipped_default_arm" in entry for entry in releases):
        return render_history_with_refusals(groups)
    latest_default_arm = shipped_default_arm(releases[-1])
    lines = [
        "| Release | Measured | Commit | Machine | Scorecard | "
        "Surviving PII bytes ↓ |",
        "| --- | --- | --- | --- | --- | ---: |",
    ]
    for group in groups:
        entry = group[-1]
        # Each row reports the arm it shipped; name it when that differs from
        # the latest default so a changed default never reads as a leak change.
        default_arm = shipped_default_arm(entry)
        surviving = _fmt("int", entry["arms"][default_arm]["surviving_pii_utf8_bytes"])
        if default_arm != latest_default_arm:
            surviving += f" (`{default_arm}`)"
        lines.append(
            f"| {_history_version_cell(group)} | {entry['date']} | `{entry['commit'][:7]}` | "
            f"{entry['machine']} | {_scorecard_links(group)} | "
            f"{surviving} |"
        )
    return "\n".join(lines)


def _history_version_cell(group: Sequence[Mapping[str, Any]]) -> str:
    """The group label; every member shares the flags, since both are in the key."""
    entry = group[-1]
    version = group_label(group)
    if entry.get("provisional"):
        version += " *(provisional)*"
    if entry.get("scored_label_contract"):
        version += f" · {contract_label(entry)}"
    return version


def _scorecard_links(group: Sequence[Mapping[str, Any]]) -> str:
    """Every member's scorecard: the merged row stands on all of them."""
    return ", ".join(
        f"[`{entry['scorecard']}`]({entry['scorecard']})" for entry in group
    )


def common_set_surviving_bytes(releases: Sequence[Mapping[str, Any]]) -> list[int]:
    """Each row's surviving bytes on the documents every row's shipped arm processed.

    Aggregate scorecards carry no per-document leak bytes, so the common set is
    computable only when it is the whole population: every row evaluated the
    same corpus and population and no shipped arm refused a document. Anything
    else is refused rather than approximated, because a refusing setup looks
    better on a table that silently drops the documents it refused.
    """
    corpora = {
        (
            entry["dataset"]["integrity"]["sha256"],
            entry["dataset"]["evaluated_population"]["documents"],
        )
        for entry in releases
    }
    if len(corpora) != 1:
        raise RenderError(
            "release rows measured different corpora or populations; the "
            "common-document-set column needs one shared population"
        )
    refused = [
        entry["version"]
        for entry in releases
        if entry["arms"][shipped_default_arm(entry)]["failed_closed_documents"]
    ]
    if refused:
        raise RenderError(
            f"{', '.join(refused)}: the shipped arm refused documents, so the "
            "common-document-set leak needs per-document data this history lacks"
        )
    return [
        entry["arms"][shipped_default_arm(entry)]["surviving_pii_utf8_bytes"]
        for entry in releases
    ]


def render_history_with_refusals(groups: Sequence[Sequence[Mapping[str, Any]]]) -> str:
    common = common_set_surviving_bytes([group[-1] for group in groups])
    lines = [
        "| Release | Measured | Commit | Machine | Scorecard | Shipped arm | "
        "Refused ↓ | Leaked PII bytes, all processed ↓ | "
        "Leaked PII bytes, common documents ↓ | False-positive bytes ↔ | "
        "Restore exact ↑ | clean p95 ms ↓ |",
        "| --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for group, common_bytes in zip(groups, common):
        entry = group[-1]
        default_arm = shipped_default_arm(entry)
        arm = entry["arms"][default_arm]
        lines.append(
            f"| {_history_version_cell(group)} | {entry['date']} | `{entry['commit'][:7]}` | "
            f"{entry['machine']} | {_scorecard_links(group)} | "
            f"`{default_arm}` | {_fmt('int', arm['failed_closed_documents'])} | "
            f"{_fmt('int', arm['surviving_pii_utf8_bytes'])} | {_fmt('int', common_bytes)} | "
            f"{_fmt('int', arm['false_positive_utf8_bytes'])} | "
            f"{_fmt('pct', arm['restore_exact_rate'])} | {_fmt('ms', arm['clean_ms_p95'])} |"
        )
    return "\n".join(lines)


def render_history_by_contract(
    groups: Sequence[Sequence[Mapping[str, Any]]], versions: Sequence[int]
) -> str:
    """One row per group; leak and false-positive columns per contract, headline first.

    Refused, restore exact and latency do not depend on the contract, so they
    are shown once. A group not measured under a contract says so instead of
    borrowing the other contract's numbers.
    """
    headers = ["Release", "Measured", "Commit", "Machine", "Scorecards", "Shipped arm", "Refused ↓"]
    for version in versions:
        headers += [
            f"Leaked PII bytes, all processed, v{version} ↓",
            f"Leaked PII bytes, common documents, v{version} ↓",
            f"False-positive bytes, v{version} ↔",
        ]
    headers += ["Restore exact ↑", "clean p95 ms ↓"]
    cells_by_version: dict[int, list[list[str]]] = {}
    for version in versions:
        views = [contract_view(group[-1], version) for group in groups]
        measured = [view for view in views if view is not None]
        common = iter(common_set_surviving_bytes(measured)) if measured else iter(())
        column: list[list[str]] = []
        for view in views:
            if view is None:
                column.append(["*not measured*"] * 3)
                continue
            arm = view["arms"][shipped_default_arm(view)]
            column.append(
                [
                    _fmt("int", arm["surviving_pii_utf8_bytes"]),
                    _fmt("int", next(common)),
                    _fmt("int", arm["false_positive_utf8_bytes"]),
                ]
            )
        cells_by_version[version] = column
    lines = [
        "| " + " | ".join(headers) + " |",
        "| " + " | ".join(["---"] * 6 + ["---:"] * (len(headers) - 6)) + " |",
    ]
    for index, group in enumerate(groups):
        entry = group[-1]
        default_arm = shipped_default_arm(entry)
        arm = entry["arms"][default_arm]
        links = [
            f"[`{name}`]({name})"
            for member in group
            for name in [member["scorecard"]]
            + [result["scorecard"] for result in member.get("contract_results", ())]
        ]
        row = [
            _history_version_cell(group),
            entry["date"],
            f"`{entry['commit'][:7]}`",
            entry["machine"],
            ", ".join(links),
            f"`{default_arm}`",
            _fmt("int", arm["failed_closed_documents"]),
        ]
        for version in versions:
            row += cells_by_version[version][index]
        row += [_fmt("pct", arm["restore_exact_rate"]), _fmt("ms", arm["clean_ms_p95"])]
        lines.append("| " + " | ".join(row) + " |")
    return "\n".join(lines)


# --------------------------------------------------------------------------
# latency
#
# `cli-latency.py` writes one `latency-vX.Y.Z.json` per release on a quiet
# host. It is separate evidence from the scorecard, whose `clean p95 ms` runs
# on a loaded host, and it never feeds `result_key`: latency is host noise
# for the question "did the results change".
# --------------------------------------------------------------------------

#: (label, pipeline and CLI key suffix) for the two setups the file measures.
LATENCY_SETUPS: tuple[tuple[str, str], ...] = (
    ("`gaze setup` without Nym (rules + NER)", "setup"),
    ("`gaze setup` (rules + NER + Nym)", "setup_nym"),
)


def load_latency(directory: Path, history: Mapping[str, Any]) -> dict[str, Any]:
    """Every committed `latency-<version>.json`; a release without one is absent."""
    loaded: dict[str, Any] = {}
    for entry in history["releases"]:
        path = directory / f"latency-{entry['version']}.json"
        if not path.exists():
            continue
        try:
            loaded[entry["version"]] = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as error:
            raise RenderError(f"{path.name} is not valid JSON: {error}") from error
    return loaded


def _latency_number(data: Mapping[str, Any], path: Sequence[str], where: str) -> float:
    value = _dig(data, path, where)
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value < 0:
        raise RenderError(
            f"{where}: {'.'.join(path)} must be a non-negative number, got {value!r}"
        )
    return float(value)


def _latency_rows(
    label: str, data: Mapping[str, Any] | None, version: str
) -> tuple[list[str], list[str], str | None]:
    """(pipeline rows, CLI rows, provenance line) for one displayed group."""
    if data is None:
        missing = "not measured"
        return (
            [f"| {label} | {missing} | — | — | — | — |"],
            [f"| {label} | {missing} | — | — | — | — |"],
            None,
        )
    where = f"latency-{version}.json"
    if data.get("smoke") is not False:
        raise RenderError(
            f"{where}: a smoke run (or one without `smoke: false`) is never a "
            "timing claim"
        )
    verdict = data.get("verdict")
    hardware = data.get("hardware")
    if not isinstance(verdict, str) or not isinstance(hardware, str):
        raise RenderError(f"{where}: verdict and hardware must be strings")
    def number(*path: str) -> float:
        return _latency_number(data, path, where)

    pipeline, cli = [], []
    for setup_label, key in LATENCY_SETUPS:
        pipeline.append(
            f"| {label} | {setup_label} | "
            f"{_fmt('ms', number('pipeline', key, 'warm_clean', 'p50_ms'))} | "
            f"{_fmt('ms', number('pipeline', key, 'warm_clean', 'p95_ms'))} | "
            f"{_fmt('ms', number('pipeline', key, 'cold_first_document_ms'))} | "
            f"{number('pipeline', key, 'peak_rss_mib'):.1f} |"
        )
        cli.append(
            f"| {label} | {setup_label} | "
            f"{_fmt('ms', number('cli', f'oneshot_{key}', 'per_document', 'p50_ms'))} | "
            f"{_fmt('ms', number('cli', f'oneshot_{key}', 'per_document', 'p95_ms'))} | "
            f"{_fmt('ms', number('cli', f'daemon_{key}', 'warm', 'p50_ms'))} | "
            f"{_fmt('ms', number('cli', f'daemon_{key}', 'warm', 'p95_ms'))} |"
        )
    documents = _require_nonneg_int(data.get("documents"), f"{where} documents")
    load = _latency_number(data, ("host_before", "load_1m"), where)
    note = (
        f"- **{label}:** [`{where}`]({where}), verdict `{verdict}`, "
        f"{documents} documents, 1-minute load {load:.2f} at start. Host: {hardware}."
    )
    return pipeline, cli, note


def render_latency(
    history: Mapping[str, Any], latency: Mapping[str, Any] | None = None
) -> str:
    """Quiet-host latency per displayed group, read from its newest release's file."""
    if not history["releases"]:
        return "> Latency renders once a release has been measured."
    latency = latency or {}
    pipeline = [
        "| Release | Setup | Warm p50 ms ↓ | Warm p95 ms ↓ | "
        "Cold first document ms ↓ | Peak RSS MiB ↓ |",
        "| --- | --- | ---: | ---: | ---: | ---: |",
    ]
    cli = [
        "| Release | Setup | One-shot p50 ms ↓ | One-shot p95 ms ↓ | "
        "Daemon warm p50 ms ↓ | Daemon warm p95 ms ↓ |",
        "| --- | --- | ---: | ---: | ---: | ---: |",
    ]
    notes = []
    for group in displayed_groups(history):
        version = group[-1]["version"]
        rows = _latency_rows(group_label(group), latency.get(version), version)
        pipeline.extend(rows[0])
        cli.extend(rows[1])
        if rows[2]:
            notes.append(rows[2])
    return "\n".join(
        [
            "**In-process pipeline.** Warm is the per-document `clean` time once "
            "models are loaded; cold is the first document, model load included.",
            "",
            *pipeline,
            "",
            "**CLI.** One-shot starts `gaze clean` per document; the daemon "
            "(`gaze daemon`) loads once and serves every document after the first.",
            "",
            *cli,
            "",
            *(notes or ["No release in this table has a latency file yet."]),
        ]
    )


RENDERERS = {
    "current-release": render_current_release,
    "charts": render_charts,
    "history": render_history,
    "readme-chart": render_readme_chart,
    "latency": render_latency,
}


def begin_marker(name: str) -> str:
    return f"<!-- BEGIN GENERATED: {name} -->"


def end_marker(name: str) -> str:
    return f"<!-- END GENERATED: {name} -->"


def apply_blocks(
    document: str,
    history: Mapping[str, Any],
    names: Sequence[str] = BLOCK_NAMES,
    latency: Mapping[str, Any] | None = None,
) -> str:
    """Replace each generated block in place, leaving all prose untouched.

    `latency` maps a version to its parsed latency file; only the latency
    block reads it, and a version without one renders as not measured.
    """
    for name in names:
        begin, end = begin_marker(name), end_marker(name)
        start = document.find(begin)
        stop = document.find(end)
        if start < 0 or stop < 0:
            raise RenderError(f"document is missing the {name!r} generated block")
        if stop < start:
            raise RenderError(f"{name!r} markers are out of order")
        body = (
            render_latency(history, latency)
            if name == "latency"
            else RENDERERS[name](history)
        )
        document = (
            document[: start + len(begin)] + "\n\n" + body + "\n\n" + document[stop:]
        )
    return document


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def _display(path: Path) -> str:
    """Repo-relative when possible: both targets are named README.md."""
    try:
        return str(path.resolve().relative_to(REPO_ROOT))
    except ValueError:
        return path.name


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--doc", type=Path, default=DEFAULT_DOC)
    parser.add_argument("--history", type=Path, default=DEFAULT_HISTORY)
    parser.add_argument(
        "--readme",
        type=Path,
        help=(
            "root README carrying the readme-chart block. Defaults to the repo "
            "README when --history is the committed history, since that is the "
            "history the README's numbers come from; otherwise no README is touched."
        ),
    )
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
    parser.add_argument(
        "--append-contract-result",
        action="store_true",
        help=(
            "add --scorecard, the same release re-scored under another "
            "scored-label contract, to the existing --version row"
        ),
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
    parser.add_argument(
        "--shipped-default-arm",
        default=SHIPPED_DEFAULT_ARM,
        help=(
            "scorecard config the release shipped as its default; a release "
            "benchmarked through --policy records `policy-file`"
        ),
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        history = load_history(args.history)

        if args.append_contract_result:
            if args.check or args.append_history:
                raise RenderError(
                    "--append-contract-result excludes --check and --append-history"
                )
            if not args.scorecard or not args.release_version:
                raise RenderError("--append-contract-result requires --scorecard and --version")
            rows = [e for e in history["releases"] if e["version"] == args.release_version]
            if not rows:
                raise RenderError(f"no history row for {args.release_version}")
            scorecard = json.loads(args.scorecard.read_text(encoding="utf-8"))
            contract = _scored_label_contract(scorecard)
            if contract is None:
                raise RenderError("the scorecard is scored under contract v1")
            expected = contract_scorecard_name(args.release_version, contract["version"])
            if args.scorecard.name != expected:
                raise RenderError(
                    f"scorecard must be named {expected}, got {args.scorecard.name}"
                )
            rows[0].setdefault("contract_results", []).append(
                contract_result_from_scorecard(
                    scorecard,
                    rows[0],
                    scorecard_filename=expected,
                    scorecard_sha256=_sha256(args.scorecard),
                )
            )
            validate_history(history)
            write_history(args.history, history)

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
                shipped_arm=args.shipped_default_arm,
            )
            history["releases"].append(entry)
            history["releases"].sort(key=lambda item: version_sort_key(item["version"]))
            validate_history(history)
            write_history(args.history, history)

        for entry in history["releases"]:
            names = [entry["scorecard"]] + [
                result["scorecard"] for result in entry.get("contract_results", ())
            ]
            for name in names:
                if not (args.history.parent / name).exists():
                    raise RenderError(
                        f"{entry['version']} names {name}, which is not "
                        "committed; the machine-readable evidence must stay in the tree"
                    )

        readme = args.readme
        if readme is None and args.history.resolve() == DEFAULT_HISTORY.resolve():
            readme = DEFAULT_README
        targets = [(args.doc, BLOCK_NAMES)]
        if readme is not None:
            targets.append((readme, README_BLOCK_NAMES))
        latency = load_latency(args.history.parent, history)
        outputs = []
        for path, names in targets:
            original = path.read_text(encoding="utf-8")
            outputs.append(
                (path, original, apply_blocks(original, history, names, latency))
            )

        if args.check:
            drifted = [path for path, original, rendered in outputs if rendered != original]
            for path in drifted:
                sys.stderr.write(
                    f"{path} is out of sync with {args.history}.\n"
                    "Re-run scripts/bench/render_benchmark_doc.py and commit the result.\n"
                )
            if drifted:
                return 1
            for path, _, _ in outputs:
                print(f"render_benchmark_doc: {_display(path)} is in sync")
            return 0

        for path, _, rendered in outputs:
            path.write_text(rendered, encoding="utf-8")
        print(
            "render_benchmark_doc: wrote "
            f"{', '.join(_display(path) for path, _, _ in outputs)} "
            f"({len(history['releases'])} release rows)"
        )
        return 0
    except RenderError as error:
        sys.stderr.write(f"render_benchmark_doc: {error}\n")
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
