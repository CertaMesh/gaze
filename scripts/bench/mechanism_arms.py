#!/usr/bin/env python3
"""Measure one detection mechanism on its own: the stack with it versus without it.

A mechanism is a policy delta: TOML sections that the candidate policy adds to
the base policy and nothing else. `record` takes two full runner outputs per
contract (base and candidate, v2 and v1), proves they are one comparable pair,
copies both observation records into the repository and writes one ledger row.
`check` re-derives every ledger number from those committed records (no corpus,
model or binary) and fails if the ledger or the rendered table drifted.

Adding a mechanism needs a delta file and a `record` run, no code. For a
mechanism that `gaze setup` already enables, the base is the setup policy minus
the delta's sections and the candidate is the setup policy, so the delta still
only adds.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import tomllib
from collections import Counter
from pathlib import Path
from typing import Any, Mapping, Sequence

import agentic_layers as agentic
import gaze_bench_score as score
import scorecard_record as record
import verify_record_scorecards as verify
from render_benchmark_doc import (
    GOLD_GAP_CONTRACT,
    HEADLINE_CONTRACT,
    RenderError,
    _gold_gap_from_run,
    begin_marker,
    check_gold_gap_arms,
    end_marker,
    headline_arms,
    version_sort_key,
)


ROOT = Path(__file__).resolve().parents[2]
BENCH_DIR = ROOT / "docs/reference/benchmarks"
DOC = BENCH_DIR / "README.md"
LEDGER = BENCH_DIR / "mechanism-arms.json"
EVIDENCE_DIR = BENCH_DIR / "mechanisms"
RELEASE_HISTORY = BENCH_DIR / "release-history.json"
BLOCK = "mechanism-arms"
SCHEMA_VERSION = 1
# From GOLD_GAP_CONTRACT on, false-positive bytes are the gold-gap adjusted
# value (same rule as render_benchmark_doc.headline_arms).
# The contracts a pair is measured (and gated) under; the others are re-scored.
GATED_CONTRACTS = (2, 1)
# The one scorecard field a v1 run may differ in from the v1 rescore of the v2
# run's record: it names the record file, which is a different file by
# construction. Everything else, timing aside, must match.
REPLAY_ALLOWED_DIFFERENCES = frozenset({"observation_record"})
MECHANISM_ID = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
ADDED_IN = re.compile(r"^v\d+\.\d+(?:\.\d+)?$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


class MechanismError(ValueError):
    pass


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _contract(version: int) -> score.ScoredLabelContract:
    """Contract v1 is built in; every later one is `scored-labels-v<N>.json`."""
    if version == 1:
        return score.SCORED_LABEL_CONTRACT_V1
    path = BENCH_DIR / f"scored-labels-v{version}.json"
    if not path.is_file():
        raise MechanismError(f"scored-label contract v{version} has no file at {path.name}")
    return score.load_scored_label_contract(
        path, display_path=path.relative_to(ROOT).as_posix()
    )


def required_contracts(bench_dir: Path | None = None) -> tuple[int, ...]:
    """v1 plus every `scored-labels-v<N>.json` in the repository, newest first.

    Every row must carry numbers under all of them. The set comes from the
    repository, never from the row, so deleting a contract block from a row
    fails `check` instead of quietly demoting the headline. The records do not
    depend on a contract, so a newly added contract needs only `refresh`, not a
    new benchmark run.
    """
    found = {
        int(match.group(1))
        for path in (bench_dir or BENCH_DIR).glob("scored-labels-v*.json")
        if (match := re.fullmatch(r"scored-labels-v(\d+)\.json", path.name))
    }
    versions = tuple(sorted(found | {1}, reverse=True))
    if versions[0] != HEADLINE_CONTRACT or not set(GATED_CONTRACTS) <= set(versions):
        raise MechanismError(
            f"contract files {list(versions)} disagree with headline v{HEADLINE_CONTRACT} "
            f"or lack the gated contracts {list(GATED_CONTRACTS)}"
        )
    return versions


def contract_version(scorecard: Mapping[str, Any]) -> int:
    contract = scorecard.get("scoring", {}).get("scored_label_contract")
    if contract is None:
        return 1
    version = contract.get("version")
    if type(version) is not int:
        raise MechanismError("scorecard contract version is not an integer")
    return version


def _single_run(scorecard: Mapping[str, Any]) -> Mapping[str, Any]:
    runs = scorecard.get("runs")
    if not isinstance(runs, list) or len(runs) != 1 or runs[0].get("config") != "policy-file":
        raise MechanismError("a mechanism arm is exactly one policy-file run")
    return runs[0]


def _count(value: Any, where: str) -> int:
    if type(value) is not int or value < 0:
        raise MechanismError(f"{where} is not a non-negative integer")
    return value


def headline(run: Mapping[str, Any], version: int) -> dict[str, int]:
    """Layer C bytes as the benchmark document reads them under `version`.

    The false-positive rule is the renderer's own (`headline_arms`), so this
    table and the release table can never disagree on which number is FP.
    """
    utf8 = run["metrics"]["utf8_bytes"]
    arm = {"false_positive_utf8_bytes": utf8["false_positive"], "byte_precision": None}
    try:
        gold_gap = _gold_gap_from_run(run, "policy-file")
        if gold_gap is not None:
            arm["gold_gap"] = gold_gap
        check_gold_gap_arms({"policy-file": arm}, version, f"contract v{version}")
        false_positive = headline_arms({"policy-file": arm}, version)["policy-file"][
            "false_positive_utf8_bytes"
        ]
    except RenderError as error:
        raise MechanismError(str(error)) from error
    return {
        "gold": _count(utf8["pii"], "utf8_bytes.pii"),
        "leaked": _count(utf8["leaked"], "utf8_bytes.leaked"),
        "false_positive": _count(false_positive, "false-positive bytes"),
        "failed_closed": _count(
            run["pipeline_availability"]["failed_closed_documents"], "failed_closed_documents"
        ),
    }


def leaked_by_label(run: Mapping[str, Any]) -> dict[str, int]:
    return {
        label: _count(block["leaked_utf8_bytes"], f"{label}.leaked_utf8_bytes")
        for label, block in run["per_label_recall"].items()
    }


def false_positive_by_class(record_path: Path, contract: score.ScoredLabelContract) -> dict[str, int]:
    """Layer C false-positive bytes per predicted class, from the scorer's own view.

    A byte two classes cover counts for each, so the classes can sum to more
    than the run's total; the per-class change is what this is for.
    """
    header, observations = record._read(record_path)
    rows = {row["id"]: row for row in header["documents"]}
    result: Counter[str] = Counter()
    for observation in observations:
        response = observation["response"]
        if observation["layer"] != "C" or response.get("refused"):
            continue
        (document,) = score.apply_scored_label_contract(
            [record._document_from_row(rows[observation["document_id"]])], contract
        )
        gold, ignored, predictions = score.contract_scoring_view(
            document, score.final_trace_predictions(document, response)
        )
        for label in {span.label for span in predictions}:
            predicted = score.subtract_intervals(
                score.merge_intervals(
                    (span.start, span.end) for span in predictions if span.label == label
                ),
                ignored,
            )
            outside = score.interval_length(score.subtract_intervals(predicted, gold))
            if outside:
                result[label] += outside
    return dict(sorted(result.items()))


def _delta(base: Mapping[str, int], candidate: Mapping[str, int]) -> dict[str, int]:
    return {
        key: candidate.get(key, 0) - base.get(key, 0)
        for key in sorted(base.keys() | candidate.keys())
        if candidate.get(key, 0) != base.get(key, 0)
    }


def identity_from_card(scorecard: Mapping[str, Any]) -> dict[str, Any]:
    return {
        "revision": scorecard["gaze"]["revision"],
        "corpus_sha256": scorecard["dataset"]["integrity"]["sha256"],
        "sampling_seed": scorecard["parameters"]["sampling_seed"],
        "policy_sha256": scorecard["parameters"]["policy_sha256"],
    }


def derive(
    base_record: Path, candidate_record: Path, versions: Sequence[int] | None = None
) -> dict[str, Any]:
    """Every derived ledger field, recomputed from the two committed records:
    identity (commit, corpus, seed, both policy SHAs), the numbers per contract,
    the agentic layers and the gate verdict per gated contract."""
    contracts: dict[str, Any] = {}
    layers: dict[str, Any] = {}
    gate: dict[str, Any] = {}
    identity: dict[str, Any] = {}
    for version in versions or required_contracts():
        contract = _contract(version)
        cards = {
            "base": record.rescore(base_record, contract),
            "candidate": record.rescore(candidate_record, contract),
        }
        runs = {arm: _single_run(card) for arm, card in cards.items()}
        for arm, card in cards.items():
            if contract_version(card) != version:
                raise MechanismError(f"{arm} rescored under v{version} reports another contract")
        entry = {arm: headline(run, version) for arm, run in runs.items()}
        entry["leaked_by_label_delta"] = _delta(
            leaked_by_label(runs["base"]), leaked_by_label(runs["candidate"])
        )
        if version < GOLD_GAP_CONTRACT:
            entry["false_positive_by_class_delta"] = _delta(
                false_positive_by_class(base_record, contract),
                false_positive_by_class(candidate_record, contract),
            )
        contracts[str(version)] = entry
        base_identity, candidate_identity = (identity_from_card(card) for card in cards.values())
        shared = {key: base_identity[key] for key in ("revision", "corpus_sha256", "sampling_seed")}
        if any(candidate_identity[key] != value for key, value in shared.items()):
            raise MechanismError("base and candidate records differ in commit, corpus or seed")
        identity = {
            **shared,
            "base_policy_sha256": base_identity["policy_sha256"],
            "candidate_policy_sha256": candidate_identity["policy_sha256"],
        }
        if version in GATED_CONTRACTS:
            decision = agentic.decide(
                agentic.layer_totals(cards["base"], "policy-file"),
                agentic.layer_totals(cards["candidate"], "policy-file"),
            )
            gate[f"v{version}"] = {"verdict": decision["verdict"], "reason": decision["reason"]}
        if not layers and "layers" in cards["base"]:
            totals = {
                arm: agentic.layer_totals(card, "policy-file") for arm, card in cards.items()
            }
            layers = {
                layer: {
                    arm: {
                        "leaked": totals[arm][layer]["leaked"],
                        "false_positive": totals[arm][layer]["false_positive"],
                        "failed_closed": totals[arm][layer]["failed_closed"],
                    }
                    for arm in ("base", "candidate")
                }
                for layer in ("A", "D", "R")
            }
    return {**identity, "contracts": contracts, "agentic_layers": layers, "gate": gate}


# --------------------------------------------------------------------------
# record
# --------------------------------------------------------------------------


def _load_arm(directory: Path) -> tuple[dict[str, Any], Path]:
    scorecard_path = directory / "scorecard-v4.json"
    record_path = directory / "observations-v1.jsonl.gz"
    for path in (scorecard_path, record_path):
        if not path.is_file():
            raise MechanismError(f"runner output is missing {path}")
    scorecard = json.loads(scorecard_path.read_text(encoding="utf-8"))
    pinned = scorecard.get("observation_record", {}).get("sha256")
    if pinned != sha256(record_path):
        raise MechanismError(f"{record_path} does not match its scorecard's pinned SHA-256")
    return scorecard, record_path


def _identity(scorecard: Mapping[str, Any]) -> dict[str, Any]:
    parameters = scorecard["parameters"]
    return {
        "revision": scorecard["gaze"]["revision"],
        "dirty": scorecard["gaze"]["dirty"],
        "corpus_sha256": scorecard["dataset"]["integrity"]["sha256"],
        "profile": parameters["profile"],
        "binary_profile": parameters["binary_profile"],
        "sampling_seed": parameters["sampling_seed"],
        "scored_population": _single_run(scorecard)["scored_population"]["document_ids_digest"],
    }


def _replay_matches(record_path: Path, measured: Mapping[str, Any]) -> list[str]:
    """Top-level fields where the v1 rescore of `record_path` differs from a measured v1 run."""
    replay = verify.without_timing(record.rescore(record_path, _contract(1)))
    expected = verify.without_timing(measured)
    return sorted(
        key for key in replay.keys() | expected.keys()
        if replay.get(key) != expected.get(key)
    )


def check_pair(
    arms: Mapping[str, tuple[dict[str, Any], Path]], delta: Path
) -> dict[str, Any]:
    """Refuse anything but one comparable base/candidate pair per contract."""
    for name, (scorecard, _) in arms.items():
        want = 2 if name.endswith("v2") else 1
        if contract_version(scorecard) != want:
            raise MechanismError(f"{name} was measured under v{contract_version(scorecard)}, not v{want}")
        if scorecard["gaze"]["dirty"] is not False:
            raise MechanismError(f"{name} was measured from a dirty tree")
        if scorecard["parameters"]["profile"] != "full":
            raise MechanismError(f"{name} is not a full profile")
        _single_run(scorecard)
    identities = {name: _identity(scorecard) for name, (scorecard, _) in arms.items()}
    reference = identities["base-v2"]
    for name, identity in identities.items():
        differing = sorted(key for key in reference if identity[key] != reference[key])
        if differing:
            raise MechanismError(f"{name} differs from base-v2 in {differing}")
    policies: dict[str, set[str]] = {"base": set(), "candidate": set()}
    gates: dict[str, Any] = {}
    for suffix in ("v2", "v1"):
        base, candidate = arms[f"base-{suffix}"][0], arms[f"candidate-{suffix}"][0]
        policies["base"].add(base["parameters"]["policy_sha256"])
        policies["candidate"].add(candidate["parameters"]["policy_sha256"])
        result = agentic.gate(base, candidate, policy_delta=delta)
        if result["verdict"] == "not_comparable":
            raise MechanismError(
                f"{suffix} pair is not comparable: {result.get('policy_delta_reason')}; "
                f"differing: {result.get('differing')}"
            )
        gates[suffix] = {"verdict": result["verdict"], "reason": result["reason"]}
    for arm, digests in policies.items():
        if len(digests) != 1:
            raise MechanismError(f"{arm} policy differs between the v2 and v1 runs")
    for arm in ("base", "candidate"):
        differing = _replay_matches(arms[f"{arm}-v2"][1], arms[f"{arm}-v1"][0])
        unexpected = sorted(set(differing) - REPLAY_ALLOWED_DIFFERENCES)
        if unexpected:
            raise MechanismError(
                f"{arm}: the v1 run differs from the v1 rescore of the v2 record in {unexpected}"
            )
    return {
        "revision": reference["revision"],
        "corpus_sha256": reference["corpus_sha256"],
        "sampling_seed": reference["sampling_seed"],
        "base_policy_sha256": policies["base"].pop(),
        "candidate_policy_sha256": policies["candidate"].pop(),
        "gate": gates,
    }


ATTESTED_NOTE = (
    "Recorded at record time from --binary, git and --machine; the observation records "
    "do not carry them, so check cannot re-derive them."
)
# Fields a measurement holds besides what `derive` recomputes from its records.
NON_DERIVED_FIELDS = frozenset({"release", "records", "attested"})


def git_crates_tree(revision: str) -> str:
    """The measured commit's `crates/` tree: equal trees mean the same detection code,
    so a later rebase that touches only scripts or docs leaves the row valid."""
    try:
        return subprocess.run(
            ["git", "-C", str(ROOT), "rev-parse", f"{revision}:crates"],
            check=True, capture_output=True, text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as error:
        raise MechanismError(f"cannot resolve the crates tree of {revision}: {error}") from error


def record_measurement(
    ledger: dict[str, Any], *, mechanism: str, title: str, added_in: str, delta: Path,
    runs: Mapping[str, Path], binary: Path, machine: str, release: str | None,
    crates_tree: str | None = None,
    evidence_dir: Path = EVIDENCE_DIR, root: Path = ROOT,
) -> dict[str, Any]:
    root, delta, evidence_dir = root.resolve(), delta.resolve(), evidence_dir.resolve()
    if not MECHANISM_ID.fullmatch(mechanism):
        raise MechanismError(f"mechanism id {mechanism!r} must be lowercase kebab case")
    if not ADDED_IN.fullmatch(added_in):
        raise MechanismError(f"added_in {added_in!r} must look like v0.16 or v0.16.0")
    if release is not None and not re.fullmatch(r"^v\d+\.\d+\.\d+$", release):
        raise MechanismError(f"release {release!r} must look like v0.16.0")
    if not machine.strip():
        raise MechanismError("--machine is required: the scorecard does not record the host")
    arms = {name: _load_arm(path) for name, path in runs.items()}
    if set(arms) != {"base-v2", "candidate-v2", "base-v1", "candidate-v1"}:
        raise MechanismError("record needs base-v2, candidate-v2, base-v1 and candidate-v1")
    pair = check_pair(arms, delta)
    crates_tree = crates_tree or git_crates_tree(pair["revision"])
    evidence_dir.mkdir(parents=True, exist_ok=True)
    stem = f"{mechanism}-{release or pair['revision'][:12]}"
    files = {}
    for arm in ("base", "candidate"):
        target = evidence_dir / f"{stem}-{arm}.jsonl.gz"
        shutil.copyfile(arms[f"{arm}-v2"][1], target)
        files[arm] = {"file": target.relative_to(root).as_posix(), "sha256": sha256(target)}
    derived = derive(evidence_dir / f"{stem}-base.jsonl.gz", evidence_dir / f"{stem}-candidate.jsonl.gz")
    disagreeing = sorted(key for key, value in pair.items() if derived[key] != value)
    if disagreeing:
        raise MechanismError(f"the committed records re-derive other {disagreeing} than the runs")
    measurement = {
        "release": release,
        "records": files,
        **derived,
        "attested": {
            "binary_sha256": sha256(binary),
            "crates_tree": crates_tree,
            "machine": machine,
            "note": ATTESTED_NOTE,
        },
    }
    entry = next((item for item in ledger["mechanisms"] if item["id"] == mechanism), None)
    delta_row = {"file": delta.relative_to(root).as_posix(), "sha256": sha256(delta)}
    if entry is None:
        entry = {"id": mechanism, "title": title, "added_in": added_in,
                 "policy_delta": delta_row, "measurements": []}
        ledger["mechanisms"].append(entry)
    elif entry["policy_delta"] != delta_row or entry["added_in"] != added_in:
        raise MechanismError(f"{mechanism} is already recorded with another delta or added_in")
    entry["measurements"] = [
        item for item in entry["measurements"]
        if (item["release"], item["revision"]) != (release, pair["revision"])
    ] + [measurement]
    validate(ledger, root)
    return measurement


# --------------------------------------------------------------------------
# validation, rendering, check
# --------------------------------------------------------------------------


def empty_ledger() -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "comment": (
            "Per-mechanism benchmark arms: the stack with one mechanism versus without it. "
            "Written by scripts/bench/mechanism_arms.py record; checked by its check command."
        ),
        "mechanisms": [],
    }


def validate(ledger: Mapping[str, Any], root: Path = ROOT) -> None:
    if ledger.get("schema_version") != SCHEMA_VERSION:
        raise MechanismError(f"ledger schema_version must be {SCHEMA_VERSION}")
    seen: set[str] = set()
    for entry in ledger.get("mechanisms", []):
        where = f"mechanism {entry.get('id')!r}"
        if not isinstance(entry.get("id"), str) or not MECHANISM_ID.fullmatch(entry["id"]):
            raise MechanismError(f"{where}: invalid id")
        if entry["id"] in seen:
            raise MechanismError(f"{where}: duplicate id")
        seen.add(entry["id"])
        if not isinstance(entry.get("added_in"), str) or not ADDED_IN.fullmatch(entry["added_in"]):
            raise MechanismError(f"{where}: invalid added_in")
        if not isinstance(entry.get("title"), str) or not entry["title"].strip():
            raise MechanismError(f"{where}: title is missing")
        _check_file(entry.get("policy_delta"), f"{where} policy_delta", root)
        delta = tomllib.loads((root / entry["policy_delta"]["file"]).read_text(encoding="utf-8"))
        if not delta or any(not isinstance(section, dict) for section in delta.values()):
            raise MechanismError(f"{where}: policy delta must add at least one TOML section")
        if not entry.get("measurements"):
            raise MechanismError(f"{where}: no measurement")
        for measurement in entry["measurements"]:
            _validate_measurement(measurement, f"{where} @ {measurement.get('revision')}", root)


def _check_file(row: Any, where: str, root: Path) -> None:
    if not isinstance(row, dict) or not isinstance(row.get("file"), str):
        raise MechanismError(f"{where}: file is missing")
    if not isinstance(row.get("sha256"), str) or not HEX64.fullmatch(row["sha256"]):
        raise MechanismError(f"{where}: sha256 is missing")
    path = root / row["file"]
    if not path.is_file() or sha256(path) != row["sha256"]:
        raise MechanismError(f"{where}: {row['file']} is missing or differs from its SHA-256")


def _validate_measurement(measurement: Mapping[str, Any], where: str, root: Path) -> None:
    sha1 = re.compile(r"^[0-9a-f]{40}$")
    attested = measurement.get("attested")
    if not isinstance(attested, dict):
        raise MechanismError(f"{where}: attested block is missing")
    for key, pattern, value in (
        ("revision", sha1, measurement.get("revision")),
        ("corpus_sha256", HEX64, measurement.get("corpus_sha256")),
        ("base_policy_sha256", HEX64, measurement.get("base_policy_sha256")),
        ("candidate_policy_sha256", HEX64, measurement.get("candidate_policy_sha256")),
        ("attested.crates_tree", sha1, attested.get("crates_tree")),
        ("attested.binary_sha256", HEX64, attested.get("binary_sha256")),
    ):
        if not isinstance(value, str) or not pattern.fullmatch(value):
            raise MechanismError(f"{where}: {key} is missing or malformed")
    if measurement["base_policy_sha256"] == measurement["candidate_policy_sha256"]:
        raise MechanismError(f"{where}: base and candidate policies are identical")
    if not isinstance(attested.get("machine"), str) or not attested["machine"].strip():
        raise MechanismError(f"{where}: attested.machine is missing")
    for arm in ("base", "candidate"):
        _check_file(measurement.get("records", {}).get(arm), f"{where} {arm} record", root)
    contracts = measurement.get("contracts", {})
    if not all(isinstance(key, str) and key.isdigit() for key in contracts) or not {
        str(version) for version in GATED_CONTRACTS
    } <= set(contracts):
        raise MechanismError(
            f"{where}: needs numbers under at least contracts {list(GATED_CONTRACTS)}"
        )
    if set(measurement.get("gate", {})) != {f"v{version}" for version in GATED_CONTRACTS}:
        raise MechanismError(f"{where}: needs the v2 and v1 gate verdicts")


def measured_contracts(measurement: Mapping[str, Any]) -> list[int]:
    """The contracts a row has numbers for, newest first. `check` requires these to
    be exactly `required_contracts()`; validation and rendering accept any set."""
    return sorted((int(key) for key in measurement["contracts"]), reverse=True)


def load_ledger(path: Path = LEDGER) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8")) if path.is_file() else empty_ledger()


def _signed(value: int) -> str:
    return f"{value:+,}" if value else "0"


def _released_versions(history_path: Path) -> list[str]:
    history = json.loads(history_path.read_text(encoding="utf-8"))
    return sorted(
        (release["version"] for release in history["releases"]),
        key=version_sort_key, reverse=True,
    )


def _before(version: str, added_in: str) -> bool:
    def parts(value: str) -> tuple[int, int, int]:
        numbers = [int(part) for part in value.lstrip("v").split("-")[0].split(".")]
        return (numbers + [0, 0])[:3]  # type: ignore[return-value]
    return parts(version) < parts(added_in)


def release_cell(entry: Mapping[str, Any], version: str) -> str:
    """One mechanism's cell for a shipped release: numbers or an explicit reason."""
    if _before(version, entry["added_in"]):
        return f"not available: mechanism added in {entry['added_in']}"
    measured = next((m for m in entry["measurements"] if m["release"] == version), None)
    if measured is None:
        return "not measured for this release"
    version = measured_contracts(measured)[0]
    headline_row = measured["contracts"][str(version)]
    return (
        f"v{version}: leaked {_signed(headline_row['candidate']['leaked'] - headline_row['base']['leaked'])} B, "
        f"FP {_signed(headline_row['candidate']['false_positive'] - headline_row['base']['false_positive'])} B"
    )


def _link(repo_path: str) -> str:
    """A repository path as a link from the benchmark document."""
    return os.path.relpath(repo_path, DOC.parent.relative_to(ROOT).as_posix())


def render(ledger: Mapping[str, Any], releases: Sequence[str]) -> str:
    lines = [
        "Each row runs the same binary, corpus and seed twice: once with the base policy, "
        "once with the base policy plus one mechanism's policy delta. Nothing else differs, "
        "so the change is that mechanism's own effect. Layer C bytes; the contract v3 "
        "headline counts false-positive bytes after gold-gap credit.",
        "",
    ]
    if not ledger["mechanisms"]:
        return "\n".join(lines + ["No mechanism has been measured on its own yet."])
    lines += [
        "| Mechanism | Measured at | Contract | Leaked bytes, without → with | FP bytes, without → with | Gate |",
        "| --- | --- | --- | ---: | ---: | --- |",
    ]
    for entry in ledger["mechanisms"]:
        for measurement in entry["measurements"]:
            at = measurement["release"] or f"`{measurement['revision'][:12]}` (unreleased)"
            for version in measured_contracts(measurement):
                row = measurement["contracts"][str(version)]
                base, candidate = row["base"], row["candidate"]
                gate = measurement["gate"].get(f"v{version}", {}).get(
                    "verdict", "not gated: re-scored from the v2 records"
                )
                lines.append(
                    f"| {entry['title']} | {at} | v{version} | "
                    f"{base['leaked']:,} → {candidate['leaked']:,} "
                    f"({_signed(candidate['leaked'] - base['leaked'])}) | "
                    f"{base['false_positive']:,} → {candidate['false_positive']:,} "
                    f"({_signed(candidate['false_positive'] - base['false_positive'])}) | {gate} |"
                )
    lines += ["", "What moved, per label (contract v2; v1 adds only the credential labels):", ""]
    for entry in ledger["mechanisms"]:
        for measurement in entry["measurements"]:
            row = measurement["contracts"]["2"]
            leaked = ", ".join(
                f"{label} {_signed(value)}" for label, value in row["leaked_by_label_delta"].items()
            ) or "none"
            fp = ", ".join(
                f"`{label}` {_signed(value)}"
                for label, value in row["false_positive_by_class_delta"].items()
            ) or "none"
            layers = "; ".join(
                f"{layer} leaked {_signed(block['candidate']['leaked'] - block['base']['leaked'])}, "
                f"FP {_signed(block['candidate']['false_positive'] - block['base']['false_positive'])}"
                for layer, block in measurement["agentic_layers"].items()
            ) or "not measured"
            lines.append(
                f"- **{entry['title']}**: leaked bytes by gold label: {leaked}. "
                f"FP bytes by predicted class: {fp}. Agentic layers: {layers}. "
                f"Policy delta [`{Path(entry['policy_delta']['file']).name}`]"
                f"({_link(entry['policy_delta']['file'])}); evidence "
                f"[base]({_link(measurement['records']['base']['file'])}) and "
                f"[candidate]({_link(measurement['records']['candidate']['file'])}) "
                f"observation records. Attested, not re-derivable: `crates/` tree "
                f"`{measurement['attested']['crates_tree'][:12]}`, binary "
                f"`{measurement['attested']['binary_sha256'][:12]}`, {measurement['attested']['machine']}."
            )
    lines += [
        "",
        "Shipped releases, one column per release:",
        "",
        "| Mechanism | " + " | ".join(f"`{version}`" for version in releases) + " |",
        "| --- |" + " --- |" * len(releases),
    ]
    for entry in ledger["mechanisms"]:
        lines.append(
            f"| {entry['title']} | "
            + " | ".join(release_cell(entry, version) for version in releases) + " |"
        )
    return "\n".join(lines)


def apply(document: str, body: str) -> str:
    begin, end = begin_marker(BLOCK), end_marker(BLOCK)
    start, stop = document.find(begin), document.find(end)
    if start < 0 or stop < 0:
        raise MechanismError(f"document is missing the {BLOCK!r} generated block")
    if stop < start:
        raise MechanismError(f"{BLOCK!r} markers are out of order")
    return document[: start + len(begin)] + "\n\n" + body + "\n\n" + document[stop:]


def check_evidence(ledger: Mapping[str, Any], root: Path = ROOT) -> None:
    """Every stored number equals what the committed records re-derive."""
    for entry in ledger["mechanisms"]:
        for measurement in entry["measurements"]:
            where = f"{entry['id']} @ {str(measurement.get('revision'))[:12]}"
            required = required_contracts()
            if measured_contracts(measurement) != list(required):
                raise MechanismError(
                    f"{where}: has contracts {measured_contracts(measurement)}, the repository "
                    f"requires {list(required)}; run `mechanism_arms.py refresh`"
                )
            derived = derive(
                root / measurement["records"]["base"]["file"],
                root / measurement["records"]["candidate"]["file"],
                required,
            )
            unexplained = sorted(set(measurement) - set(derived) - NON_DERIVED_FIELDS)
            if unexplained:
                raise MechanismError(f"{where}: fields {unexplained} are neither derived nor attested")
            for key, value in derived.items():
                if measurement.get(key) != value:
                    raise MechanismError(
                        f"{where}: {key} differs from what its committed records re-derive"
                    )


def refresh(ledger: dict[str, Any], root: Path = ROOT) -> None:
    """Re-derive every row from its committed records under today's contracts."""
    for entry in ledger["mechanisms"]:
        for measurement in entry["measurements"]:
            measurement.update(derive(
                root / measurement["records"]["base"]["file"],
                root / measurement["records"]["candidate"]["file"],
            ))


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    add = commands.add_parser("record", help="record one base/candidate measurement")
    add.add_argument("--mechanism", required=True)
    add.add_argument("--title", required=True)
    add.add_argument("--added-in", required=True)
    add.add_argument("--policy-delta", type=Path, required=True)
    for name in ("base-v2", "candidate-v2", "base-v1", "candidate-v1"):
        add.add_argument(f"--{name}", type=Path, required=True,
                         help="runner output directory holding scorecard-v4.json")
    add.add_argument("--binary", type=Path, required=True,
                     help="the clean_for_bench binary all four runs used")
    add.add_argument("--machine", required=True)
    add.add_argument("--release", help="the release this measures; omit for an unreleased commit")
    commands.add_parser("check", help="fail if the ledger or its table drifted from the records")
    commands.add_parser("render", help="rewrite the table from the ledger")
    commands.add_parser(
        "refresh", help="re-derive every row from its records, e.g. after a new contract file"
    )
    args = parser.parse_args(argv)
    try:
        ledger = load_ledger()
        if args.command == "record":
            record_measurement(
                ledger, mechanism=args.mechanism, title=args.title, added_in=args.added_in,
                delta=args.policy_delta,
                runs={name: getattr(args, name.replace("-", "_"))
                      for name in ("base-v2", "candidate-v2", "base-v1", "candidate-v1")},
                binary=args.binary, machine=args.machine, release=args.release,
            )
            LEDGER.write_text(json.dumps(ledger, indent=2) + "\n", encoding="utf-8")
        elif args.command == "refresh":
            refresh(ledger)
            LEDGER.write_text(json.dumps(ledger, indent=2) + "\n", encoding="utf-8")
        validate(ledger)
        original = DOC.read_text(encoding="utf-8")
        rendered = apply(original, render(ledger, _released_versions(RELEASE_HISTORY)))
        if args.command == "check":
            check_evidence(ledger)
            if rendered != original:
                raise MechanismError("mechanism table differs from the committed ledger")
            print("mechanism arms ledger and table match their committed records")
        else:
            DOC.write_text(rendered, encoding="utf-8")
            print(f"wrote {LEDGER.relative_to(ROOT)} table")
        return 0
    except (MechanismError, agentic.LayerError, record.RecordError, KeyError,
            OSError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        parser.exit(2, f"mechanism_arms: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
