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
    cases = bench.load_cases()
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
    cases = bench.load_cases()
    rows = [{"provider": provider, "id": case["id"], "attempted": 1, "forwarded": 1,
             "failed_closed": 0, "selected_restore_exact": 1, "alignment_valid": 1,
             "seed_forwarded": int("seed" in case), "seed_restore_exact": int("seed" in case), "owned_identities_preserved": 1,
             "gold_bytes": sum(sum(bench.gold_bytes(leaf)) for leaf in case["leaves"]),
             "leaked_bytes": leaks, "false_positive_bytes_v1": 0, "false_positive_bytes_v2": 0}
            for provider in bench.PROVIDERS for case in cases
            if provider in case.get("providers", bench.PROVIDERS)]
    counts = tuple(key for key in rows[0] if key not in {"provider", "id"})
    return {"schema": "gaze-proxy-request-order-v3", "layer": "proxy-request-order",
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
    old.update(forwarded=0, failed_closed=1, selected_restore_exact=0, alignment_valid=0, owned_identities_preserved=0)
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
    cases = [case for case in bench.load_cases() if "seed" not in case]
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
    case = next(case for case in bench.load_cases()
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


def test_seeded_admission_uses_an_expanded_owner_control_and_prices_all_fp():
    baseline, candidate = capture_card(2), capture_card(0)
    ids = ["existing-token-given-source_first", "existing-token-given-source_last"]
    for row in baseline["cases"]:
        if row["provider"] == "anthropic" and row["id"] in ids:
            row.update(forwarded=0, failed_closed=1, selected_restore_exact=0, alignment_valid=0, owned_identities_preserved=0, leaked_bytes=0)
    for card in [baseline, candidate]:
        for row in card["cases"]:
            if row["provider"] == "anthropic" and row["id"] == "existing-token-given-expanded-source_first":
                row.update(false_positive_bytes_v1=3, false_positive_bytes_v2=3)
        card["totals"] = {key: sum(row[key] for row in card["cases"]) for key in card["totals"]}
    for row in candidate["cases"]:
        if row["provider"] == "anthropic" and row["id"] in ids:
            row.update(false_positive_bytes_v1=3, false_positive_bytes_v2=3)
    candidate["totals"] = {key: sum(row[key] for row in candidate["cases"]) for key in candidate["totals"]}
    result = bench.compare(baseline, candidate)
    admissions = [row for row in result["refused_to_forwarded"] if row["id"] in ids]
    assert len(admissions) == 2
    assert all(row["control_kind"] == "expanded_owner_content" and row["preexisting_fp_proven"] for row in admissions)
    assert not result["regressions"]
    candidate["cases"][next(i for i, row in enumerate(candidate["cases"])
                            if row["provider"] == "anthropic" and row["id"] == ids[0])]["false_positive_bytes_v2"] += 1
    candidate["totals"]["false_positive_bytes_v2"] += 1
    assert bench.compare(baseline, candidate)["regressions"]


def test_document_alignment_uses_independent_owner_echo_and_cross_leaf_reuse():
    second = "<1234abcd:Name_2>"
    raw = [{"text": "first|middle|last"}, {"text": "middle|last"}]
    clean = [TOKEN + "|" + second, second]
    assert bench.document_mappings(raw, clean) == {TOKEN: "first", second: "middle|last"}
    assert bench.document_mappings(raw[:1], clean[:1], owner_facts={second: "middle|last"}) == {
        TOKEN: "first", second: "middle|last"}
    with pytest.raises(ValueError, match="contradicts"):
        bench.document_mappings(raw, clean, {TOKEN: "first"}, {TOKEN: "wrong"})
    with pytest.raises(bench.AmbiguousAlignment):
        bench.document_mappings(raw[:1], clean[:1])


def test_owned_identity_is_checked_at_each_original_expanded_position():
    second = "<1234abcd:Name_2>"
    known = {TOKEN: "Maria"}
    leaves = [{"text": "Maria and Maria", "wire_text": TOKEN + " and Maria"}]
    assert bench.owned_identities_preserved(leaves, [TOKEN + " and " + second], known,
                                            {TOKEN: "Maria", second: "Maria"})
    assert not bench.owned_identities_preserved(leaves, [second + " and " + TOKEN], known,
                                                {TOKEN: "Maria", second: "Maria"})


def test_a_wrong_owner_echo_is_not_accepted_or_used_to_hide_forwarded_bytes():
    raw, clean = [{"text": "Maria"}], [TOKEN]
    mappings, consistent = bench.checked_document_mappings(raw, clean, {}, {TOKEN: "wrong"})
    assert mappings == {TOKEN: "Maria"} and not consistent
    with pytest.raises(bench.AmbiguousAlignment):
        bench.checked_document_mappings([{"text": "first|middle|last"}],
            [TOKEN + "|<1234abcd:Name_2>"], {}, {TOKEN: "wrong"})


@pytest.mark.parametrize("candidate_restores", [0, 1])
def test_explicit_paired_gate_reports_unproven_new_forwarding_without_gain_credit(candidate_restores):
    baseline, candidate = capture_card(2), capture_card(0)
    before, after = baseline["cases"][0], candidate["cases"][0]
    before.update(forwarded=0, failed_closed=1, selected_restore_exact=0,
                  alignment_valid=0, owned_identities_preserved=0, leaked_bytes=0)
    after.update(selected_restore_exact=candidate_restores,
                 false_positive_bytes_v1=10000, false_positive_bytes_v2=10000)
    for card in (baseline, candidate):
        card["totals"] = {key: sum(row[key] for row in card["cases"]) for key in card["totals"]}
    assert bench.compare(baseline, candidate)["regressions"]
    result = bench.compare(baseline, candidate, gate_policy="paired")
    assert not result["regressions"]
    assert result["paired"]["cases"] == len(baseline["cases"]) - 1
    assert result["paired"]["candidate"]["false_positive_bytes_v2"] == 0
    assert result["unproven_admission_fp"][0]["candidate"]["false_positive_bytes_v2"] == 10000
    assert len(result["refused_in_both_stage_moved"]) == 1 - candidate_restores
    assert all(row["verdict"] == "pass" for row in result["contracts"].values())
    # New forwarding alone never supplies a byte or restoration gain.
    for row in baseline["cases"]:
        row["leaked_bytes"] = 0
    baseline["totals"]["leaked_bytes"] = 0
    assert all(row["verdict"] == "fail" for row in
               bench.compare(baseline, candidate, gate_policy="paired")["contracts"].values())


def test_paired_gate_still_rejects_owner_response_failure_for_an_already_forwarded_case():
    baseline, candidate = capture_card(2), capture_card(0)
    candidate["cases"][0]["selected_restore_exact"] = 0
    candidate["totals"]["selected_restore_exact"] -= 1
    result = bench.compare(baseline, candidate, gate_policy="paired")
    assert result["regressions"]
    assert all(row["verdict"] == "fail" for row in result["contracts"].values())


def test_alignment_failure_retains_diagnostics_and_never_writes_a_scorecard(tmp_path, monkeypatch):
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

    def ambiguous(*_args):
        raise bench.AmbiguousAlignment("synthetic ambiguous token boundaries")

    monkeypatch.setattr(bench, "checked_document_mappings", ambiguous)
    case = next(case for case in bench.load_cases() if "seed" not in case)
    fixture, output = tmp_path / "fixture.json", tmp_path / "result.json"
    fixture.write_text(json.dumps({"cases": [case]}))
    with pytest.raises(bench.AmbiguousAlignment, match="openai/" + case["id"]):
        bench.measure(binary, policy, fixture, output, tmp_path)
    assert not output.exists()
    failure = json.loads(output.with_suffix(".failure.json").read_text())
    assert failure["complete"] is False
    assert failure["provider"] == "openai" and failure["case"] == case
    assert failure["clean_leaves"] == [leaf["text"] for leaf in case["leaves"]]
    assert failure["completed_case_rows"] == []
    assert failure["known_seed_mappings"] == failure["owner_facts"] == {}


def test_single_token_owner_proofs_resolve_ambiguity_without_target_restore_credit(monkeypatch):
    import queue

    first, gap, owned = TOKEN, "<1234abcd:Name_2>", "<1234abcd:Name_3>"
    raw = [{"text": "Hello maria Schneider", "gold": []}]
    clean = ["Hello " + first + gap + owned]
    known = {owned: "Schneider"}
    with pytest.raises(bench.AmbiguousAlignment):
        bench.checked_document_mappings(raw, clean, known, {})
    captured = queue.Queue()
    actual_owner_values = {first: "maria", gap: " "}
    calls = []

    def owner_response(_url, body, **kwargs):
        assert kwargs["session_id"] == "synthetic-owned-session"
        token = bench.leaves("anthropic", body)[0]
        calls.append(token)
        captured.put(("anthropic", body, [token]))
        return 200, bench.replies("anthropic", [actual_owner_values[token] + "\n"])

    monkeypatch.setattr(bench, "request_json", owner_response)
    facts, proofs = bench.prove_owner_mappings("synthetic", "anthropic", [first, gap, owned],
                                              captured, "synthetic-owned-session", known)
    assert calls == [first, gap]
    assert facts == actual_owner_values
    mappings, consistent = bench.checked_document_mappings(raw, clean, {**known, **facts}, {})
    assert mappings == {**known, **facts} and consistent
    assert all(proof["forwarded_token_identical"] and proof["raw_source_bytes_forwarded"] == 0 for proof in proofs)



@pytest.mark.parametrize("failure", ["mutated", "refused", "bad_restore"])
def test_single_token_owner_proofs_fail_closed(failure, monkeypatch):
    import queue

    captured = queue.Queue()

    def owner_response(_url, body, **_kwargs):
        if failure == "refused":
            return 422, {"error": "synthetic refusal"}
        actual = bench.native_request("anthropic", {"leaves": [{"text": "raw synthetic bytes"}]})[1] if failure == "mutated" else body
        captured.put(("anthropic", actual, [TOKEN]))
        return 502, {"error": "synthetic restore refusal"}

    monkeypatch.setattr(bench, "request_json", owner_response)
    with pytest.raises(ValueError, match="owner proof"):
        bench.prove_owner_mappings("synthetic", "anthropic", [TOKEN], captured,
                                  "synthetic-owned-session", {})


def test_supplemental_owner_proofs_do_not_turn_a_target_refusal_into_a_restore(tmp_path, monkeypatch):
    from contextlib import contextmanager
    import queue

    binary, policy = tmp_path / "binary", tmp_path / "policy.toml"
    binary.write_bytes(b"synthetic test executable")
    policy.write_text('[safety_net]\nbackend="nym"\n')
    monkeypatch.setattr(bench.subprocess, "check_output", lambda args, **kwargs:
                        "" if "status" in args else "a" * 40)
    monkeypatch.setattr(bench, "PROVIDERS", ("anthropic",))
    captured = queue.Queue()
    first, tail = TOKEN, "<1234abcd:Name_2>"
    owner_values = {first: "maria", tail: " Schneider"}
    raw = "Hello maria Schneider"

    @contextmanager
    def upstream():
        yield "synthetic", captured

    @contextmanager
    def passthrough(*_args, **_kwargs):
        yield "synthetic", ["synthetic-test-proxy"]

    def owner_response(_url, body, **kwargs):
        assert kwargs["session_id"]
        text = bench.leaves("anthropic", body)[0]
        if text == raw:
            actual = bench.native_request("anthropic", {"leaves": [{"text": "Hello " + first + tail}]})[1]
            captured.put(("anthropic", actual, [first, tail]))
            return 502, {"error": "synthetic original owner refusal"}
        captured.put(("anthropic", body, [text]))
        return 200, bench.replies("anthropic", [owner_values[text] + "\n"])

    monkeypatch.setattr(bench, "upstream", upstream)
    monkeypatch.setattr(bench, "proxy", passthrough)
    monkeypatch.setattr(bench, "request_json", owner_response)
    fixture, output = tmp_path / "fixture.json", tmp_path / "result.json"
    fixture.write_text(json.dumps({"cases": [{"id": "synthetic-owner-refusal", "order": "source_first",
        "leaves": [{"text": raw, "gold": [{"start": 6, "end": 11, "label": "FIRSTNAME"},
                                          {"start": 12, "end": 21, "label": "SURNAME"}]}]}]}))
    result = bench.measure(binary, policy, fixture, output, tmp_path, session_binary=binary)
    assert result["owner_mapping_proof_requests"] == 2
    assert result["totals"]["attempted"] == result["totals"]["forwarded"] == 1
    assert result["totals"]["selected_restore_exact"] == result["totals"]["failed_closed"] == 0
    assert result["totals"]["leaked_bytes"] == 0
    assert result["totals"]["false_positive_bytes_v2"] == 1
    assert result["cases"][0]["status"] == 502
    assert result["cases"][0]["alignment_valid"] == 1


@pytest.mark.parametrize("target_leak", [0, 7])
@pytest.mark.parametrize("seed_fp_cost", [0, 8])
def test_leak_to_refusal_ruling_requires_raw_target_leak_in_base(target_leak, seed_fp_cost):
    baseline, candidate = capture_card(0), capture_card(0)
    index = next(i for i, row in enumerate(baseline["cases"]) if row["seed_forwarded"])
    before, after = baseline["cases"][index], candidate["cases"][index]
    before.update(leaked_bytes=target_leak, seed_leaked_bytes=0, target_leaked_bytes=target_leak)
    after.update(forwarded=0, failed_closed=1, selected_restore_exact=0,
                 alignment_valid=0, owned_identities_preserved=0,
                 seed_leaked_bytes=0, target_leaked_bytes=0,
                 seed_false_positive_bytes_v1=seed_fp_cost, seed_false_positive_bytes_v2=seed_fp_cost,
                 false_positive_bytes_v1=seed_fp_cost, false_positive_bytes_v2=seed_fp_cost)
    for card in (baseline, candidate):
        for row in card["cases"]:
            row.setdefault("seed_leaked_bytes", 0)
            row.setdefault("target_leaked_bytes", row["leaked_bytes"])
            row.setdefault("seed_false_positive_bytes_v1", 0)
            row.setdefault("seed_false_positive_bytes_v2", 0)
        card["totals"] = {key: sum(row[key] for row in card["cases"]) for key in card["totals"]}
    result = bench.compare(baseline, candidate, gate_policy="leak-to-refusal")
    assert bool(result["regressions"]) == (target_leak == 0)
    assert len(result["leak_to_refused"]) == int(target_leak > 0)
    assert all(row["verdict"] == ("pass" if target_leak > seed_fp_cost else "fail")
               for row in result["contracts"].values())
    assert result["contracts"]["v2"]["leaked_bytes_removed"] == target_leak

    assert result["contracts"]["v2"]["false_positive_bytes_added"] == (seed_fp_cost if target_leak else 0)


def test_seed_leak_does_not_justify_a_new_target_refusal():
    baseline, candidate = capture_card(0), capture_card(0)
    index = next(i for i, row in enumerate(baseline["cases"]) if row["seed_forwarded"])
    before, after = baseline["cases"][index], candidate["cases"][index]
    before.update(leaked_bytes=7, seed_leaked_bytes=7, target_leaked_bytes=0)
    after.update(leaked_bytes=7, seed_leaked_bytes=7, target_leaked_bytes=0,
                 forwarded=0, failed_closed=1, selected_restore_exact=0,
                 alignment_valid=0, owned_identities_preserved=0)
    for card in (baseline, candidate):
        for row in card["cases"]:
            row.setdefault("seed_leaked_bytes", 0)
            row.setdefault("target_leaked_bytes", row["leaked_bytes"])
            row.setdefault("seed_false_positive_bytes_v1", 0)
            row.setdefault("seed_false_positive_bytes_v2", 0)
        card["totals"] = {key: sum(row[key] for row in card["cases"]) for key in card["totals"]}
    result = bench.compare(baseline, candidate, gate_policy="leak-to-refusal")
    assert result["regressions"]
    assert not result["leak_to_refused"]


@pytest.mark.parametrize("provider", bench.PROVIDERS)
def test_unsupported_carrier_is_sent_and_scored_in_its_actual_wire_position(provider):
    case = {"leaves": [{"text": "private", "gold": [], "carrier": "unsupported"},
                        {"text": "public", "gold": []}]}
    _, body = bench.native_request(provider, case)
    assert body["unsupported_carrier"] == {"nested": ["private"]}
    assert bench.leaves(provider, body) == ["public", "private"]
    assert [leaf["text"] for leaf in bench.observation_leaves(case)] == ["public", "private"]
    private = {"text": "Maria", "gold": [{"start": 0, "end": 5, "label": "FIRSTNAME"}]}
    assert bench.score_leaf(private, "Maria", {})["leaked_bytes"] == 5


def test_leak_refusal_policy_requires_complete_split_byte_evidence():
    with pytest.raises(ValueError, match="separate seed and target"):
        bench.compare(capture_card(1), capture_card(0), gate_policy="leak-to-refusal")


def test_unsupported_cases_do_not_double_count_identical_wire_requests():
    seen = set()
    for case in bench.load_cases():
        if not any(leaf.get("carrier") == "unsupported" for leaf in case["leaves"]):
            continue
        _, body = bench.native_request("openai", case)
        signature = (json.dumps(body, sort_keys=True), json.dumps(case.get("seed"), sort_keys=True))
        assert signature not in seen, case["id"]
        seen.add(signature)


def test_large_owned_carriers_have_exact_independent_gold_and_small_wire():
    cases = {case["id"]: case for case in bench.load_cases()}
    for private in (False, True):
        case = cases["unsupported-large-owned-private-byte" if private else "unsupported-large-owned-counterweight"]
        leaf = case["leaves"][-1]
        name = "X" * 2048
        clean = leaf["wire_text"].replace("{seed}", TOKEN)
        assert len(clean) < 20000
        assert len(leaf["text"]) > 2 * 1024 * 1024
        score = bench.score_leaf(leaf, clean, {TOKEN: name})
        assert score["leaked_bytes"] == int(private)
        assert score["false_positive_bytes_v1"] == 0
        assert score["false_positive_bytes_v2"] == 0
        seed = case["seed"]
        assert seed["text"][seed["gold"][-1]["start"]:seed["gold"][-1]["end"]] == "m@example.invalid"
        if private:
            source = case["leaves"][0]
            assert source["text"][source["gold"][-1]["start"]:source["gold"][-1]["end"]] == "m@example.invalid"


def test_large_carriers_are_scoped_to_the_repaired_legacy_path():
    cases = bench.load_cases()
    assert sum(provider in case.get("providers", bench.PROVIDERS)
               for provider in bench.PROVIDERS for case in cases) == 177
    for case in cases[-6:]:
        assert case["providers"] == ["openai", "gemini"]
        assert "ResponseBodyLimitExceeded" in case["provider_exclusion_reason"]


def test_adjacent_owned_public_suffix_counterweights_have_zero_private_bytes():
    cases = [case for case in bench.load_cases() if case["id"].startswith("unsupported-adjacent-owned-")]
    assert len(cases) == 4
    for case in cases:
        leaf = case["leaves"][-1]
        clean = leaf["wire_text"].replace("{seed}", TOKEN)
        score = bench.score_leaf(leaf, clean, {TOKEN: "X" * 2048})
        assert score["leaked_bytes"] == 0
        assert score["false_positive_bytes_v1"] == score["false_positive_bytes_v2"] == 0
        assert len(clean) < 20000


def test_resource_limit_exception_is_typed_separate_and_gets_no_leak_credit():
    base = capture_card(leaks=1)
    candidate = json.loads(json.dumps(base))
    row = candidate["cases"][0]
    row.update(status=413, error_code="ResidualExpansionLimitExceeded", resource_limit_refusals=1,
               forwarded=0, failed_closed=1, selected_restore_exact=0, alignment_valid=0,
               owned_identities_preserved=0, leaked_bytes=0, seed_leaked_bytes=0, target_leaked_bytes=0)
    for card in (base, candidate):
        for item in card["cases"]:
            item.setdefault("resource_limit_refusals", 0)
            item.setdefault("seed_leaked_bytes", 0)
            item.setdefault("seed_false_positive_bytes_v1", 0)
            item.setdefault("seed_false_positive_bytes_v2", 0)
            item.setdefault("target_leaked_bytes", item["leaked_bytes"])
        card["totals"] = {key: sum(item[key] for item in card["cases"]) for key in card["totals"]}
    result = bench.compare(base, candidate, gate_policy="leak-to-refusal")
    assert len(result["resource_limit_transitions"]) == 1
    assert not result["leak_to_refused"]
    assert result["refusal_target_bytes_stopped"] == 0
    assert not result["regressions"]
    row["error_code"] = "UnprovenCoverage"
    with pytest.raises(ValueError, match="typed resource-limit"):
        bench.compare(base, candidate, gate_policy="leak-to-refusal")
