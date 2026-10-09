#!/usr/bin/env python3
"""Audit the compatible v0.16 age-map extension without claiming a prediction replay.

Historical numbers stay exactly as published. Each old row's composed native
label map is checked at its own committed harness revision; the one added
Gaze label is allowed only for historical detection trees that cannot emit it.
Run with --write to refresh aggregate mapping receipts, or --check to audit.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "theirbench"))
import compare
import loaders
import pii_tracer
import theirbench

ROOT = compare.REPO
BASE = "5a7dac0fa198f20a628ed6b7fa2b5c9d41a91eb0"
BENCH = ROOT / "docs/reference/benchmarks"
NATIVE_MAP = "scripts/bench/compare/theirbench/label-maps.json"


def git_file(commit: str, path: str) -> bytes:
    return subprocess.check_output(["git", "show", f"{commit}:{path}"], cwd=ROOT)


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def audit() -> dict:
    old_map_raw = git_file(BASE, "scripts/bench/compare/label-map.json")
    old_map = json.loads(old_map_raw)
    current_map = json.loads(compare.MAP_PATH.read_bytes())
    full_map = json.loads(compare.MAP_PATH.read_bytes())
    require(current_map['gaze'].pop('custom:age') == ['AGE'], 'benchmark mapping audit failed')
    require(current_map == old_map, 'an existing mapping changed')
    require(compare.common_claimed_labels(full_map) == compare.common_claimed_labels(old_map), 'benchmark mapping audit failed')
    old_comparison = json.loads(git_file(BASE, "docs/reference/benchmarks/comparison.json"))
    comparison = json.loads((BENCH / "comparison.json").read_bytes())
    require(comparison['mapping_extension']['previous_sha256'] == digest(old_map_raw), 'benchmark mapping audit failed')
    require(comparison['mapping_sha256'] == digest(compare.MAP_PATH.read_bytes()), 'benchmark mapping audit failed')
    require(set(comparison) == set(old_comparison) | {'mapping_extension'}, 'benchmark mapping audit failed')
    for key in old_comparison:
        if key != "mapping_sha256":
            require(comparison[key] == old_comparison[key], f'comparison/{key} changed')
    source_commits = {old_comparison["gaze_main_revision"]}
    old = json.loads(git_file(BASE, "docs/reference/benchmarks/their-benchmarks.json"))
    data = json.loads((BENCH / "their-benchmarks.json").read_bytes())
    current_native_raw = (ROOT / NATIVE_MAP).read_bytes()
    current_native = json.loads(current_native_raw)
    # The historical Gaze native labels are the old map; the added age label is
    # checked separately below against the detection code, not treated as emitted.
    tables = {**old_map, "pii-tracer": pii_tracer.load_label_map()}
    for benchmark, original in old["benchmarks"].items():
        entry = data["benchmarks"][benchmark]
        verified = []
        composed = {
            family: loaders.compose_mapping(family, table, benchmark, current_native)
            for family, table in {**full_map, "pii-tracer": pii_tracer.load_label_map()}.items()
        }
        require(sorted(theirbench.common_intersection(composed)) == original["common_intersection_labels"],
                f"{benchmark} current common intersection changed")
        for row, metrics in original["rows"].items():
            require(entry['rows'][row] == metrics, f'{benchmark}/{row} metrics changed')
            require(entry['own_metric'][row] == original['own_metric'][row], 'benchmark mapping audit failed')
            require(entry['provenance'][row] == original['provenance'][row], 'benchmark mapping audit failed')
            release = original["provenance"][row].get("release")
            if release:
                source_commits.add(release["commit"])
            separate = original.get("rows_measured_separately", {}).get(row, original)
            revision = separate.get("rescored_with", separate)["harness_revision"]
            native_raw = git_file(revision, NATIVE_MAP)
            native = json.loads(native_raw)
            family = theirbench.tool_family(row)
            before = loaders.compose_mapping(family, tables[family], benchmark, native)
            after = loaders.compose_mapping(family, tables[family], benchmark, current_native)
            require(before == after, f'{benchmark}/{row} composed labels changed')
            verified.append({"row": row, "harness_revision": revision,
                             "label_maps_sha256": digest(native_raw)})
        require(entry['common_intersection_labels'] == original['common_intersection_labels'], 'benchmark mapping audit failed')
        if original["label_maps_sha256"] != digest(current_native_raw):
            receipt = {"previous_sha256": original["label_maps_sha256"],
                       "current_sha256": digest(current_native_raw),
                       "metrics_changed": False, "verified_row_maps": verified,
                       "reason": "Global label metadata gained PII-TRACE definitions. Each historical row's composed map on this benchmark is unchanged at its own committed harness revision; original metrics and per-row receipts are preserved."}
            entry["label_maps_sha256"] = receipt["current_sha256"]
            entry["label_map_metadata_update"] = receipt
    for revision in source_commits:
        done = subprocess.run(
            ["git", "grep", "-n", "-E", r'"age"|::Age\b|person_age|custom:age', revision,
             "--", "crates/gaze-recognizers", "crates/gaze-cli"],
            cwd=ROOT, capture_output=True, text=True,
        )
        require(done.returncode == 1, f'historical Gaze {revision} can emit age')
    require(sorted(source_commits) == comparison['mapping_extension']['historical_gaze_commits_without_age'], 'benchmark mapping audit failed')
    return data


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--write", action="store_true")
    action.add_argument("--check", action="store_true")
    args = parser.parse_args()
    path = BENCH / "their-benchmarks.json"
    result = audit()
    if args.write:
        path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    else:
        require(json.loads(path.read_bytes()) == result, 'mapping receipts need --write')
    print("Age-map extension audited: all historical numeric rows and composed labels unchanged.")


if __name__ == "__main__":
    main()
