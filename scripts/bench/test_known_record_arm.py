import json

import gaze_bench_score as score
import known_record_arm as arm


POLICY = {"rule": [{"kind": "default", "action": "tokenize"}]}


def document(uid: str, text: str, label: str | None) -> score.Document:
    spans = (score.Span(0, len(text.encode()), label),) if label else ()
    return score.Document(
        uid=uid, text=text, language="en", region="US",
        source_dataset="synthetic", spans=spans,
        negative_category="lookalike" if label is None else None,
    )


def test_gold_value_becomes_explicit_record_field() -> None:
    raw, eligible = arm.record_for_document(
        document("email", "alice@example.invalid", "EMAIL"), POLICY
    )
    parsed = json.loads(raw)
    assert parsed == {
        "record": {"v00": "alice@example.invalid"},
        "field_map": {"/v00": "Email"},
    }
    assert eligible == {"EMAIL": len("alice@example.invalid")}


def test_negative_receives_paired_record_and_variant_gold_is_scored() -> None:
    positive = document("positive", "Alice Smith", "GIVENNAME")
    negative = document("negative", "The catalog is open.", None)
    contexts, _ = arm.paired_records([positive, negative], POLICY)
    assert contexts[negative.uid] == contexts[positive.uid]
    counters, explicit = arm.explicit_counterweights({"en": [contexts[positive.uid]]})
    assert counters and all(explicit[item.uid] == contexts[positive.uid] for item in counters)
    assert all(not item.spans for item in counters if item.negative_category)
    assert {item.negative_category for item in counters if item.negative_category} == {
        "record_homonym", "record_surname"
    }
    assert any(item.spans and item.cell.endswith("positive") for item in counters)


def test_non_reversible_policy_fails_preflight() -> None:
    preserve = {"rule": [{"kind": "default", "action": "preserve"}]}
    try:
        arm.record_for_document(document("email", "alice@example.invalid", "EMAIL"), preserve)
    except ValueError as error:
        assert "no reversible action" in str(error)
    else:
        raise AssertionError("unsafe policy accepted")


def test_name_order_variant_has_a_scored_synthetic_probe() -> None:
    single, _ = arm.record_for_document(document("single", "Alice", "FIRSTNAME"), POLICY)
    documents, contexts = arm.explicit_counterweights({"en": [single]})
    variant = next(item for item in documents if "name_order" in item.uid)
    assert [span.label for span in variant.spans] == ["SURNAME", "FIRSTNAME"]
    assert json.loads(contexts[variant.uid])["record"] == {"name": "Alice Smith"}
