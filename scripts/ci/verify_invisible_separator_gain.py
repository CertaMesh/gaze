#!/usr/bin/env python3
"""Replay the retained invisible-separator gain evidence without detection."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = ROOT / "docs/reference/benchmarks/invisible-boundary-fixes/gain-generator-v30"
sys.path.insert(0, str(ROOT / "scripts/bench"))

import agentic_layers as agentic
import bench_json
import benchmark_spec
import gaze_bench_score as score
import scorecard_record as records
import subject_build


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, required=True)
    args = parser.parse_args()
    manifest = json.loads((EVIDENCE / "manifest.json").read_text())
    for name, expected in manifest["files"].items():
        path = EVIDENCE / name
        if Path(name).is_absolute() or ".." in Path(name).parts or path.is_symlink():
            raise ValueError("invalid evidence path")
        if sha256(path) != expected:
            raise ValueError("evidence hash mismatch")
    allowed = {
        "scripts/ci/verify_invisible_separator_gain.py",
        "scripts/ci/full-inputs.json",
    }
    changed = subprocess.check_output(
        ["git", "diff", "--name-only", manifest["candidate"], "--"], cwd=ROOT, text=True
    ).splitlines()
    evidence_prefix = EVIDENCE.relative_to(ROOT).as_posix() + "/"
    if any(name not in allowed and not name.startswith(evidence_prefix) for name in changed):
        raise ValueError("candidate differs beyond evidence publication")
    spec = json.loads((EVIDENCE / "specification.json").read_text())
    benchmark_spec.require_current(spec, ROOT, dataset=args.dataset)
    layer_contract = agentic.load_contract(ROOT)
    cards = {}
    record_paths = tuple(EVIDENCE / side / "observations-v1.jsonl.gz" for side in ("base", "candidate"))
    for side, record in zip(("base", "candidate"), record_paths, strict=True):
        capture_path = EVIDENCE / side / "capture-v1.json.gz"
        capture = bench_json.load(capture_path)
        if capture["gaze"] != {"revision": manifest[side], "dirty": False}:
            raise ValueError("capture source mismatch")
        if capture["benchmark_spec"] != spec or capture["observation_record"]["observations"] != 44040:
            raise ValueError("capture population or specification mismatch")
        if capture["observation_record"]["sha256"] != sha256(record):
            raise ValueError("capture record mismatch")
        subject_build.validate(capture, records.binary_sha256(record, required=True))
        execution = json.loads((EVIDENCE / side / "run-evidence.json").read_text())
        if execution["outcome"] != "completed" or execution["artifacts"] != {
            "scorecard": {"file": "capture-v1.json", "sha256": bench_json.content_sha256(capture_path)},
            "record": {"file": record.name, "sha256": sha256(record)},
        }:
            raise ValueError("capture execution binding mismatch")
        for version in (1, 2, 3, 4):
            contract = score.SCORED_LABEL_CONTRACT_V1 if version == 1 else score.load_scored_label_contract(
                ROOT / f"docs/reference/benchmarks/scored-labels-v{version}.json",
                display_path=f"docs/reference/benchmarks/scored-labels-v{version}.json",
            )
            replay = records.rescore(record, contract, layer_contract, max_workers=2)
            stored = bench_json.load(EVIDENCE / side / f"scorecard-v{version}.json.gz")
            if replay != stored:
                raise ValueError("scorecard replay mismatch")
            cards[side, version] = replay
            print(f"REPLAY {side} v{version}: pass", flush=True)
    for version in (2, 1):
        base, candidate = (cards[side, version] for side in ("base", "candidate"))
        if version == 2:
            contract = score.load_scored_label_contract(ROOT / "docs/reference/benchmarks/scored-labels-v2.json")
            actual = agentic.gate_frozen_records(base, candidate, *record_paths, contract)
        else:
            actual = agentic.gate(base, candidate, record_paths=record_paths)
        expected = json.loads((EVIDENCE / f"gate-v{version}.json").read_text())
        if actual != expected or actual["verdict"] != "pass":
            raise ValueError("gain gate replay mismatch")
        print(f"GATE v{version}: pass", flush=True)
    print("Invisible-separator gain evidence verified")


if __name__ == "__main__":
    main()
