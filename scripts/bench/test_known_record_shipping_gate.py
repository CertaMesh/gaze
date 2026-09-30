"""The shipping gate rejects regressions hidden by a net-positive total."""

import copy
import json
from pathlib import Path

import known_record_shipping_gate as shipping


def arm(contract: str) -> dict:
    def score(leaked: int, fp: int) -> dict:
        return {
            "metrics": {"utf8_bytes": {"leaked": leaked, "false_positive": fp}},
            "pipeline_availability": {
                "attempted_documents": 1, "completed_documents": 1,
                "failed_closed_documents": 0,
            },
            "pipeline_contract": {"restore_exact_documents": 1, "manifest_valid_documents": 1},
        }

    return {
        "contract": contract, "full": True,
        "record_match_kinds_mode": "shipping_defaults",
        "source_commit": "source", "binary_source_commit": "source",
        "binary_sha256": "candidate-binary", "policy_sha256": "policy",
        "dataset_sha256": "data", "kind_cells_manifest": {"pairs": 1},
        "layers": {
            layer: {
                "documents": 1, "baseline": score(10, 0), "with_record": score(0, 0),
                "attribution": {"schema_version": 1, "rows": [], "totals": {
                    "false_positive_added_bytes": 0, "false_positive_removed_bytes": 0,
                }},
            }
            for layer in shipping.LAYERS
        },
    }


def main_arm(candidate: dict) -> dict:
    value = copy.deepcopy(candidate)
    value["record_match_kinds_mode"] = "baseline_only"
    value["binary_source_commit"] = "main"
    value["binary_sha256"] = "main-binary"
    for layer in value["layers"].values():
        del layer["with_record"]
    return value


def test_shipping_gate_needs_two_full_product_default_arms() -> None:
    v2, v1 = arm("v2"), arm("v1")
    main_v2, main_v1 = main_arm(v2), main_arm(v1)
    assert shipping.gate(v2, v1, main_v2, main_v1)["pass"]
    v1["record_match_kinds_mode"] = "probe_all"
    try:
        shipping.gate(v2, v1, main_v2, main_v1)
    except ValueError as error:
        assert "product defaults" in str(error)
    else:
        raise AssertionError("probe-all arm passed the shipping gate")


def test_shipping_gate_vetoes_layer_leak_and_restore_regressions() -> None:
    v2, v1 = arm("v2"), arm("v1")
    main_v2, main_v1 = main_arm(v2), main_arm(v1)
    v1["layers"]["D"]["with_record"]["metrics"]["utf8_bytes"]["leaked"] = 11
    assert not shipping.gate(v2, v1, main_v2, main_v1)["pass"]
    v1 = arm("v1")
    v1["layers"]["C"]["with_record"]["pipeline_contract"]["restore_exact_documents"] = 0
    assert not shipping.gate(v2, v1, main_v2, main_v1)["pass"]


def test_shipping_gate_rejects_false_positive_cost_above_gain() -> None:
    v2, v1 = arm("v2"), arm("v1")
    main_v2, main_v1 = main_arm(v2), main_arm(v1)
    for value in (v2, v1):
        value["layers"]["C"]["with_record"]["metrics"]["utf8_bytes"]["false_positive"] = 51
    result = shipping.gate(v2, v1, main_v2, main_v1)
    assert result["contracts"]["v2"]["total_false_positive_bytes_rise"] > result["contracts"]["v2"]["total_leaked_bytes_fall"]
    assert not result["pass"]


def test_shipping_gate_reports_class_kind_and_decoy_cost() -> None:
    v2, v1 = arm("v2"), arm("v1")
    main_v2, main_v1 = main_arm(v2), main_arm(v1)
    for value in (v2, v1):
        value["layers"]["D"]["with_record"]["metrics"]["utf8_bytes"]["false_positive"] = 1
        value["layers"]["D"]["attribution"]["totals"]["false_positive_added_bytes"] = 1
        value["layers"]["D"]["attribution"]["rows"] = [{
            "record_class": "custom:phone", "match_kind": "exact",
            "population": "decoy", "gold_recovered_bytes": 0,
            "gold_lost_bytes": 0, "false_positive_added_bytes": 1,
            "false_positive_removed_bytes": 0,
        }]
    result = shipping.gate(v2, v1, main_v2, main_v1)
    row = result["class_kind_rows"][0]
    assert (row["record_class"], row["match_kind"]) == ("custom:phone", "exact")
    assert row["contracts"]["v2"]["layers"]["D"]["decoy_false_positive_bytes_added"] == 1
    assert row["contracts"]["v2"]["layers"]["C"] == shipping.empty_attribution()


def test_shipping_gate_vetoes_non_exempt_layer_d_fp_even_with_net_gain() -> None:
    root = Path(__file__).resolve().parents[2] / "docs/reference/benchmarks"
    originals = [
        json.loads((root / f"known-record-shipping-{name}.json").read_text())
        for name in ("v2", "v1", "main-v2", "main-v1")
    ]
    assert shipping.gate(*originals)["pass"]
    for record_class, match_kind in [
        ("name_multi", "case_folded"),
        ("trace_gap", "unattributed"),
    ]:
        arms = copy.deepcopy(originals)
        layer = arms[0]["layers"]["D"]
        layer["with_record"]["metrics"]["utf8_bytes"]["false_positive"] += 1
        layer["attribution"]["totals"]["false_positive_added_bytes"] += 1
        layer["attribution"]["rows"].append({
            "record_class": record_class, "match_kind": match_kind,
            "population": "decoy", "gold_recovered_bytes": 0,
            "gold_lost_bytes": 0, "false_positive_added_bytes": 1,
            "false_positive_removed_bytes": 0,
        })
        assert not shipping.gate(*arms)["contracts"]["v2"]["layers"]["D"]["pass"]


def test_shipping_gate_keeps_only_declared_layer_d_fp_exemptions() -> None:
    for record_class, match_kind in shipping.D_FP_EXEMPTIONS:
        v2, v1 = arm("v2"), arm("v1")
        main_v2, main_v1 = main_arm(v2), main_arm(v1)
        for value in (v2, v1):
            layer = value["layers"]["D"]
            layer["with_record"]["metrics"]["utf8_bytes"]["false_positive"] = 1
            layer["attribution"]["totals"]["false_positive_added_bytes"] = 1
            layer["attribution"]["rows"] = [{
                "record_class": record_class, "match_kind": match_kind,
                "population": "decoy", "gold_recovered_bytes": 0,
                "gold_lost_bytes": 0, "false_positive_added_bytes": 1,
                "false_positive_removed_bytes": 0,
            }]
        assert shipping.gate(v2, v1, main_v2, main_v1)["pass"]


def test_shipping_gate_requires_exact_no_record_main_parity() -> None:
    v2, v1 = arm("v2"), arm("v1")
    main_v2, main_v1 = main_arm(v2), main_arm(v1)
    main_v2["layers"]["K"]["baseline"]["metrics"]["utf8_bytes"]["leaked"] = 9
    result = shipping.gate(v2, v1, main_v2, main_v1)
    assert not result["contracts"]["v2"]["layers"]["K"]["no_record_matches_main"]
    assert not result["pass"]


def test_shipping_gate_rejects_binary_change_between_contracts() -> None:
    v2, v1 = arm("v2"), arm("v1")
    main_v2, main_v1 = main_arm(v2), main_arm(v1)
    main_v1["binary_sha256"] = "different"
    try:
        shipping.gate(v2, v1, main_v2, main_v1)
    except ValueError as error:
        assert "main binary differs" in str(error)
    else:
        raise AssertionError("mixed main binaries passed the shipping gate")
