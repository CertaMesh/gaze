#!/usr/bin/env python3
"""Replay and check the separate dictionary API-sequence release-history ledger."""
import argparse
import json
import hashlib
import subprocess
from pathlib import Path

import dictionary_sequences as sequence
import gaze_bench_score as score

ROOT = Path(__file__).resolve().parents[2]
LEDGER = Path("docs/reference/benchmarks/dictionary-sequence-history.json")


def totals(run):
    utf8 = run["metrics"]["utf8_bytes"]
    available = run["pipeline_availability"]
    contract = run["pipeline_contract"]
    return {
        "leaked_bytes": utf8["leaked"], "false_positive_bytes": utf8["false_positive"],
        "refusals": available["failed_closed_documents"],
        "attempted": available["attempted_documents"],
        "exact_restores": contract["restore_exact_documents"],
        "restore_failures": contract["documents"] - contract["restore_exact_documents"],
        "valid_manifests": contract["manifest_valid_documents"],
        "invalid_manifests": contract["documents"] - contract["manifest_valid_documents"],
    }


def replay_row(directory):
    result = {}
    for version in (1, 2, 4):
        contract = (score.SCORED_LABEL_CONTRACT_V1 if version == 1 else
                    score.load_scored_label_contract(ROOT /
                        f"docs/reference/benchmarks/scored-labels-v{version}.json"))
        replay = sequence.replay(directory, contract)
        result[str(version)] = {key: totals(run) for key, run in replay["runs"].items()}
    return result


def check(path=ROOT / LEDGER):
    ledger = json.loads(path.read_text())
    expected_releases = {"v0.14.0", "v0.15.0", "v0.15.1", "v0.16.0"}
    if {row["release"] for row in ledger["releases"]} != expected_releases:
        raise ValueError("dictionary history does not cover every displayed release")
    for row in [*ledger["releases"], *ledger["development"]]:
        if "unavailable" in row:
            if not row["unavailable"] or row["record_api_available"]:
                raise ValueError("invalid unavailable release explanation")
            continue
        directory = ROOT / row["capture"]
        manifest = json.loads((directory / "capture.json").read_text())
        if manifest.get("producer_binding", {}).get("fresh_locked_build") is not True:
            raise ValueError("history capture has no fresh producer build binding")
        generated = sequence.generate(manifest["identity"]["seed"], manifest["identity"]["partition"])
        expected_identity = sequence.identity(generated, manifest["identity"]["seed"],
                                              manifest["identity"]["partition"])
        # Original captures retain their exact producer source identity. Replaying
        # with stronger validation changes source bytes, not generated inputs.
        original_revision = "a6449e2f15563326e7ab9b76e5c9654148acd1e0"
        original_source = subprocess.check_output(
            ["git", "show", f"{original_revision}:scripts/bench/dictionary_sequences.py"], cwd=ROOT)
        original_hash = hashlib.sha256(original_source).hexdigest()
        if manifest["identity"].get("generator_sha256") == original_hash:
            expected_identity["generator_sha256"] = original_hash
        if manifest["identity"] != expected_identity:
            raise ValueError("capture does not use the committed generator and adapter")
        if manifest["runtime"]["revision"] != row["revision"]:
            raise ValueError("history runtime does not match capture")
        if replay_row(directory) != row["contracts"]:
            raise ValueError("dictionary history differs from canonical replay")
    proof = json.loads((ROOT / ledger["no_extension_proof"]).read_text())
    if proof["contract"] != "v4" or not proof["decision_byte_identical"]:
        raise ValueError("missing unchanged v4 gate proof")
    table = []
    for row in ledger["releases"]:
        if "unavailable" in row:
            table.append(f"| {row['release']} | unavailable: no public record-matching API | — | — | — | — |")
            continue
        contracts = row["contracts"]
        if contracts["1"] != contracts["2"] or contracts["1"] != contracts["4"]:
            raise ValueError("history table needs separate contract columns")
        runs = contracts["1"]
        def pair(key):
            return f"{runs[key]['leaked_bytes']:,} / {runs[key]['false_positive_bytes']:,}"
        errors = [sum(r[key] for r in runs.values())
                  for key in ("refusals", "restore_failures", "invalid_manifests")]
        table.append(f"| {row['release']} | {pair('switch/K')} | {runs['switch/B']['false_positive_bytes']:,} | {pair('fresh/K')} | {pair('literal/K')} | {' / '.join(map(str, errors))} |")
    page = (ROOT / "docs/reference/benchmarks/README.md").read_text()
    if "\n".join(table) not in page:
        raise ValueError("dictionary release-history table differs from its ledger")
    return ledger


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check",))
    parser.parse_args()
    check()
    print("dictionary sequence release history matches canonical v1/v2/v4 replays")


if __name__ == "__main__":
    main()
