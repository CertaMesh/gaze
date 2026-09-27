#!/usr/bin/env python3
"""Prove a stored release record reproduces committed contract scorecards."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import gaze_bench_score as score
import scorecard_record as record


TIMING_KEYS = {"generated_at", "latency_ms", "warm_latency_ms", "wall_seconds", "documents_per_second"}


def without_timing(value: object) -> object:
    if isinstance(value, dict):
        return {
            key: without_timing(item)
            for key, item in value.items()
            if key not in TIMING_KEYS and not key.endswith(("_ms", "_seconds"))
        }
    if isinstance(value, list):
        return [without_timing(item) for item in value]
    return value


def exact_except_timing(actual: dict, expected: dict) -> bool:
    return (
        json.dumps(without_timing(actual), indent=2, ensure_ascii=False) + "\n"
        == json.dumps(without_timing(expected), indent=2, ensure_ascii=False) + "\n"
    )


def verify(record_path: Path, v1_path: Path, v2_path: Path, v2_contract: Path,
           v3_contract: Path) -> dict[str, object]:
    v1 = record.rescore(record_path, score.SCORED_LABEL_CONTRACT_V1)
    v2 = record.rescore(record_path, score.load_scored_label_contract(
        v2_contract, display_path="docs/reference/benchmarks/scored-labels-v2.json"
    ))
    v3 = record.rescore(record_path, score.load_scored_label_contract(
        v3_contract, display_path="docs/reference/benchmarks/scored-labels-v3.json"
    ))
    expected_v1 = json.loads(v1_path.read_text(encoding="utf-8"))
    expected_v2 = json.loads(v2_path.read_text(encoding="utf-8"))
    result = {
        "v1_exact_except_timing": exact_except_timing(v1, expected_v1),
        "v2_exact_except_timing": exact_except_timing(v2, expected_v2),
        "v3_valid": (
            v3["schema_version"] == 4
            and v3["scoring"]["scored_label_contract"]["version"] == 3
            and all("gold_gap" in run["metrics"] for run in v3["runs"])
        ),
    }
    if not all(result.values()):
        raise record.RecordError(f"historical record proof failed: {result}")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("record", type=Path)
    parser.add_argument("--v1", type=Path, required=True)
    parser.add_argument("--v2", type=Path, required=True)
    parser.add_argument("--v2-contract", type=Path, required=True)
    parser.add_argument("--v3-contract", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(verify(args.record, args.v1, args.v2, args.v2_contract,
                            args.v3_contract), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
