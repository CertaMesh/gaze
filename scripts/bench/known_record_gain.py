#!/usr/bin/env python3
"""Summarize class-by-match-kind gains from paired known-record oracle arms."""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path

LAYERS = ("C", "A", "D", "R")
CONTRACTS = ("v2", "v1")
MATCH_KINDS = (
    "exact", "whitespace_flexible", "case_folded",
    "whitespace_case_folded", "corroborated_single",
)


def table(v2: dict, v1: dict) -> dict:
    inputs = {"v2": v2, "v1": v1}
    for contract, arm in inputs.items():
        if arm.get("contract") != contract or arm.get("full") is not True:
            raise ValueError(f"{contract}: expected a full {contract} oracle arm")
        if set(arm.get("layers", {})) != set(LAYERS):
            raise ValueError(f"{contract}: missing oracle layer")
    for key in ("source_commit", "policy_sha256", "dataset_sha256"):
        if v2.get(key) != v1.get(key):
            raise ValueError(f"oracle arms differ in {key}")

    rows: dict[tuple[str, str], dict] = defaultdict(dict)
    for contract, arm in inputs.items():
        for layer in LAYERS:
            attribution = arm["layers"][layer]["attribution"]
            if attribution.get("schema_version") != 1:
                raise ValueError(f"{contract} {layer}: unsupported attribution schema")
            for item in attribution["rows"]:
                if item["population"] not in {"decoy", "non_decoy"}:
                    raise ValueError(f"{contract} {layer}: unknown population")
                key = (item["record_class"], item["match_kind"])
                layer_row = rows[key].setdefault(contract, {}).setdefault(layer, {
                    "gold_recovered_bytes": 0,
                    "gold_lost_bytes": 0,
                    "false_positive_added_bytes": 0,
                    "false_positive_removed_bytes": 0,
                    "decoy_false_positive_added_bytes": 0,
                    "non_decoy_false_positive_added_bytes": 0,
                })
                for field in (
                    "gold_recovered_bytes", "gold_lost_bytes",
                    "false_positive_added_bytes", "false_positive_removed_bytes",
                ):
                    layer_row[field] += item[field]
                layer_row[f"{item['population']}_false_positive_added_bytes"] += item[
                    "false_positive_added_bytes"
                ]

    output_rows = []
    for (record_class, match_kind), by_contract in sorted(rows.items()):
        contract_rows = {}
        for contract in CONTRACTS:
            by_layer = by_contract.get(contract, {})
            leak_fall = sum(
                row["gold_recovered_bytes"] - row["gold_lost_bytes"]
                for row in by_layer.values()
            )
            fp_rise = sum(
                row["false_positive_added_bytes"] - row["false_positive_removed_bytes"]
                for row in by_layer.values()
            )
            contract_rows[contract] = {
                "leaked_gold_bytes_fall": leak_fall,
                "false_positive_bytes_rise": fp_rise,
                "gain_pass": leak_fall > fp_rise,
                "layers": by_layer,
            }
        output_rows.append({
            "record_class": record_class,
            "match_kind": match_kind,
            "default_gain_pass": (
                None if record_class == "unattributed" else all(
                    contract_rows[contract]["gain_pass"] for contract in CONTRACTS
                )
            ),
            "contracts": contract_rows,
        })
    observed_kinds = {
        row["match_kind"] for row in output_rows
        if row["record_class"] != "unattributed"
    }
    return {
        "schema_version": 1,
        "source_commit": v2["source_commit"],
        "policy_sha256": v2["policy_sha256"],
        "dataset_sha256": v2["dataset_sha256"],
        "rule": "leaked gold bytes fall > false-positive bytes rise under both contracts",
        "unmeasured_match_kinds": sorted(set(MATCH_KINDS) - observed_kinds),
        "rows": output_rows,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--v2", required=True, type=Path)
    parser.add_argument("--v1", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = table(json.loads(args.v2.read_text()), json.loads(args.v1.read_text()))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
