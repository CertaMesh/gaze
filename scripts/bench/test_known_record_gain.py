"""The class gate counts D false positives and requires both contracts."""

import json
import sys

import pytest

import known_record_gain as gain


def arm(contract: str, *, gold: int, fp: int, layer: str = "C", population: str = "non_decoy") -> dict:
    layers = {
        name: {
            "attribution": {"schema_version": 1, "rows": []},
            "name_multi_positive_spans_by_kind": {kind: 0 for kind in gain.NAME_MULTI_KINDS},
        }
        for name in gain.LAYERS
    }
    layers["K"]["kind_cells"] = {"schema_version": 1, "rows": [{"bucket": "synthetic"}]}
    layers[layer]["attribution"]["rows"] = [{
        "record_class": "name_single", "match_kind": "exact", "population": population,
        "gold_recovered_bytes": gold, "gold_lost_bytes": 0,
        "false_positive_added_bytes": fp, "false_positive_removed_bytes": 0,
    }]
    return {
        "contract": contract, "full": True, "layers": layers,
        "source_commit": "abc", "policy_sha256": "policy", "dataset_sha256": "corpus",
        "kind_cells_manifest": {"pairs": 1, "corpus_sha256": "synthetic"},
        "name_multi_measurement_kinds": list(gain.NAME_MULTI_KINDS),
    }


def test_both_contracts_must_outweigh_false_positives() -> None:
    v2, v1 = arm("v2", gold=10, fp=0), arm("v1", gold=10, fp=0)
    v2["layers"]["D"]["attribution"]["rows"] = arm(
        "v2", gold=0, fp=9, layer="D", population="decoy"
    )["layers"]["D"]["attribution"]["rows"]
    v1["layers"]["D"]["attribution"]["rows"] = arm(
        "v1", gold=0, fp=11, layer="D", population="decoy"
    )["layers"]["D"]["attribution"]["rows"]
    result = next(row for row in gain.table(v2, v1)["rows"] if row["record_class"] == "name_single")
    assert result["contracts"]["v2"]["gain_pass"]
    assert not result["contracts"]["v1"]["gain_pass"]
    assert not result["default_gain_pass"]
    assert result["contracts"]["v2"]["layers"]["D"]["decoy_false_positive_added_bytes"] == 9


def test_layer_d_decoy_veto_has_narrow_declared_value_exceptions() -> None:
    for record_class, match_kind, vetoed in (
        ("address_part", "exact", True),
        ("custom:credit_card", "exact", False),
        ("custom:credit_card", "whitespace_flexible", True),
        ("custom:phone", "exact", False),
        ("custom:iban", "exact", False),
    ):
        v2, v1 = arm("v2", gold=100, fp=0), arm("v1", gold=100, fp=0)
        for source in (v2, v1):
            source["layers"]["C"]["attribution"]["rows"][0]["record_class"] = record_class
            source["layers"]["C"]["attribution"]["rows"][0]["match_kind"] = match_kind
            source["layers"]["D"]["attribution"]["rows"] = [{
                "record_class": record_class, "match_kind": match_kind, "population": "decoy",
                "gold_recovered_bytes": 0, "gold_lost_bytes": 0,
                "false_positive_added_bytes": 1, "false_positive_removed_bytes": 0,
            }]
        result = next(row for row in gain.table(v2, v1)["rows"] if row["record_class"] == record_class)
        assert result["contracts"]["v2"]["gain_pass"] is True
        assert result["d_counterweight_veto"] is vetoed
        assert result["default_gain_pass"] is not vetoed


def test_mismatched_measurement_basis_fails() -> None:
    v2, v1 = arm("v2", gold=1, fp=0), arm("v1", gold=1, fp=0)
    v1["dataset_sha256"] = "other"
    try:
        gain.table(v2, v1)
    except ValueError as error:
        assert "dataset_sha256" in str(error)
    else:
        raise AssertionError("mismatched corpus was accepted")


def test_unattributed_rows_are_not_policy_decisions() -> None:
    v2, v1 = arm("v2", gold=1, fp=0), arm("v1", gold=1, fp=0)
    for value in (v2, v1):
        value["layers"]["C"]["attribution"]["rows"][0]["record_class"] = "unattributed"
    result = gain.table(v2, v1)
    assert next(row for row in result["rows"] if row["record_class"] == "unattributed")["default_gain_pass"] is None
    assert "whitespace_flexible" in result["unmeasured_match_kinds"]


def test_name_multi_rows_print_each_layer_and_require_positive_inputs() -> None:
    v2, v1 = arm("v2", gold=1, fp=0), arm("v1", gold=1, fp=0)
    for source in (v2, v1):
        source["layers"]["K"]["name_multi_positive_spans_by_kind"] = {
            kind: 1 for kind in gain.NAME_MULTI_KINDS
        }
        source["layers"]["K"]["attribution"]["rows"] = [
            {
                "record_class": "name_multi", "match_kind": kind,
                "population": "non_decoy", "gold_recovered_bytes": 10,
                "gold_lost_bytes": 0, "false_positive_added_bytes": 2,
                "false_positive_removed_bytes": 0,
            }
            for kind in gain.NAME_MULTI_KINDS
        ]
    result = gain.table(v2, v1)
    rows = [row for row in result["rows"] if row["record_class"] == "name_multi"]
    assert {row["match_kind"] for row in rows} == set(gain.NAME_MULTI_KINDS)
    assert all(row["default_gain_pass"] is True for row in rows)
    assert all(row["contracts"]["v2"]["layers"]["C"]["gold_recovered_bytes"] == 0 for row in rows)
    printed = gain.format_name_multi_rows(result)
    assert "v2 K name_multi exact 1 10 2 on" in printed
    assert "v1 C name_multi whitespace_case_folded 0 0 0 on" in printed
    assert len(printed.splitlines()) == 2 + 2 * len(gain.LAYERS) * len(gain.NAME_MULTI_KINDS)

    v1["layers"]["K"]["name_multi_positive_spans_by_kind"]["case_folded"] = 0
    blind = gain.table(v2, v1)
    case = next(row for row in blind["rows"] if row["record_class"] == "name_multi" and row["match_kind"] == "case_folded")
    assert case["default_gain_pass"] is None
    assert "case_folded" in blind["unmeasured_name_multi_kinds"]


def test_cli_prints_all_name_multi_rows_then_refuses_blind_gate(
    tmp_path, monkeypatch, capsys
) -> None:
    v2_path, v1_path, output = (tmp_path / name for name in ("v2.json", "v1.json", "gain.json"))
    v2_path.write_text(json.dumps(arm("v2", gold=1, fp=0)))
    v1_path.write_text(json.dumps(arm("v1", gold=1, fp=0)))
    monkeypatch.setattr(sys, "argv", [
        "known_record_gain.py", "--v2", str(v2_path), "--v1", str(v1_path),
        "--output", str(output),
    ])
    with pytest.raises(SystemExit, match="unmeasured name_multi kinds"):
        gain.main()
    assert "v2 K name_multi exact 0 0 0" in capsys.readouterr().out
    assert json.loads(output.read_text())["unmeasured_name_multi_kinds"] == list(gain.NAME_MULTI_KINDS)
