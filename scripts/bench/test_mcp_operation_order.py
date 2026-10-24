"""Independent byte oracle and signed-manifest checks; no runtime or models run here."""
import copy
import hashlib
import json

import pytest
import mcp_operation_order as arm


def snapshot(entries):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
    key = Ed25519PrivateKey.from_private_bytes(bytes([1]) * 32)
    public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    payload = json.dumps({"session_hex": "deadbeef", "entries": entries}).encode()
    header = bytes([6]) + public
    return (header + key.sign(header + payload) + payload).hex()


def entry(token="<deadbeef:Name_1>", raw="Maria Schneider", cls="Name"):
    return {"token": token, "raw": raw, "class": cls, "family": "counter"}


def record(protected, entries):
    call = "01ARZ3NDEKTSV4RRFFQ69G5FAV"
    response = []
    wire = b"[]"
    mapping = {item["token"]: item["raw"] for item in entries}
    def restored(value):
        if isinstance(value, str):
            return arm.TOKEN.sub(lambda match: mapping[match.group()], value)
        if isinstance(value, list):
            return [restored(item) for item in value]
        if isinstance(value, dict):
            return {key: restored(item) for key, item in value.items()}
        return value
    return {"id": "synthetic", "phase": "target", "error": None, "tool_args": protected, "response": response, "response_wire": "[]",
            "snapshot_hex": snapshot(entries), "restored": restored(protected), "audit": {"begin": 1, "finish": 1, "fail": 0, "args": protected,
            "call": call, "snapshot": {"locator": "inline-sha256:" + call, "byte_len": len(wire),
            "sha256_hex": hashlib.sha256(call.encode() + b"\0" + arm.ulid_bytes(call) + b"\0" + wire).hexdigest()}}}


def case():
    return {"id": "synthetic", "direction": "arguments", "expanded": ["hello Maria Schneider"],
            "gold": [{"path": [0], "span": [6, 21], "class": "Name"}]}


def test_oracle_counts_source_last_leak_and_exact_token_coverage():
    data = case()
    raw = record(data["expanded"], [])
    assert arm.score_case(data, raw)["leaked"] == 15
    protected = record(["hello <deadbeef:Name_1>"], [entry()])
    scored = arm.score_case(data, protected)
    assert scored == {"leaked": 0, "false_positive": 0, "refused": 0, "dispatch_failed": 0, "restore_exact": 1,
                      "restore_failed": 0, "manifest_valid": 1, "manifest_invalid": 0}


def test_oracle_detects_missing_suffix_wrong_offsets_and_false_positives():
    data = case()
    partial = record(["hello <deadbeef:Name_1> Schneider"], [entry(raw="Maria")])
    assert arm.score_case(data, partial)["leaked"] == len(" Schneider")
    wrong = record(["hello <deadbeef:Name_1>"], [entry(raw="Schneider Maria")])
    assert arm.score_case(data, wrong)["restore_failed"] == 1
    over = record(["<deadbeef:Name_1>"], [entry(raw="hello Maria Schneider")])
    assert arm.score_case(data, over)["false_positive"] == 6
    bad_gold = copy.deepcopy(data)
    bad_gold["gold"][0]["span"][1] = 99
    with pytest.raises(ValueError, match="invalid gold"):
        arm.score_case(bad_gold, record(data["expanded"], []))


def test_signature_prefix_duplicates_shape_types_and_audit_are_independent():
    for entries in [[entry(token="<12345678:Name_1>")], [entry(), entry()]]:
        with pytest.raises(ValueError):
            arm.entries_from_snapshot(snapshot(entries))
    signed = bytearray.fromhex(snapshot([entry()]))
    signed[40] ^= 1
    with pytest.raises(ValueError, match="invalid signed"):
        arm.entries_from_snapshot(signed.hex())
    for original, protected in [([17], ["17"]), ({"a": "public"}, {"b": "public"}), (["public"], [])]:
        with pytest.raises(ValueError):
            arm.masks(original, protected, {})
    observed = record(case()["expanded"], [])
    observed["audit"]["snapshot"]["byte_len"] += 1
    assert arm.score_case(case(), observed)["manifest_invalid"] == 1


def test_fixture_extents_are_utf8_boundaries_and_cover_all_layers_and_controls():
    rows = arm.fixture()
    assert {r["direction"] for r in rows} == {"arguments", "response"}
    assert {r["category"] for r in rows if r["layer"] == "D"} == {"text", "code", "number"}
    assert any(r["seed_names"] for r in rows)
    for row in rows:
        for span in row["gold"]:
            text = row["expanded"]
            for edge in span["path"]:
                text = text[edge]
            start, end = span["span"]
            assert 0 <= start < end <= len(text.encode())
            text.encode()[:start].decode()
            text.encode()[start:end].decode()
    assert set(arm.contracts()) == {"v1", "v2"}


def test_neutral_predictions_and_refusals_remain_visible():
    data = {"id": "synthetic", "direction": "arguments", "expanded": ["public"], "gold": []}
    observed = record(["<deadbeef:Name_1>"], [entry(raw="public", cls={"Custom": "password"})])
    assert arm.score_case(data, observed)["false_positive"] == 6
    assert arm.score_case(data, observed, {"custom:password"})["false_positive"] == 0
    assert arm.score_case(data, {"id": "synthetic", "phase": "seed", "error": "seed failed"})["refused"] == 1


def test_gate_rejects_per_layer_regression_and_new_refusal_even_at_equal_totals():
    totals = {layer: dict.fromkeys(arm.FIELDS, 0) for layer in "ADR"}
    totals["R"]["leaked"] = 15
    scores = {contract: {"layers": copy.deepcopy(totals), "cases": [{"id": "a", "refused": 0}, {"id": "b", "refused": 1}], "d_counterweights": {}} for contract in ("v1", "v2")}
    base = {"specification": "fixed", "contracts": "fixed", "model_files": "fixed", "host_registry_lock": "fixed", "policy_sha256": "fixed", "scores": scores}
    candidate = copy.deepcopy(base)
    for contract in ("v1", "v2"):
        candidate["scores"][contract]["layers"]["R"]["leaked"] = 0
    assert all(row["pass"] for row in arm.compare(base, candidate).values())
    for contract in ("v1", "v2"):
        candidate["scores"][contract]["cases"][0]["refused"] = 1
        candidate["scores"][contract]["cases"][1]["refused"] = 0
    assert not any(row["pass"] for row in arm.compare(base, candidate).values())
    candidate["specification"] = "different"
    with pytest.raises(ValueError, match="non-comparable"):
        arm.compare(base, candidate)


def test_owned_identity_cannot_be_replaced_by_an_equivalent_new_token():
    entries = {"<deadbeef:Name_1>": entry(), "<deadbeef:Name_2>": entry(token="<deadbeef:Name_2>")}
    with pytest.raises(ValueError, match="identity"):
        arm.verify_owned_identity("hello {{owned_0}}", "hello <deadbeef:Name_2>", entries, ["Maria Schneider"])
    arm.verify_owned_identity("hello {{owned_0}}", "hello <deadbeef:Name_1>", {"<deadbeef:Name_1>": entry()}, ["Maria Schneider"])


def test_runtime_restore_failure_is_not_hidden_by_a_good_mapping_oracle():
    observed = record(["hello <deadbeef:Name_1>"], [entry()])
    observed["restored"] = None
    assert arm.score_case(case(), observed)["restore_failed"] == 1


def test_later_dispatch_failure_cannot_hide_arguments_already_released():
    observed = record(case()["expanded"], [])
    observed["error"] = "response protection failed"
    observed["response"] = None
    observed["response_wire"] = "null"
    observed["audit"].update(finish=0, fail=1, snapshot=None)
    result = arm.score_case(case(), observed)
    assert result["leaked"] == 15
    assert result["refused"] == 0
    assert result["dispatch_failed"] == 1
    response_case = {**case(), "direction": "response"}
    assert arm.score_case(response_case, observed)["refused"] == 1
