#!/usr/bin/env python3
"""Summarize class-by-match-kind gains from paired known-record oracle arms."""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path

LAYERS = ("C", "A", "D", "R", "K")
CONTRACTS = ("v2", "v1")
MATCH_KINDS = (
    "exact", "whitespace_flexible", "case_folded",
    "whitespace_case_folded", "corroborated_single",
)
NAME_MULTI_KINDS = (
    "exact", "case_folded", "whitespace_flexible", "whitespace_case_folded"
)


def empty_layer_row() -> dict[str, int]:
    return {
        "positive_spans": 0,
        "gold_recovered_bytes": 0,
        "gold_lost_bytes": 0,
        "false_positive_added_bytes": 0,
        "false_positive_removed_bytes": 0,
        "decoy_false_positive_added_bytes": 0,
        "non_decoy_false_positive_added_bytes": 0,
    }


def table(v2: dict, v1: dict) -> dict:
    inputs = {"v2": v2, "v1": v1}
    for contract, arm in inputs.items():
        if arm.get("contract") != contract or arm.get("full") is not True:
            raise ValueError(f"{contract}: expected a full {contract} oracle arm")
        if set(arm.get("layers", {})) != set(LAYERS):
            raise ValueError(f"{contract}: missing oracle layer")
    for key in ("source_commit", "policy_sha256", "dataset_sha256", "kind_cells_manifest"):
        if v2.get(key) != v1.get(key):
            raise ValueError(f"oracle arms differ in {key}")
    if not isinstance(v2.get("kind_cells_manifest"), dict) or not v2["kind_cells_manifest"].get("pairs"):
        raise ValueError("oracle arms have no layer K manifest")
    if any(arm.get("name_multi_measurement_kinds") != list(NAME_MULTI_KINDS) for arm in inputs.values()):
        raise ValueError("oracle arms did not opt into every name_multi measurement kind")

    rows: dict[tuple[str, str], dict] = defaultdict(dict)
    for kind in NAME_MULTI_KINDS:
        rows[("name_multi", kind)] = {}
    for contract, arm in inputs.items():
        for layer in LAYERS:
            if layer == "K":
                cells = arm["layers"][layer].get("kind_cells")
                if not isinstance(cells, dict) or cells.get("schema_version") != 1 or not cells.get("rows"):
                    raise ValueError(f"{contract} K: missing scored kind cells")
            positives = arm["layers"][layer].get("name_multi_positive_spans_by_kind")
            if not isinstance(positives, dict) or set(positives) != set(NAME_MULTI_KINDS):
                raise ValueError(f"{contract} {layer}: missing name_multi positive input counts")
            if any(type(value) is not int or value < 0 for value in positives.values()):
                raise ValueError(f"{contract} {layer}: invalid name_multi positive input counts")
            for kind in NAME_MULTI_KINDS:
                layer_row = rows[("name_multi", kind)].setdefault(contract, {}).setdefault(
                    layer, empty_layer_row()
                )
                layer_row["positive_spans"] = positives[kind]
            attribution = arm["layers"][layer]["attribution"]
            if attribution.get("schema_version") != 1:
                raise ValueError(f"{contract} {layer}: unsupported attribution schema")
            for item in attribution["rows"]:
                if item["population"] not in {"decoy", "non_decoy"}:
                    raise ValueError(f"{contract} {layer}: unknown population")
                key = (item["record_class"], item["match_kind"])
                layer_row = rows[key].setdefault(contract, {}).setdefault(
                    layer, empty_layer_row()
                )
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
            positive_spans = sum(row["positive_spans"] for row in by_layer.values())
            leak_fall = sum(
                row["gold_recovered_bytes"] - row["gold_lost_bytes"]
                for row in by_layer.values()
            )
            fp_rise = sum(
                row["false_positive_added_bytes"] - row["false_positive_removed_bytes"]
                for row in by_layer.values()
            )
            contract_rows[contract] = {
                "positive_spans": positive_spans,
                "leaked_gold_bytes_fall": leak_fall,
                "false_positive_bytes_rise": fp_rise,
                "gain_pass": (
                    None if record_class == "name_multi" and positive_spans == 0
                    else leak_fall > fp_rise
                ),
                "layers": by_layer,
            }
        gain_results = [contract_rows[contract]["gain_pass"] for contract in CONTRACTS]
        output_rows.append({
            "record_class": record_class,
            "match_kind": match_kind,
            "default_gain_pass": (
                None if record_class == "unattributed" or None in gain_results
                else all(gain_results)
            ),
            "contracts": contract_rows,
        })
    observed_kinds = {
        row["match_kind"] for row in output_rows
        if row["record_class"] != "unattributed"
        if row["record_class"] != "name_multi"
        or any(row["contracts"][contract]["positive_spans"] for contract in CONTRACTS)
    }
    name_multi_by_kind = {
        row["match_kind"]: row for row in output_rows
        if row["record_class"] == "name_multi"
    }
    unmeasured_name_multi = [
        kind for kind in NAME_MULTI_KINDS
        if name_multi_by_kind[kind]["default_gain_pass"] is None
    ]
    return {
        "schema_version": 1,
        "source_commit": v2["source_commit"],
        "policy_sha256": v2["policy_sha256"],
        "dataset_sha256": v2["dataset_sha256"],
        "rule": "leaked gold bytes fall > false-positive bytes rise under both contracts",
        "unmeasured_match_kinds": sorted(set(MATCH_KINDS) - observed_kinds),
        "unmeasured_name_multi_kinds": unmeasured_name_multi,
        "rows": output_rows,
    }


def format_name_multi_rows(result: dict) -> str:
    lines = [
        "name_multi gain evidence (positive spans are input rows, not recovered bytes)",
        "contract layer record_class match_kind positive_spans leaked_gold_bytes_fall false_positive_bytes_rise proposed_default",
    ]
    by_kind = {
        row["match_kind"]: row for row in result["rows"]
        if row["record_class"] == "name_multi"
    }
    for contract in CONTRACTS:
        for layer in LAYERS:
            for kind in NAME_MULTI_KINDS:
                row = by_kind[kind]["contracts"][contract]["layers"][layer]
                decision = by_kind[kind]["default_gain_pass"]
                proposed = "unmeasured" if decision is None else "on" if decision else "off"
                lines.append(
                    f"{contract} {layer} name_multi {kind} {row['positive_spans']} "
                    f"{row['gold_recovered_bytes'] - row['gold_lost_bytes']} "
                    f"{row['false_positive_added_bytes'] - row['false_positive_removed_bytes']} "
                    f"{proposed}"
                )
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--v2", required=True, type=Path)
    parser.add_argument("--v1", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = table(json.loads(args.v2.read_text()), json.loads(args.v1.read_text()))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(format_name_multi_rows(result))
    if result["unmeasured_name_multi_kinds"]:
        raise SystemExit("unmeasured name_multi kinds: " + ", ".join(result["unmeasured_name_multi_kinds"]))


if __name__ == "__main__":
    main()
