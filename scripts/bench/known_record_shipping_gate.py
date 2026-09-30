#!/usr/bin/env python3
"""Gate paired caller-known-record arms using product match-kind defaults."""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path

LAYERS = ("C", "A", "D", "R", "K")


def empty_attribution() -> dict[str, int]:
    return {
        "leaked_gold_bytes_fall": 0,
        "false_positive_bytes_rise": 0,
        "decoy_false_positive_bytes_added": 0,
    }


def class_kind_rows(arms: dict[str, dict]) -> list[dict]:
    metrics = defaultdict(lambda: defaultdict(lambda: defaultdict(empty_attribution)))
    for contract, arm in arms.items():
        for layer in LAYERS:
            attribution = arm["layers"][layer].get("attribution")
            if not isinstance(attribution, dict) or attribution.get("schema_version") != 1:
                raise ValueError(f"{contract} {layer}: missing class-kind attribution")
            for item in attribution["rows"]:
                row = metrics[(item["record_class"], item["match_kind"])][contract][layer]
                row["leaked_gold_bytes_fall"] += (
                    item["gold_recovered_bytes"] - item["gold_lost_bytes"]
                )
                row["false_positive_bytes_rise"] += (
                    item["false_positive_added_bytes"] - item["false_positive_removed_bytes"]
                )
                if item["population"] == "decoy":
                    row["decoy_false_positive_bytes_added"] += item["false_positive_added_bytes"]
    return [
        {
            "record_class": record_class,
            "match_kind": match_kind,
            "contracts": {
                contract: {
                    "layers": {
                        layer: by_contract.get(contract, {}).get(layer, empty_attribution())
                        for layer in LAYERS
                    },
                    "total_leaked_gold_bytes_fall": sum(
                        row["leaked_gold_bytes_fall"]
                        for row in by_contract.get(contract, {}).values()
                    ),
                    "total_false_positive_bytes_rise": sum(
                        row["false_positive_bytes_rise"]
                        for row in by_contract.get(contract, {}).values()
                    ),
                }
                for contract in ("v2", "v1")
            },
        }
        for (record_class, match_kind), by_contract in sorted(metrics.items())
    ]


def gate(v2: dict, v1: dict, main_v2: dict, main_v1: dict) -> dict:
    arms = {"v2": v2, "v1": v1}
    main_arms = {"v2": main_v2, "v1": main_v1}
    for contract, arm in arms.items():
        if arm.get("contract") != contract or arm.get("full") is not True:
            raise ValueError(f"{contract}: expected a full {contract} arm")
        if arm.get("record_match_kinds_mode") != "shipping_defaults":
            raise ValueError(f"{contract}: product defaults were not measured")
        if set(arm.get("layers", {})) != set(LAYERS):
            raise ValueError(f"{contract}: missing oracle layer")
        main_arm = main_arms[contract]
        if main_arm.get("contract") != contract or main_arm.get("full") is not True:
            raise ValueError(f"main {contract}: expected a full {contract} arm")
        if main_arm.get("record_match_kinds_mode") != "baseline_only":
            raise ValueError(f"main {contract}: no-record baseline was not measured")
        if set(main_arm.get("layers", {})) != set(LAYERS):
            raise ValueError(f"main {contract}: missing oracle layer")
        for key in ("source_commit", "policy_sha256", "dataset_sha256", "kind_cells_manifest"):
            if arm.get(key) != main_arm.get(key):
                raise ValueError(f"{contract}: main and candidate differ in {key}")
    for key in ("source_commit", "policy_sha256", "dataset_sha256", "kind_cells_manifest"):
        if v2.get(key) != v1.get(key):
            raise ValueError(f"oracle arms differ in {key}")
    if not main_v2.get("binary_source_commit") or main_v2["binary_source_commit"] != main_v1.get("binary_source_commit"):
        raise ValueError("main binary commit differs across contracts")

    results = {}
    for contract, arm in arms.items():
        layer_results = {}
        for layer in LAYERS:
            layer_arm = arm["layers"][layer]
            if not isinstance(layer_arm.get("documents"), int):
                raise ValueError(f"{contract} {layer}: missing scored documents")
            base = layer_arm["baseline"]
            record = layer_arm["with_record"]
            main_layer = main_arms[contract]["layers"][layer]
            main_baseline_match = (
                main_layer.get("documents") == layer_arm["documents"]
                and main_layer.get("baseline") == base
            )
            base_bytes = base["metrics"]["utf8_bytes"]
            record_bytes = record["metrics"]["utf8_bytes"]
            base_availability = base["pipeline_availability"]
            record_availability = record["pipeline_availability"]
            base_contract = base["pipeline_contract"]
            record_contract = record["pipeline_contract"]
            documents = layer_arm["documents"]
            if any(
                value["attempted_documents"] != documents
                for value in (base_availability, record_availability)
            ):
                raise ValueError(f"{contract} {layer}: mismatched document count")
            leaked_fall = base_bytes["leaked"] - record_bytes["leaked"]
            fp_rise = record_bytes["false_positive"] - base_bytes["false_positive"]
            refused_rise = (
                record_availability["failed_closed_documents"]
                - base_availability["failed_closed_documents"]
            )
            incomplete_rise = (
                base_availability["completed_documents"]
                - record_availability["completed_documents"]
            )
            restore_fall = (
                base_contract["restore_exact_documents"]
                - record_contract["restore_exact_documents"]
            )
            manifest_fall = (
                base_contract["manifest_valid_documents"]
                - record_contract["manifest_valid_documents"]
            )
            passed = main_baseline_match and all(
                value <= 0
                for value in (-leaked_fall, refused_rise, incomplete_rise, restore_fall, manifest_fall)
            )
            layer_results[layer] = {
                "documents": documents,
                "no_record_matches_main": main_baseline_match,
                "baseline_leaked_bytes": base_bytes["leaked"],
                "with_record_leaked_bytes": record_bytes["leaked"],
                "leaked_bytes_fall": leaked_fall,
                "baseline_false_positive_bytes": base_bytes["false_positive"],
                "with_record_false_positive_bytes": record_bytes["false_positive"],
                "false_positive_bytes_rise": fp_rise,
                "refused_documents_rise": refused_rise,
                "incomplete_documents_rise": incomplete_rise,
                "exact_restore_documents_fall": restore_fall,
                "valid_manifest_documents_fall": manifest_fall,
                "pass": passed,
            }
        leaked_fall = sum(row["leaked_bytes_fall"] for row in layer_results.values())
        fp_rise = sum(row["false_positive_bytes_rise"] for row in layer_results.values())
        results[contract] = {
            "layers": layer_results,
            "total_leaked_bytes_fall": leaked_fall,
            "total_false_positive_bytes_rise": fp_rise,
            "pass": (
                all(row["pass"] for row in layer_results.values())
                and leaked_fall > 0
                and fp_rise < leaked_fall
            ),
        }
    return {
        "schema_version": 1,
        "source_commit": v2["source_commit"],
        "main_binary_source_commit": main_v2["binary_source_commit"],
        "policy_sha256": v2["policy_sha256"],
        "dataset_sha256": v2["dataset_sha256"],
        "class_kind_rows": class_kind_rows(arms),
        "contracts": results,
        "pass": all(result["pass"] for result in results.values()),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--v2", type=Path, required=True)
    parser.add_argument("--v1", type=Path, required=True)
    parser.add_argument("--main-v2", type=Path, required=True)
    parser.add_argument("--main-v1", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = gate(
        json.loads(args.v2.read_text()), json.loads(args.v1.read_text()),
        json.loads(args.main_v2.read_text()), json.loads(args.main_v1.read_text()),
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    for contract, data in result["contracts"].items():
        print(f"{contract}: leaked fall {data['total_leaked_bytes_fall']}; FP rise {data['total_false_positive_bytes_rise']}; pass {data['pass']}")
        for layer, row in data["layers"].items():
            print(f"  {layer}: no-record main parity {row['no_record_matches_main']}; leaked {row['baseline_leaked_bytes']} -> {row['with_record_leaked_bytes']}; FP {row['baseline_false_positive_bytes']} -> {row['with_record_false_positive_bytes']}; refused rise {row['refused_documents_rise']}; pass {row['pass']}")
    if not result["pass"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
