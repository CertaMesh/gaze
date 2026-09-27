#!/usr/bin/env python3
"""Prove a stored release record reproduces committed contract scorecards.

Two modes. The default replays a record captured in the same run as the
committed scorecards and demands the whole scorecard back, except timing.
`--capture` checks a record captured later, with today's harness, against a
release's older committed scorecards: provenance and harness fields differ by
construction, so it compares the dataset identity and every run field except
timing. A run field the harness itself computes (not the release) may be
skipped by name with `--ignore-run-field`; the result still reports, per
skipped field, whether it differed, so nothing is dropped silently.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Sequence

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


RUN_TIMING = {"latency_ms", "warm_latency_ms", "process"}


def _runs(card: dict, drop: set[str]) -> list[dict]:
    return [
        without_timing({k: v for k, v in run.items() if k not in RUN_TIMING | drop})
        for run in card["runs"]
    ]


def _dataset_identity(card: dict) -> dict:
    dataset = card["dataset"]
    return {key: dataset.get(key) for key in ("integrity", "evaluated_population")}


def verify_capture(record_path: Path, v1_path: Path, v2_path: Path, v2_contract: Path,
                   v3_contract: Path, ignore: Sequence[str] = ()) -> dict[str, object]:
    """A later capture of a release reproduces its committed v1 and v2 results."""
    contracts = {
        "v1": score.SCORED_LABEL_CONTRACT_V1,
        "v2": score.load_scored_label_contract(
            v2_contract, display_path="docs/reference/benchmarks/scored-labels-v2.json"
        ),
    }
    committed = {
        "v1": json.loads(v1_path.read_text(encoding="utf-8")),
        "v2": json.loads(v2_path.read_text(encoding="utf-8")),
    }
    skipped = set(ignore)
    result: dict[str, object] = {}
    ignored: dict[str, dict[str, bool]] = {name: {} for name in sorted(skipped)}
    for name, contract in contracts.items():
        replay = record.rescore(record_path, contract)
        expected = committed[name]
        result[f"{name}_commit_matches"] = replay["gaze"]["revision"] == expected["gaze"]["revision"]
        result[f"{name}_dataset_matches"] = _dataset_identity(replay) == _dataset_identity(expected)
        result[f"{name}_runs_match"] = _runs(replay, skipped) == _runs(expected, skipped)
        for field in skipped:
            ignored[field][name] = [run.get(field) for run in replay["runs"]] != [
                run.get(field) for run in expected["runs"]
            ]
    v3 = record.rescore(record_path, score.load_scored_label_contract(
        v3_contract, display_path="docs/reference/benchmarks/scored-labels-v3.json"
    ))
    result["v3_valid"] = (
        v3["schema_version"] == 4
        and v3["scoring"]["scored_label_contract"]["version"] == 3
        and all("gold_gap" in run["metrics"] for run in v3["runs"])
    )
    if not all(result.values()):
        raise record.RecordError(f"capture record proof failed: {result}")
    result["ignored_run_fields_differ"] = ignored
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("record", type=Path)
    parser.add_argument("--v1", type=Path, required=True)
    parser.add_argument("--v2", type=Path, required=True)
    parser.add_argument("--v2-contract", type=Path, required=True)
    parser.add_argument("--v3-contract", type=Path, required=True)
    parser.add_argument("--capture", action="store_true",
                        help="record captured later with today's harness")
    parser.add_argument("--ignore-run-field", action="append", default=[],
                        help="with --capture: a harness-computed run field to skip (reported)")
    args = parser.parse_args()
    if args.ignore_run_field and not args.capture:
        parser.error("--ignore-run-field needs --capture")
    if args.capture:
        result = verify_capture(args.record, args.v1, args.v2, args.v2_contract,
                                args.v3_contract, args.ignore_run_field)
    else:
        result = verify(args.record, args.v1, args.v2, args.v2_contract, args.v3_contract)
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
