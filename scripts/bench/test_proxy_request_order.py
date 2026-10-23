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
             "failed_closed": 0, "restore_exact": 1, "token_ownership_valid": 1,
             "gold_bytes": sum(sum(bench.gold_bytes(leaf)) for leaf in case["leaves"]),
             "leaked_bytes": leaks, "false_positive_bytes_v1": 0, "false_positive_bytes_v2": 0}
            for provider in bench.PROVIDERS for case in cases]
    counts = tuple(key for key in rows[0] if key not in {"provider", "id"})
    return {"schema": "gaze-proxy-request-order-v1", "layer": "proxy-request-order",
            "contract_labels": ["FIRSTNAME", "SURNAME", "EMAIL"], "scored_contracts": ["v2", "v1"],
            "fixture_sha256": bench.digest(bench.FIXTURE), "harness_sha256": bench.digest(bench.Path(bench.__file__)),
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


def test_native_upstream_echo_preserves_token_order_and_records_real_forwarding():
    second = "<1234abcd:Name_2>"
    with bench.upstream() as (url, captured):
        for provider in bench.PROVIDERS:
            case = {"leaves": [{"text": f"{TOKEN} and {second} and {TOKEN}"}]}
            path, body = bench.native_request(provider, case)
            status, reply = bench.request_json(url + path, body)
            assert status == 200
            assert bench.restored_values(provider, reply) == [TOKEN, second]
            actual_provider, actual_body, tokens = captured.get_nowait()
            assert actual_provider == provider
            assert bench.leaves(provider, actual_body) == [case["leaves"][0]["text"]]
            assert tokens == [TOKEN, second]
            assert captured.empty()
