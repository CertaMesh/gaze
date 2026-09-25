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

BLOCK_NAMES = ("current-release", "charts", "history")


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
    lines.append("")

    headers = ["Arm info"] + [column[0] for column in ARM_COLUMNS]
    lines.append("| " + " | ".join(headers) + " |")
    lines.append("| --- | " + " | ".join(["---:"] * len(ARM_COLUMNS)) + " |")
    for arm, block in entry["arms"].items():
        cells = [_fmt(kind, block[field]) for _, field, kind in ARM_COLUMNS]
        label = f"`{arm}`"
        if arm == shipped_default_arm(entry):
            label += " **(shipped default)**"
        lines.append("| " + " | ".join([label] + cells) + " |")
    lines.extend(render_gold_gap(entry))
    if entry.get("validator_recall"):
        lines.extend(render_validator_recall(entry))
    return "\n".join(lines)


def render_validator_recall(entry: Mapping[str, Any]) -> list[str]:
    lines = [
        "",
        f"Validator-backed labels on `{shipped_default_arm(entry)}`. Gold that "
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

    default_arm = shipped_default_arm(entry)
    # One line across two contracts would show a change in what counts as gold
    # as a change in leaks, so the trend keeps only the latest row's contract.
    latest_contract = _contract_key(entry)
    measured = [item for item in releases if default_arm in item["arms"]]
    trend = [
        (item["version"], item["arms"][default_arm]["surviving_pii_utf8_bytes"])
        for item in measured
        if _contract_key(item) == latest_contract
    ]
    lines.extend(["", f"**Trend across releases — `{default_arm}`.**"])
    if len(trend) < len(measured):
        lines.extend(
            [
                "",
                f"> Only rows measured under {contract_label(entry)} are on "
                "this line; "
                f"{len(measured) - len(trend)} row(s) under another contract are "
                "in the history table.",
            ]
        )
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
    # Rows that record their own shipped arm were appended with the refusal-aware
    # layout. A history of legacy rows alone keeps the original table byte for byte.
    if any("shipped_default_arm" in entry for entry in releases):
        return render_history_with_refusals(releases)
    latest_default_arm = shipped_default_arm(releases[-1])
    lines = [
        "| Release | Measured | Commit | Machine | Scorecard | "
        "Surviving PII bytes ↓ |",
        "| --- | --- | --- | --- | --- | ---: |",
    ]
    for entry in releases:
        # Each row reports the arm it shipped; name it when that differs from
        # the latest default so a changed default never reads as a leak change.
        default_arm = shipped_default_arm(entry)
        surviving = _fmt("int", entry["arms"][default_arm]["surviving_pii_utf8_bytes"])
        if default_arm != latest_default_arm:
            surviving += f" (`{default_arm}`)"
        lines.append(
            f"| {_history_version_cell(entry)} | {entry['date']} | `{entry['commit'][:7]}` | "
            f"{entry['machine']} | [`{entry['scorecard']}`]({entry['scorecard']}) | "
            f"{surviving} |"
        )
    return "\n".join(lines)


def _history_version_cell(entry: Mapping[str, Any]) -> str:
    version = entry["version"]
    if entry.get("provisional"):
        version += " *(provisional)*"
    if entry.get("scored_label_contract"):
        version += f" · {contract_label(entry)}"
    return version


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


def render_history_with_refusals(releases: Sequence[Mapping[str, Any]]) -> str:
    common = common_set_surviving_bytes(releases)
    lines = [
        "| Release | Measured | Commit | Machine | Scorecard | Shipped arm | "
        "Refused ↓ | Leaked PII bytes, all processed ↓ | "
        "Leaked PII bytes, common documents ↓ | False-positive bytes ↔ | "
        "Restore exact ↑ | clean p95 ms ↓ |",
        "| --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for entry, common_bytes in zip(releases, common):
        default_arm = shipped_default_arm(entry)
        arm = entry["arms"][default_arm]
        lines.append(
            f"| {_history_version_cell(entry)} | {entry['date']} | `{entry['commit'][:7]}` | "
            f"{entry['machine']} | [`{entry['scorecard']}`]({entry['scorecard']}) | "
            f"`{default_arm}` | {_fmt('int', arm['failed_closed_documents'])} | "
            f"{_fmt('int', arm['surviving_pii_utf8_bytes'])} | {_fmt('int', common_bytes)} | "
            f"{_fmt('int', arm['false_positive_utf8_bytes'])} | "
            f"{_fmt('pct', arm['restore_exact_rate'])} | {_fmt('ms', arm['clean_ms_p95'])} |"
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
