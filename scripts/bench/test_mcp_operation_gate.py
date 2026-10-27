"""Composition and admission guards. No models or inference run here."""
import copy
import hashlib

import pytest
import mcp_operation_gate as gate
import mcp_operation_order as arm


def totals():
    rows = {layer: {"attempted": 10, "documents": 10, "headline_leaked": 0, "leaked": 0,
                    "twin_leaked": 0, "false_positive": 0, "failed_closed": 0, "restore_exact": 10,
                    "manifest_valid": 10, "guard_false_positive": {}} for layer in ("C", "A", "D", "R")}
    rows["D"]["guard_false_positive"] = {family: 0 for families in gate.layers.CREDIT_GUARD_FAMILIES.values() for family in families}
    return rows


def native():
    policy = '[ner]\nmodel_dir="/synthetic/ner"\nthreshold=0.3\n'
    return {"runtime_revision": "synthetic", "policy_document": policy,
            "policy_sha256": hashlib.sha256(policy.encode()).hexdigest(),
            "contracts": {key: value["identity"] for key, value in arm.contracts().items()},
            "model_files": {"GAZE_NER_MODEL_DIR": {"SHA256SUMS": "1" * 64}, "GAZE_NYM_MODEL_DIR": {"SHA256SUMS": "2" * 64}},
            "scores": {contract: {"cases": [{"id": "a", "layer": "A"}, {"id": "d", "layer": "D"}, {"id": "r", "layer": "R"}],
                                 "layers": {layer: {**dict.fromkeys(arm.FIELDS, 0), "restore_exact": 1, "manifest_valid": 1} for layer in ("A", "D", "R")}}
                       for contract in ("v1", "v2")}}


def core(contract="v1"):
    n = native()
    result = {"gaze": {"revision": n["runtime_revision"], "dirty": False},
              "parameters": {"policy_sha256": n["policy_sha256"], "ner_threshold": 0.3},
              "runner_provenance": {"model_bundles": [{"model_id": "davlan-mbert-ner-hrl-onnx", "observed_sha256": "1" * 64},
                                                     {"model_id": "nym-small-int8", "observed_sha256": "2" * 64}]}}
    if contract == "v2":
        result["scoring"] = {"scored_label_contract": {"id": "scored-labels-v2", "version": 2, "file_sha256": n["contracts"]["v2"]}}
    return result


def test_composition_adds_native_operations_without_changing_original_c_or_inputs():
    original, n = totals(), native()
    n["scores"]["v1"]["layers"]["R"]["leaked"] = 15
    n["scores"]["v1"]["layers"]["D"]["false_positive"] = 4
    before = copy.deepcopy(original)
    composed = gate._compose(original, n, "v1")
    assert composed["C"] == original["C"]
    assert composed["R"]["leaked"] == composed["R"]["headline_leaked"] == 15
    assert composed["R"]["attempted"] == composed["R"]["documents"] == 11
    assert composed["D"]["false_positive"] == 4
    assert original == before
    assert set(composed) == {"C", "A", "D", "R"}


def test_composed_totals_use_the_canonical_byte_gain_rule():
    n, candidate = native(), native()
    n["scores"]["v1"]["layers"]["R"]["leaked"] = 15
    candidate["scores"]["v1"]["layers"]["R"]["false_positive"] = 1
    result = gate.layers.decide(gate._compose(totals(), n, "v1"), gate._compose(totals(), candidate, "v1"))
    assert result["verdict"] == "pass"
    assert result["summary"] == {"leaked_bytes_decrease": 15, "false_positive_bytes_increase": 1}


def test_core_regression_cannot_hide_behind_native_gain():
    before, after = totals(), totals()
    after["C"]["leaked"] = after["C"]["headline_leaked"] = 1
    assert "C:leaked" in gate._core_regressions(before, after)
    after = totals()
    after["R"]["restore_exact"] -= 1
    assert "R:restore_exact" in gate._core_regressions(before, after)
    after = totals()
    family = next(iter(after["D"]["guard_false_positive"]))
    after["D"]["guard_false_positive"][family] = 1
    assert any("credit-guard" in item for item in gate._core_regressions(before, after))


def test_binding_requires_the_exact_clean_revision_policy_models_and_contract():
    for contract in ("v1", "v2"):
        gate._bind(core(contract), native(), contract)
    changed = core()
    changed["gaze"]["dirty"] = True
    with pytest.raises(ValueError, match="clean runtime"):
        gate._bind(changed, native(), "v1")
    changed = core()
    changed["parameters"]["policy_sha256"] = "different"
    with pytest.raises(ValueError, match="same setup policy"):
        gate._bind(changed, native(), "v1")
    changed = core()
    changed["parameters"]["ner_threshold"] = 0.5
    with pytest.raises(ValueError, match="thresholds"):
        gate._bind(changed, native(), "v1")
    changed = core()
    changed["runner_provenance"]["model_bundles"][0]["observed_sha256"] = "3" * 64
    with pytest.raises(ValueError, match="different model"):
        gate._bind(changed, native(), "v1")
    with pytest.raises(ValueError, match="wrong scored-label"):
        gate._bind(core("v2"), native(), "v1")


def test_text_execution_receipts_remain_mandatory_even_after_native_admission(monkeypatch, tmp_path):
    monkeypatch.setattr(gate.mcp, "compare", lambda *args: {key: {"regressions": []} for key in ("v1", "v2")})
    cards = {key: (core(key), core(key)) for key in ("v1", "v2")}
    paths = {key: (tmp_path / "base.json", tmp_path / "candidate.json") for key in ("v1", "v2")}
    with pytest.raises(gate.layers.LayerError):
        gate.compare(cards, paths, (native(), native()), paths["v1"])


def test_unchanged_text_mode_rejects_unbound_runtime_revisions_before_reuse():
    with pytest.raises(ValueError, match="bind both measured"):
        gate.compare_unchanged_text({"base_revision": "base", "candidate_revision": "candidate"},
                                    (native(), native()), (), None)
