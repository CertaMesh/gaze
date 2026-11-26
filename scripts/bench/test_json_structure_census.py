"""JSON structure census: positions, carriers, attribution and the F2 gate."""
from __future__ import annotations

import json
from pathlib import Path

import pytest

import json_structure_census as census

ROOT = Path(__file__).resolve().parents[2]
V30_RECORD = ROOT / "docs/reference/benchmarks/history-generator-v30/v0.17.0/observations-v1.jsonl.gz"


def roles_of(text: str, needle: str, *, occurrence: int = 0, log: bool = False) -> set:
    """Roles of the UTF-8 bytes of the `occurrence`-th `needle` in `text`."""
    shape = census.structure(text, log_carrier=log)
    index = -1
    for _ in range(occurrence + 1):
        index = text.index(needle, index + 1)
    start = len(text[:index].encode("utf-8"))
    end = start + len(needle.encode("utf-8"))
    return set(shape.roles[start:end])


def trace(start, end, label="custom:x", sources=(("nym-small-int8", "winner"),)):
    return {"raw_start": start, "raw_end": end, "class": label, "provenance": {"contributions": [
        {"recognizer_id": name, "role": role, "raw_start": start, "raw_end": end}
        for name, role in sources]}}


def test_positions_of_keys_numbers_literals_and_values():
    text = '{"orderId":"A-12","qty":12,"ok":true,"note":null}'
    assert roles_of(text, '"orderId"') == {census.KEY}
    assert roles_of(text, "A-12") == {census.STRING}
    assert roles_of(text, "12,") == {census.NUMBER, census.SYNTAX}
    assert roles_of(text, "true") == {census.LITERAL}
    assert roles_of(text, "null") == {census.LITERAL}
    # A quoted literal is a string, not a literal.
    assert roles_of('{"ok":"true"}', "true") == {census.STRING}


def test_name_is_a_selector_only_inside_a_declared_carrier():
    carrier = '{"name":"lookup_record","arguments":{"customerId":"881234"}}'
    assert roles_of(carrier, "lookup_record") == {census.SELECTOR}
    assert roles_of(carrier, "881234") == {census.ARGUMENT}
    person = '{"name":"Dr. Schmidt","email":"dr.schmidt@example.invalid"}'
    assert roles_of(person, "Dr. Schmidt") == {census.STRING}
    explicit = '{"tool":"lookup","arguments":{"customerId":"858522"}}'
    assert roles_of(explicit, "lookup") == {census.SELECTOR}
    anthropic = '{"type":"tool_use","name":"get_account","input":{"limit":"20"}}'
    assert roles_of(anthropic, "get_account") == {census.SELECTOR}
    assert roles_of(anthropic, "20") == {census.ARGUMENT}
    # `input` alone, without a selector, is an ordinary field.
    assert roles_of('{"input":{"limit":"20"}}', "20") == {census.STRING}
    # A selector needs a structured argument member beside it.
    assert roles_of('{"tool":"hammer","price":3}', "hammer") == {census.STRING}
    assert roles_of('{"name":"Dr. Schmidt","input":"hello"}', "Dr. Schmidt") == {census.STRING}
    # A JSON-Schema tool definition is not a call.
    definition = '{"name":"get_account","parameters":{"type":"object"}}'
    assert roles_of(definition, "get_account") == {census.STRING}


def test_serialized_arguments_map_back_to_source_bytes():
    inner = json.dumps({"customerId": "NX7344", "days": 30})
    text = json.dumps({"function": {"name": "lookup_record", "arguments": inner}})
    assert roles_of(text, '\\"customerId\\"') == {census.KEY}
    assert roles_of(text, "NX7344") == {census.ARGUMENT}
    assert roles_of(text, "30") == {census.NUMBER}
    # Two levels deep: every escaped quote of the inner key still maps to it.
    twice = json.dumps({"payload": json.dumps({"payload": json.dumps({"memberId": "Z-9"})})})
    assert roles_of(twice, "memberId") == {census.KEY}
    assert roles_of(twice, "Z-9") == {census.STRING}


def test_escapes_and_non_ascii_keep_byte_offsets():
    text = '{"stra\\u00dfe":"Hauptstra\u00dfe 7","path":"a\\/b"}'
    assert roles_of(text, "Hauptstra\u00dfe 7") == {census.STRING}
    assert roles_of(text, '"stra\\u00dfe"') == {census.KEY}
    shape = census.structure(text)
    slash = len(text[:text.index("\\/")].encode("utf-8"))  # byte offset, after the two-byte ß
    assert (slash, slash + 2) in shape.escapes


def test_invalid_or_ambiguous_json_gives_no_structure():
    for text in ('{"id":"1","id":"2"}', '{"id":"1",}', '{"id":"\\x41"}', '{"id": 01}'):
        assert set(census.structure(text).roles) == {None}, text
    # A bracketed prose fragment is not JSON structure.
    assert set(census.structure("see [1] and [2, 3]").roles) == {None}
    # Trailing prose after a complete object keeps the object.
    assert roles_of('{"k":"v"} then 12', "k") == {census.KEY}
    assert roles_of('{"k":"v"} then 12', "12") == {None}


def test_log_keys_are_outside_quotes_only():
    text = 'level=debug values="fe80::1 fe80::2" result=stored'
    assert roles_of(text, "level=", log=True) == {census.LOG_KEY}
    assert roles_of(text, "result=", log=True) == {census.LOG_KEY}
    assert roles_of(text, "fe80:", occurrence=1, log=True) == {None}
    assert roles_of(text, "level=", log=False) == {None}
    # An apostrophe in prose or an escaped quote does not hide the next key.
    assert roles_of("msg=can't connect user=bob", "user=", log=True) == {census.LOG_KEY}
    assert roles_of('msg="say \\"hi" user=bob', "user=", log=True) == {census.LOG_KEY}


def test_log_carriers_are_named_surfaces_only():
    assert census._LOG_SURFACE.search("adjacent_log_kv")
    assert census._LOG_SURFACE.search("coverage_log")
    assert not census._LOG_SURFACE.search("credentials_review_metadata_logprob")


def test_fragments_split_by_role_and_flag_critical_bytes():
    text = '{"name":"get_account","input":{"zip":"90210"}}'
    raw = text.encode()
    selector = text.index("get_account")
    value = text.index("90210")
    rows = census.fragments("d", "D", raw, [], [trace(selector - 1, selector + 11), trace(value, value + 5,
                            sources=(("postal.us", "winner"),))], census.structure(text))
    assert [(r["role"], r["critical"], r["sources"]) for r in rows] == [
        (census.SYNTAX, True, ["nym-small-int8"]),
        (census.SELECTOR, True, ["nym-small-int8"]),
        (census.ARGUMENT, True, ["postal.us"]),
    ]
    # Gold bytes are never false positives; a defeated contributor detects nothing.
    rows = census.fragments("d", "D", raw, [(value, value + 5)], [trace(value, value + 5, sources=(
        ("postal.us", "winner"), ("ner", "defeated")))], census.structure(text))
    assert rows == []


def test_a_range_cutting_an_escape_is_critical_even_in_a_plain_value():
    text = '{"note":"a\\u00e9b"}'
    escape = text.index("\\u00e9")
    rows = census.fragments("d", "D", text.encode(), [], [trace(escape + 2, escape + 9)],
                            census.structure(text))
    assert rows and all(r["critical"] for r in rows if r["start"] < escape + 6)


def test_escape_flag_comes_from_trace_edges_not_fragment_edges():
    text = '{"note":"a\\u00e9b"}'
    escape = len(text[:text.index("\\u00e9")].encode())
    whole = trace(escape, escape + 6, sources=(("nym-small-int8", "winner"),))
    whole["provenance"]["contributions"] = [
        {"recognizer_id": "nym-small-int8", "role": "winner", "raw_start": escape, "raw_end": escape + 3},
        {"recognizer_id": "rule.x", "role": "co_member", "raw_start": escape + 3, "raw_end": escape + 6}]
    rows = census.fragments("d", "D", text.encode(), [], [whole], census.structure(text))
    assert len(rows) == 2 and not any(r["critical"] for r in rows)
    # A cut escape stays critical even when gold takes the cut half.
    rows = census.fragments("d", "D", text.encode(), [(escape, escape + 3)],
                            [trace(escape - 1, escape + 3)], census.structure(text))
    assert [(r["start"], r["critical"]) for r in rows] == [(escape - 1, True)]


def test_one_complete_config_is_read():
    def run(ids):
        return {"config": "pass2-ner", "scored_population": {"document_ids": ids},
                "failed_closed_population": {"document_ids": []}}
    header = {"scorecard": {"layers": {"A": {"runs": [run(["a", "b"])]}}}}
    rows = [{"layer": "A", "document_id": uid, "config": config}
            for uid in ("a", "b") for config in ("pass2-ner", "rule-floor-extended")]
    assert len(census._selected_rows(header, rows, "pass2-ner")) == 2
    with pytest.raises(census.CensusError):
        census._selected_rows(header, rows[:2], "pass2-ner")  # b is missing
    with pytest.raises(census.CensusError):
        census._selected_rows(header, rows, "policy-file")


def test_whitespace_bridging_two_gold_parts_is_not_a_false_positive():
    text = "Dr. Schmidt"
    raw = text.encode()
    rows = census.fragments("d", "A", raw, [(0, 3), (4, 11)], [trace(0, 11)], None)
    assert rows == []
    rows = census.fragments("d", "A", raw, [(0, 3)], [trace(0, 11)], None)
    assert [(r["start"], r["end"], r["role"]) for r in rows] == [(3, 11, census.PROSE)]


def test_learned_veto_exposure_counts_only_learned_only_gold():
    text = '{"customerId":819903,"limit":10}'
    value = text.index("819903")
    gold = [(value, value + 6)]
    shape = census.structure(text)
    learned = census.learned_veto_exposure(gold, [trace(value, value + 6)], shape)
    assert learned[census.NUMBER] == 6 and learned["outside_one_string_leaf"] == 6
    assert learned[census.KEY] == 0
    ruled = census.learned_veto_exposure(gold, [trace(value, value + 6, sources=(
        ("nym-small-int8", "winner"), ("customer_id.labelled", "co_member")))], shape)
    assert ruled[census.NUMBER] == 0


def _census(rows):
    return {"generator_corpus_sha256": "c", "benchmark_spec_id": "s", "critical_fragments": rows}


def test_gate_fails_only_new_critical_bytes():
    base = _census([{"document_id": "d", "start": 10, "end": 15, "role": "argument",
                     "class": "custom:postal_code", "sources": ["postal.us"], "critical": True}])
    same = _census([{**base["critical_fragments"][0], "start": 11, "end": 14}])
    assert census.gate(base, same) == []
    grown = _census([{**base["critical_fragments"][0], "start": 10, "end": 16}])
    assert len(census.gate(base, grown)) == 1
    moved = _census([{**base["critical_fragments"][0], "document_id": "e"}])
    assert len(census.gate(base, moved)) == 1
    with pytest.raises(census.CensusError):
        census.gate(base, {**same, "benchmark_spec_id": "other"})


@pytest.fixture(scope="module")
def v30():
    return census.census(V30_RECORD)


def test_v30_release_record_census(v30):
    # Pinned against the immutable v0.17.0 generator-v30 record. The old
    # classifier's 2,930 B omits Anthropic `input` arguments (7 fragments,
    # 5 documents, 32 B) and counted 6 IPv6 values inside quotes as log keys.
    summary = v30["summary"]
    assert summary["critical"] == {"fragments": 378, "documents": 364, "bytes": 2962}
    by_source = {(r["source"], r["class"], r["role"]): r["bytes"] for r in summary["critical_by_source"]}
    assert by_source == {
        ("ner", "organization", "key"): 370,
        ("nym-small-int8", "custom:building_number", "argument"): 56,
        ("nym-small-int8", "custom:building_number", "number"): 5,
        ("nym-small-int8", "custom:license_plate", "argument"): 1810,
        ("nym-small-int8", "custom:username", "literal"): 80,
        ("nym-small-int8", "custom:username", "selector"): 291,
        ("postal.us", "custom:postal_code", "argument"): 210,
        ("postal.us", "custom:postal_code", "number"): 140,
    }
    assert not any(r["role"] == census.LOG_KEY for r in v30["critical_fragments"])


def test_v30_blanket_learned_veto_would_expose_record_ids(v30):
    exposure = {r["veto"]: (r["documents"], r["gold_bytes"]) for r in v30["learned_veto_exposure"]}
    assert exposure == {"number": (4, 24), "outside_one_string_leaf": (4, 24)}
    # Layer C cannot be rebuilt, so its exposure is only bounded.
    assert set(v30["unmeasured_learned_veto_exposure_upper_bound"]) == {"C"}
