"""Independent byte accounting for the native proxy ordering case set."""
import json

import pytest

import proxy_request_order as bench


TOKEN = "<1234abcd:Name_1>"


def test_unicode_bytes_and_surrounding_public_text_are_scored_exactly():
    raw = "Write to Zoë today."
    start = len("Write to ".encode())
    end = start + len("Zoë".encode())
    leaf = {"text": raw, "gold": [{"start": start, "end": end, "label": "FIRSTNAME"}]}
    assert bench.score_leaf(leaf, f"Write to {TOKEN} today.", {TOKEN: "Zoë"}) == {
        "gold_bytes": 4, "leaked_bytes": 0,
        "false_positive_bytes_v1": 0, "false_positive_bytes_v2": 0,
    }
    assert bench.score_leaf(leaf, raw, {})["leaked_bytes"] == 4


def test_token_restoration_must_match_every_original_byte():
    with pytest.raises(ValueError, match="restore exactly"):
        bench.protected_bytes("synthetic", TOKEN, {TOKEN: "different"})
    with pytest.raises(KeyError):
        bench.protected_bytes("synthetic", TOKEN, {})


def test_credential_neutrality_only_excludes_non_gold_v2_bytes():
    token = "<1234abcd:Custom:password_1>"
    leaf = {"text": "PUBLIC", "gold": []}
    assert bench.score_leaf(leaf, token, {token: "PUBLIC"}) == {
        "gold_bytes": 0, "leaked_bytes": 0,
        "false_positive_bytes_v1": 6, "false_positive_bytes_v2": 0,
    }
    leaf["gold"] = [{"start": 0, "end": 6, "label": "SURNAME"}]
    assert bench.score_leaf(leaf, token, {token: "PUBLIC"})["leaked_bytes"] == 0


def test_authored_gold_and_order_twins_are_independent_of_runtime_output():
    cases = json.loads(bench.FIXTURE.read_text())["cases"]
    assert len({case["id"] for case in cases}) == len(cases)
    for case in cases:
        for leaf in case["leaves"]:
            mask = bench.gold_bytes(leaf)
            assert len(mask) == len(leaf["text"].encode())
        if case["id"] != "common-parts-counterweight":
            twin_id = case["id"].replace("source_first", "source_last")
            twin = next(item for item in cases if item["id"] == twin_id)
            assert sorted(json.dumps(leaf, sort_keys=True) for leaf in case["leaves"]) == sorted(
                json.dumps(leaf, sort_keys=True) for leaf in twin["leaves"])


def capture_card(leaks):
    cases = json.loads(bench.FIXTURE.read_text())["cases"]
    rows = [{"provider": provider, "id": case["id"], "attempted": 1, "forwarded": 1,
             "failed_closed": 0, "selected_restore_exact": 1, "alignment_valid": 1,
             "seed_forwarded": int("seed" in case), "seed_restore_exact": int("seed" in case),
             "gold_bytes": sum(sum(bench.gold_bytes(leaf)) for leaf in case["leaves"]),
             "leaked_bytes": leaks, "false_positive_bytes_v1": 0, "false_positive_bytes_v2": 0}
            for provider in bench.PROVIDERS for case in cases]
    counts = tuple(key for key in rows[0] if key not in {"provider", "id"})
    return {"schema": "gaze-proxy-request-order-v2", "layer": "proxy-request-order",
            "contract_labels": ["FIRSTNAME", "SURNAME", "EMAIL"], "scored_contracts": ["v2", "v1"],
            "fixture_sha256": bench.digest(bench.FIXTURE), "harness_sha256": bench.digest(bench.Path(bench.__file__)),
            "session_host_sha256": bench.digest(bench.Path(bench.__file__).with_name("native-session-proxy") / "main.rs"),
            "session_builder_sha256": bench.digest(bench.Path(bench.__file__).with_name("build_native_session_proxy.py")),
            "v2_contract_sha256": bench.digest(bench.V2_CONTRACT), "policy_sha256": "synthetic-policy",
            "cases": rows, "totals": {key: sum(row[key] for row in rows) for key in counts}}


def test_comparison_requires_gain_and_rejects_exposure_hidden_by_an_aggregate_gain():
    baseline, candidate = capture_card(2), capture_card(0)
    assert all(result["verdict"] == "pass" for result in bench.compare(baseline, candidate)["contracts"].values())
    assert all(result["verdict"] == "fail" for result in bench.compare(candidate, candidate)["contracts"].values())
    candidate["cases"][0]["leaked_bytes"] = 3
    candidate["totals"]["leaked_bytes"] = 3
    result = bench.compare(baseline, candidate)
    assert result["regressions"]
    assert all(row["verdict"] == "fail" for row in result["contracts"].values())


def test_comparison_refuses_missing_cases_or_self_reported_totals():
    baseline, candidate = capture_card(2), capture_card(0)
    candidate["totals"]["gold_bytes"] += 1
    with pytest.raises(ValueError, match="totals"):
        bench.compare(baseline, candidate)
    candidate = capture_card(0)
    candidate["cases"].pop()
    with pytest.raises(ValueError, match="missing or duplicate"):
        bench.compare(baseline, candidate)


def test_admission_fp_is_separate_and_requires_an_existing_pipeline_control():
    baseline, candidate = capture_card(0), capture_card(0)

    def retotal(card):
        card["totals"] = {key: sum(row[key] for row in card["cases"]) for key in card["totals"]}

    def row(card, case):
        return next(item for item in card["cases"] if item["provider"] == "anthropic" and item["id"] == case)

    old = row(baseline, "synthetic-0-source_last")
    old.update(forwarded=0, failed_closed=1, selected_restore_exact=0, alignment_valid=0)
    for card, case in ((baseline, "synthetic-0-source_first"),
                       (candidate, "synthetic-0-source_first"), (candidate, "synthetic-0-source_last")):
        row(card, case).update(false_positive_bytes_v1=1000, false_positive_bytes_v2=1000)
    retotal(baseline)
    retotal(candidate)
    result = bench.compare(baseline, candidate)
    assert result["paired"]["cases"] == len(baseline["cases"]) - 1
    assert result["refused_to_forwarded"][0]["preexisting_fp_proven"]
    # Admission and restore-count gains cannot replace paired byte improvement.
    assert all(item["verdict"] == "fail" for item in result["contracts"].values())
    baseline["cases"][0]["leaked_bytes"] = 10
    retotal(baseline)
    result = bench.compare(baseline, candidate)
    assert all(item["net_bytes"] == 10 and item["verdict"] == "pass"
               for item in result["contracts"].values())
    row(candidate, "synthetic-0-source_last")["false_positive_bytes_v2"] += 1
    retotal(candidate)
    result = bench.compare(baseline, candidate)
    assert not result["refused_to_forwarded"][0]["preexisting_fp_proven"]
    assert all(item["verdict"] == "fail" for item in result["contracts"].values())


def test_native_upstream_echo_preserves_token_order_and_records_real_forwarding():
    second = "<1234abcd:Name_2>"
    with bench.upstream() as (url, captured):
        for provider in bench.PROVIDERS:
            case = {"leaves": [{"text": f"{TOKEN} and {second} and {TOKEN}"}]}
            path, body = bench.native_request(provider, case)
            status, reply = bench.request_json(url + path, body)
            assert status == 200
            assert bench.restored_values(provider, reply) == [TOKEN + "\n", second + "\n"]
            actual_provider, actual_body, tokens = captured.get_nowait()
            assert actual_provider == provider
            assert bench.leaves(provider, actual_body) == [case["leaves"][0]["text"]]
            assert tokens == [TOKEN, second]
            assert captured.empty()


def test_literal_gap_alignment_requires_one_reversible_interpretation():
    assert bench.infer_mappings("Write to Zoë today.", f"Write to {TOKEN} today.") == {TOKEN: "Zoë"}
    second = "<1234abcd:Name_2>"
    with pytest.raises(ValueError, match="adjacent"):
        bench.infer_mappings("firstsecond", TOKEN + second)
    with pytest.raises(ValueError, match="uniquely"):
        bench.infer_mappings("first|middle|last", TOKEN + "|" + second)
    with pytest.raises(ValueError, match="inconsistent"):
        bench.infer_mappings("first and second.", TOKEN + " and " + TOKEN + ".")


def test_non_gold_classes_are_priced_even_when_not_echoed():
    token = "<1234abcd:Custom:url_1>"
    raw = "https://example.invalid/public"
    assert bench.echoed_tokens([TOKEN, token]) == [TOKEN]
    assert bench.score_leaf({"text": raw, "gold": []}, token, {token: raw})["false_positive_bytes_v2"] == len(raw)


def test_post_forward_response_errors_cannot_hide_request_leaks(monkeypatch, tmp_path):
    from contextlib import contextmanager
    binary, policy = tmp_path / "binary", tmp_path / "policy.toml"
    binary.write_bytes(b"synthetic test executable")
    policy.write_text('[safety_net]\nbackend="nym"\n')
    monkeypatch.setattr(bench.subprocess, "check_output", lambda args, **kwargs:
                        "" if "status" in args else "a" * 40)

    @contextmanager
    def passthrough(_binary, _policy, upstream_url, _log_path, _root):
        yield upstream_url, ["synthetic-test-proxy"]

    monkeypatch.setattr(bench, "proxy", passthrough)
    original_request = bench.request_json

    def fail_after_forward(*args, **kwargs):
        original_request(*args, **kwargs)  # The native upstream receives the actual request.
        return 502, {"error": "synthetic-owner-response-failure"}

    monkeypatch.setattr(bench, "request_json", fail_after_forward)
    fixture = tmp_path / "unseeded.json"
    cases = [case for case in json.loads(bench.FIXTURE.read_text())["cases"] if "seed" not in case]
    fixture.write_text(json.dumps({"cases": cases}))
    result = bench.measure(binary, policy, fixture, tmp_path / "result.json", tmp_path)
    expected = len(cases) * len(bench.PROVIDERS)
    assert result["totals"]["forwarded"] == result["totals"]["attempted"] == expected
    assert result["totals"]["failed_closed"] == 0
    assert result["totals"]["leaked_bytes"] == result["totals"]["gold_bytes"] > 0
    assert result["totals"]["selected_restore_exact"] == 0


def test_repeated_literal_gaps_use_complete_leaf_and_reused_token_constraints():
    email = "<1234abcd:Email_1>"
    given, surname = "<1234abcd:Name_3>", "<1234abcd:Name_2>"
    raw = "From: Johanna <m@example.invalid>\nHello maria Johanna schneider"
    clean = f"From: {TOKEN} <{email}>\nHello {given} {TOKEN} {surname}"
    assert bench.infer_mappings(raw, clean) == {
        TOKEN: "Johanna", email: "m@example.invalid", given: "maria", surname: "schneider",
    }
    assert bench.protected_bytes(raw, clean, bench.infer_mappings(raw, clean))


def test_literal_gap_alignment_still_refuses_genuinely_ambiguous_token_extents():
    with pytest.raises(ValueError, match="uniquely"):
        bench.infer_mappings("maria johanna schneider", f"{TOKEN} <1234abcd:Name_2>")


def test_seeded_alignment_uses_prior_manifest_values_for_adjacent_tokens():
    suffix = "<1234abcd:Name_2>"
    assert bench.infer_mappings("Maria schneider", TOKEN + suffix, {TOKEN: "Maria"}) == {
        TOKEN: "Maria", suffix: " schneider"}
    assert bench.infer_mappings("maria Schneider", suffix + TOKEN, {TOKEN: "Schneider"}) == {
        suffix: "maria ", TOKEN: "Schneider"}
    with pytest.raises(ValueError, match="inconsistent"):
        bench.infer_mappings("Maria schneider", TOKEN + suffix, {TOKEN: "Schneider"})
    with pytest.raises(ValueError, match="adjacent"):
        bench.infer_mappings("Maria schneider", TOKEN + suffix)


def test_seed_binding_requires_a_real_prior_owned_token_and_exact_restore(monkeypatch):
    import queue
    email = "<1234abcd:Email_1>"
    case = next(case for case in json.loads(bench.FIXTURE.read_text())["cases"]
                if case["id"] == "existing-token-given-source_last")
    captured = queue.Queue()
    def seed_reply(url, body, **kwargs):
        assert kwargs["session_id"] == "synthetic-session"
        wire = {"messages": [{"content": f"From: {TOKEN} <{email}>\nHello."}]}
        captured.put(("openai", wire, [TOKEN, email]))
        return 200, bench.replies("openai", ["Maria\n", "m@example.invalid\n"])
    monkeypatch.setattr(bench, "request_json", seed_reply)
    bound, known, scores, token = bench.seed_session("http://example.invalid", "openai", case, captured, "synthetic-session")
    assert token == TOKEN and known[TOKEN] == "Maria"
    assert "{seed}" not in json.dumps(bound) and TOKEN in json.dumps(bound)
    assert scores["gold_bytes"] > 0 and scores["leaked_bytes"] == 0
    monkeypatch.setattr(bench, "request_json", lambda *args, **kwargs: (400, {}))
    with pytest.raises(ValueError, match="seed request refused"):
        bench.seed_session("http://example.invalid", "openai", case, captured, "synthetic-session")


def test_native_session_ids_satisfy_the_transport_uuid4_contract():
    import uuid
    for provider in bench.PROVIDERS:
        value = bench.native_session_id(provider, "synthetic-case")
        identifier = uuid.UUID(value)
        assert identifier.version == 4 and identifier.variant == uuid.RFC_4122
        assert str(identifier) == value
