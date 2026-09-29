"""The class gate counts D false positives and requires both contracts."""

import known_record_gain as gain


def arm(contract: str, *, gold: int, fp: int, layer: str = "C", population: str = "non_decoy") -> dict:
    layers = {name: {"attribution": {"schema_version": 1, "rows": []}} for name in gain.LAYERS}
    layers[layer]["attribution"]["rows"] = [{
        "record_class": "name_single", "match_kind": "exact", "population": population,
        "gold_recovered_bytes": gold, "gold_lost_bytes": 0,
        "false_positive_added_bytes": fp, "false_positive_removed_bytes": 0,
    }]
    return {
        "contract": contract, "full": True, "layers": layers,
        "source_commit": "abc", "policy_sha256": "policy", "dataset_sha256": "corpus",
    }


def test_both_contracts_must_outweigh_false_positives() -> None:
    v2, v1 = arm("v2", gold=10, fp=0), arm("v1", gold=10, fp=0)
    v2["layers"]["D"]["attribution"]["rows"] = arm(
        "v2", gold=0, fp=9, layer="D", population="decoy"
    )["layers"]["D"]["attribution"]["rows"]
    v1["layers"]["D"]["attribution"]["rows"] = arm(
        "v1", gold=0, fp=11, layer="D", population="decoy"
    )["layers"]["D"]["attribution"]["rows"]
    result = gain.table(v2, v1)["rows"][0]
    assert result["contracts"]["v2"]["gain_pass"]
    assert not result["contracts"]["v1"]["gain_pass"]
    assert not result["default_gain_pass"]
    assert result["contracts"]["v2"]["layers"]["D"]["decoy_false_positive_added_bytes"] == 9


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
    assert result["rows"][0]["default_gain_pass"] is None
    assert "whitespace_flexible" in result["unmeasured_match_kinds"]
