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
    assert all(row["pass"] for row in arm._compare_totals(base, candidate).values())
    for contract in ("v1", "v2"):
        candidate["scores"][contract]["cases"][0]["refused"] = 1
        candidate["scores"][contract]["cases"][1]["refused"] = 0
    assert not any(row["pass"] for row in arm._compare_totals(base, candidate).values())
    candidate["specification"] = "different"
    with pytest.raises(ValueError, match="non-comparable"):
        arm._compare_totals(base, candidate)


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


def test_gate_replays_observations_instead_of_trusting_saved_scores(monkeypatch):
    row = {**case(), "layer": "R", "category": "text"}
    monkeypatch.setattr(arm, "fixture", lambda: [row])
    specification = arm.identity()
    observed = record(row["expanded"], [])
    card = {"specification": specification, "contracts": {k: v["identity"] for k, v in arm.contracts().items()},
            "model_files": {}, "host_registry_lock": "fixed", "policy_sha256": "fixed", "runtime_revision": "fixed", "binary_sha256": "fixed",
            "records": [observed], "scores": arm.score([row], [observed])}
    card["host_build"] = {"runtime_revision": "fixed", "binary_sha256": "fixed", "runtime_lock_sha256": "fixed",
                          "source_sha256": specification["scripts/bench/native-mcp-operation/main.rs"],
                          "builder_sha256": specification["scripts/bench/build_native_mcp_operation.py"],
                          "registry_dependencies_pinned_to_runtime": True}
    assert not any(result["pass"] for result in arm.compare(card, copy.deepcopy(card)).values())
    tampered = copy.deepcopy(card)
    tampered["scores"]["v1"]["layers"]["R"]["leaked"] = 0
    with pytest.raises(ValueError, match="replayed"):
        arm.compare(card, tampered)
    tampered = copy.deepcopy(card)
    tampered["host_build"]["runtime_revision"] = "different"
    with pytest.raises(ValueError, match="provenance"):
        arm.compare(card, tampered)


def test_policy_must_load_the_models_whose_private_copies_are_hashed(tmp_path):
    policy = tmp_path / "policy.toml"
    ner, nym = tmp_path / "ner", tmp_path / "nym"
    policy.write_text(f'[ner]\nmodel_dir = "{ner}"\n[safety_net.nym]\nmodel_dir = "{nym}"\n')
    directories = {"GAZE_NER_MODEL_DIR": ner, "GAZE_NYM_MODEL_DIR": nym}
    arm.verify_policy_bundle_paths(policy, directories)
    with pytest.raises(ValueError, match="declared isolated bundle"):
        arm.verify_policy_bundle_paths(policy, {**directories, "GAZE_NER_MODEL_DIR": tmp_path / "other"})


def test_other_namespace_is_literal_and_never_credited_as_coverage():
    foreign = '<cafebabe:Name_1>'
    original = foreign + ' Maria Schneider'
    protected = foreign + ' <deadbeef:Name_1>'
    mask = arm.leaf_mask(original, protected, {'<deadbeef:Name_1>': entry()})
    assert mask[:len(foreign)] == [None] * len(foreign)
    assert mask[len(foreign):] == [None] + ['name'] * len('Maria Schneider')
    with pytest.raises(ValueError, match='authoritative original'):
        arm.leaf_mask('Maria Schneider', foreign, {})


def test_new_hit_gap_counterweights_keep_whitespace_outside_gold():
    rows = [row for row in arm.fixture() if row["id"].startswith("public-new-hit-gap-")]
    assert len(rows) == 48
    assert {row["direction"] for row in rows} == {"arguments", "response"}
    for row in rows:
        assert row["layer"] == "D" and row["category"] == "text"
        assert row["seed_names"][:3] == ["Maria", "Schneider", "Fischer"]
        text = row["expanded"]["items"][0].encode()
        mask = bytearray(len(text))
        for gold in row["gold"]:
            start, end = gold["span"]
            assert text[start:end].decode() in row["seed_names"][:3]
            mask[start:end] = b"\x01" * (end - start)
        assert bytes(byte for byte, private in zip(text, mask) if not private).decode().isspace()


def test_empty_owned_admission_counterweights_cover_both_wire_boundaries():
    rows = [row for row in arm.fixture() if row["id"].startswith("public-empty-owned-")]
    assert len(rows) == 2
    assert {row["direction"] for row in rows} == {"arguments", "response"}
    for row in rows:
        assert row["layer"] == "D" and row["category"] == "text"
        assert row["seed_names"] == ["Maria", ""]
        assert row["payload"]["items"] == ["{{owned_1}}Maria", "Maria{{owned_1}}"]
        assert row["expanded"] == {"items": ["Maria", "Maria"], "count": 17}
        owned = entry(raw="Maria")
        empty = entry(token="<deadbeef:Name_2>", raw="")
        wire = {"items": [empty["token"] + owned["token"], owned["token"] + empty["token"]], "count": 17}
        assert arm.masks(row["expanded"], wire, {owned["token"]: owned, empty["token"]: empty}) == {
            ("items", 0): ["name"] * 5, ("items", 1): ["name"] * 5}
        entries = {owned["token"]: owned, empty["token"]: empty}
        arm.verify_owned_identity(row["payload"], wire, entries, row["seed_names"])
        with pytest.raises(ValueError, match="caller-owned token identity changed"):
            arm.verify_owned_identity(row["payload"], {"items": [owned["token"], owned["token"]], "count": 17}, entries, row["seed_names"])
