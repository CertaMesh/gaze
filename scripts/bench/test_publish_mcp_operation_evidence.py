"""Public metadata redaction must not alter the synthetic observations."""
import copy
import hashlib

import pytest
import publish_mcp_operation_evidence as publish


def capture():
    policy = '[ner]\nmodel_dir="/Users/private/ner"\nthreshold=0.3\n[safety_net.nym]\nmodel_dir="/Users/private/nym"\n'
    return {"policy_document": policy, "policy_sha256": hashlib.sha256(policy.encode()).hexdigest(),
            "host_build": {"command": ["cargo", "build", "--manifest-path", "/Users/private/host/Cargo.toml"]},
            "records": [{"synthetic": "public fixture payload", "snapshot_hex": "00"}], "scores": {"v1": {"leaked": 1}}}


def test_publication_removes_private_paths_and_keeps_observations_and_actual_policy_digest():
    original = capture()
    before = copy.deepcopy(original)
    public = publish.public_capture(original)
    assert public["records"] == original["records"]
    assert public["scores"] == original["scores"]
    assert public["policy_sha256"] == original["policy_sha256"]
    assert "policy_document" not in public
    assert "/Users/" not in str(public)
    assert original == before


def test_publication_refuses_a_policy_digest_mismatch_or_unclassified_private_path():
    original = capture()
    original["policy_sha256"] = "wrong"
    with pytest.raises(ValueError, match="digest"):
        publish.public_capture(original)
    original = capture()
    original["records"][0]["synthetic"] = "/Users/private/unclassified"
    with pytest.raises(ValueError, match="path remains"):
        publish.public_capture(original)


def test_uncovered_diagnostic_distinguishes_public_spaces_from_raw_name_bytes(monkeypatch):
    from test_mcp_operation_order import entry, record
    for gap, expected in [(" ", True), (" van ", False)]:
        text = "Maria" + gap + "Schneider"
        case = {"id": "synthetic", "direction": "arguments", "expanded": [text],
                "gold": [{"path": [0], "span": [0, len(text.encode())], "class": "Name"}]}
        observed = record(["<deadbeef:Name_1>" + gap + "<deadbeef:Name_2>"],
                          [entry(raw="Maria"), entry(token="<deadbeef:Name_2>", raw="Schneider")])
        monkeypatch.setattr(publish.mcp, "fixture", lambda: [case])
        rows = publish.uncovered_gold_bytes({"records": [observed]})
        assert bytes(row["byte"] for row in rows).decode() == gap
        assert all(row["gap_is_whitespace"] == expected for row in rows)
        assert all(row["owned_before"] and row["owned_after"] for row in rows)
        assert publish.mcp.score_case(case, observed)["leaked"] == len(gap.encode())
