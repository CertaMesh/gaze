"""Structural reuse proofs, without inference or unsigned fixture commits."""
import copy
import json

import pytest
import mcp_text_path_identity as proof


def test_current_tree_proof_has_real_per_file_hashes_and_replays():
    record = proof.build("HEAD", "HEAD")
    assert record["protected_file_count"] > 100
    assert all(row["base_sha256"] == row["candidate_sha256"] and len(row["base_sha256"]) == 64 for row in record["protected_files"])
    assert any(row["path"] == "crates/gaze/src/pipeline/protection.rs" for row in record["protected_files"])
    assert any(row["path"] == "scripts/bench/run_no_opf_benchmark.py" for row in record["protected_files"])
    assert not record["fresh_text_capture"]
    proof.verify(record)
    changed = copy.deepcopy(record)
    changed["protected_files"][0]["candidate_sha256"] = "0" * 64
    with pytest.raises(ValueError, match="record differs"):
        proof.verify(changed)


def test_protected_change_is_rejected_before_hashing(monkeypatch):
    actual = proof._tree
    calls = 0
    def altered(revision):
        nonlocal calls
        tree = actual(revision)
        calls += 1
        if calls == 2:
            path = "crates/gaze/src/pipeline/protection.rs"
            mode, kind, _ = tree[path]
            tree[path] = (mode, kind, "0" * 40)
        return tree
    monkeypatch.setattr(proof, "_tree", altered)
    with pytest.raises(ValueError, match="text-path or unclassified"):
        proof.build("HEAD", "HEAD")


def test_rmcp_counter_exemption_cannot_hide_other_fixture_changes():
    before = {"receipt": {"counts": {"observer_recognizer_source_events": 1, "leaf_restore_exact": 2}}}
    after = copy.deepcopy(before)
    after["receipt"]["counts"]["observer_recognizer_source_events"] = 3
    proof._golden_only(json.dumps(before), json.dumps(after))
    after["receipt"]["counts"]["leaf_restore_exact"] = 1
    with pytest.raises(ValueError, match="more than"):
        proof._golden_only(json.dumps(before), json.dumps(after))
    assert not proof._allowed("crates/gaze/src/mcp_operation.rs")
    assert not proof._allowed("scripts/bench/unclassified_new_driver.py")


def test_ci_registry_exception_only_adds_native_evidence_paths():
    before = {"paths": ["existing.txt"], "gate_readers": {"gate": "unchanged"}}
    after = copy.deepcopy(before)
    after["paths"] = sorted(after["paths"] + [proof.EVIDENCE_PREFIX + name for name in proof.EVIDENCE_NAMES])
    proof._ci_registry_only(json.dumps(before), json.dumps(after))
    legacy = copy.deepcopy(after)
    legacy["paths"].remove(proof.EVIDENCE_PREFIX + "whitespace-diagnostic.json")
    proof._ci_registry_only(json.dumps(before), json.dumps(legacy))
    partial = copy.deepcopy(after)
    partial["paths"].remove(proof.EVIDENCE_PREFIX + "gate.json")
    with pytest.raises(ValueError, match="beyond"):
        proof._ci_registry_only(json.dumps(before), json.dumps(partial))
    after["gate_readers"]["gate"] = "changed"
    with pytest.raises(ValueError, match="beyond"):
        proof._ci_registry_only(json.dumps(before), json.dumps(after))


def test_documentation_exceptions_are_exact_and_recorded():
    assert proof.DOCUMENTATION_FILES == {"CHANGELOG.md", "docs/explanation/mcp/mcp-runtime.md", "crates/gaze-mcp-rmcp/README.md"}
    assert all(proof._allowed(path) for path in proof.DOCUMENTATION_FILES)
    assert not proof._allowed("docs/reference/benchmarks/README.md")
    assert not proof._allowed("docs/unclassified.md")
    record = proof.build("HEAD", "HEAD")
    assert record["documentation_changes"] == []
    record["documentation_changes"] = [{"path": "CHANGELOG.md", "candidate_sha256": "0" * 64}]
    with pytest.raises(ValueError, match="record differs: documentation_changes"):
        proof.verify(record)


def test_rmcp_transport_exception_only_admits_resource_limit_arm_and_test():
    before = "runtime\n}\n"
    after = proof.RMCP_RESOURCE_ARM + "runtime\n" + proof.RMCP_RESOURCE_TEST + "}\n"
    proof._rmcp_resource_limit_only(before, after)
    with pytest.raises(ValueError, match="beyond"):
        proof._rmcp_resource_limit_only(before, after.replace("runtime", "changed"))
    with pytest.raises(ValueError, match="missing"):
        proof._rmcp_resource_limit_only(before, after.replace(proof.RMCP_RESOURCE_ARM, ""))
    record = proof.build("HEAD", "HEAD")
    record["transport_changes"] = [{"path": proof.RMCP_FRONTEND, "candidate_sha256": "0" * 64}]
    with pytest.raises(ValueError, match="record differs: transport_changes"):
        proof.verify(record)
