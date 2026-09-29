"""Model-free checks for aggregate-only known-record attribution."""

from __future__ import annotations

import hashlib

import gaze_bench_score as score
import known_record_attribution as attribution


def document(text: str, spans: tuple[score.Span, ...] = (), *, decoy: bool = False) -> score.Document:
    return score.Document(
        uid="synthetic", text=text, language="en", region="", source_dataset=(
            "known-record-oracle-counterweight" if decoy else "synthetic"
        ), spans=spans,
    )


def trace(start: int, end: int, class_name: str = "name", slot: int = 0) -> dict:
    digest = hashlib.sha256(class_name.encode()).hexdigest()
    return {
        "raw_start": start, "raw_end": end, "class": class_name,
        "action": "tokenize", "provenance": {
            "stage": "primary_pipeline", "decision": "policy",
            "source_ids": [f"context/record-v2-{digest}-{slot}"],
        },
    }


def response(*items: dict) -> dict:
    return {"final_protection_trace": list(items)}


def row_for(text: str, value: str, *, common: frozenset[str] = frozenset(), decoy: bool = False) -> dict:
    raw = text.encode()
    doc = document(text, () if decoy else (score.Span(0, len(raw), "GIVENNAME"),), decoy=decoy)
    recorder = attribution.AttributionRecorder.create(common, frozenset({"GIVENNAME"}))
    recorder.record_baseline(doc, response())
    recorder.record_candidate(doc, response(trace(0, len(raw))), [("Name", value)], decoy=decoy)
    return recorder.result(
        leaked_fall=0 if decoy else len(raw),
        false_positive_rise=len(raw) if decoy else 0,
        eligible_leak_fall=len(raw) if not decoy and text == value else 0,
    )["rows"][0]


def test_exact_and_common_word_corrobation_have_separate_kinds() -> None:
    exact = row_for("Maren", "Maren")
    assert (exact["record_class"], exact["match_kind"], exact["gold_recovered_bytes"]) == (
        "name_single", "exact", 5,
    )
    common = row_for("Will", "Will", common=frozenset({"will"}))
    assert (common["record_class"], common["match_kind"], common["gold_recovered_bytes"]) == (
        "name_single", "corroborated_single", 4,
    )


def test_whitespace_and_casefold_kinds_keep_utf8_byte_counts() -> None:
    whitespace = row_for("Alice\u00a0Smith", "Alice Smith")
    assert whitespace["record_class"] == "name_multi"
    assert whitespace["match_kind"] == "whitespace_flexible"
    assert whitespace["gold_recovered_bytes"] == len("Alice\u00a0Smith".encode())
    folded = row_for("ALICE SMITH", "Alice Smith")
    assert folded["match_kind"] == "case_folded"
    exact_nonbreaking = row_for("Alice\u00a0Smith", "Alice\u00a0Smith")
    assert exact_nonbreaking["match_kind"] == "exact"


def test_decoy_false_positive_is_counted_without_raw_value() -> None:
    row = row_for("Maren", "Maren", decoy=True)
    assert row["population"] == "decoy"
    assert row["false_positive_added_bytes"] == 5
    assert "Maren" not in str(row)


def test_baseline_displacement_is_visible_and_reconciled() -> None:
    doc = document("Maren", (score.Span(0, 5, "GIVENNAME"),))
    recorder = attribution.AttributionRecorder.create(frozenset(), frozenset({"GIVENNAME"}))
    recorder.record_baseline(doc, response(trace(0, 5)))
    recorder.record_candidate(doc, response(), [("Name", "Maren")], decoy=False)
    row = recorder.result(leaked_fall=-5, false_positive_rise=0)["rows"][0]
    assert row["record_class"] == "unattributed"
    assert row["match_kind"] == "baseline_removed"
    assert row["gold_lost_bytes"] == 5


def test_neutral_prediction_outside_gold_adds_no_scored_false_positive() -> None:
    doc = score.Document(
        uid="synthetic", text="Maren", language="en", region="", source_dataset="synthetic",
        spans=(), neutral_prediction_classes=frozenset({"name"}),
    )
    recorder = attribution.AttributionRecorder.create(frozenset(), frozenset({"GIVENNAME"}))
    recorder.record_baseline(doc, response())
    recorder.record_candidate(doc, response(trace(0, 5)), [("Name", "Maren")], decoy=False)
    result = recorder.result(leaked_fall=0, false_positive_rise=0)
    assert result["rows"] == []


def test_selected_candidate_event_shape_from_component_ledger_is_used() -> None:
    doc = document("Maren", (score.Span(0, 5, "GIVENNAME"),))
    digest = hashlib.sha256(b"name").hexdigest()
    candidate = response(trace(0, 5, "name"))
    candidate["final_protection_trace"][0]["provenance"]["source_ids"] = []
    candidate["candidate_events"] = [{
        "original": 0, "recognizer_id": f"context/record-v2-{digest}-0",
        "class": "name", "raw_start": 0, "raw_end": 5,
        "outcome": "selected", "selection_start": 0, "selection_end": 5,
        "settlement": "resolve", "role": "winner", "tier": None,
        "defeat_kind": None, "winner": None, "veto_reason": None,
    }]
    recorder = attribution.AttributionRecorder.create(frozenset(), frozenset({"GIVENNAME"}))
    recorder.record_baseline(doc, response())
    recorder.record_candidate(doc, candidate, [("Name", "Maren")], decoy=False)
    row = recorder.result(leaked_fall=5, false_positive_rise=0)["rows"][0]
    assert (row["record_class"], row["match_kind"], row["gold_recovered_bytes"]) == (
        "name_single", "exact", 5,
    )


def test_non_record_trace_cannot_claim_record_recovery() -> None:
    doc = document("Maren", (score.Span(0, 5, "GIVENNAME"),))
    other = trace(0, 5)
    other["provenance"]["source_ids"] = ["ner/synthetic"]
    recorder = attribution.AttributionRecorder.create(frozenset(), frozenset({"GIVENNAME"}))
    recorder.record_baseline(doc, response())
    recorder.record_candidate(
        doc, response(other, trace(0, 5)), [("Name", "Maren")], decoy=False
    )
    row = recorder.result(leaked_fall=5, false_positive_rise=0)["rows"][0]
    assert row["record_class"] == "name_single"


def test_source_slot_disambiguates_same_class_values() -> None:
    doc = document("MAREN", (score.Span(0, 5, "GIVENNAME"),))
    recorder = attribution.AttributionRecorder.create(frozenset(), frozenset({"GIVENNAME"}))
    recorder.record_baseline(doc, response())
    recorder.record_candidate(
        doc, response(trace(0, 5, slot=1)),
        [("Name", "Maren"), ("Name", "MAREN")], decoy=False,
    )
    row = recorder.result(leaked_fall=5, false_positive_rise=0)["rows"][0]
    assert row["match_kind"] == "exact"
