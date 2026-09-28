import json
import re
from pathlib import Path
from unittest.mock import patch

import gaze_bench_score as score
import known_record_arm as arm


POLICY = {"rule": [{"kind": "default", "action": "tokenize"}]}


def test_oracle_transport_injects_context_without_changing_scorer() -> None:
    requests = []

    class FakeTransport:
        def exchange(self, request):
            requests.append(request)
            return {"ok": True}

    def fake_run_config(**kwargs):
        transport = score.BenchSubprocess()
        transport.exchange({"fixture_id": "known", "text": "synthetic"})
        transport.exchange({"fixture_id": "unknown", "text": "synthetic"})
        return {"scored": True}

    with patch.object(score, "BenchSubprocess", FakeTransport), patch.object(
        score, "run_config", fake_run_config
    ):
        result = arm.run_with_record_context(
            {"known": '{"record":{}}', "unknown": None}
        )
        assert score.BenchSubprocess is FakeTransport

    assert result == {"scored": True}
    assert requests == [
        {"fixture_id": "known", "text": "synthetic", "context_json": '{"record":{}}'},
        {"fixture_id": "unknown", "text": "synthetic"},
    ]


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


def test_negative_receives_paired_record_and_counterweights() -> None:
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


def test_non_reversible_policy_fails_preflight() -> None:
    preserve = {"rule": [{"kind": "default", "action": "preserve"}]}
    try:
        arm.record_for_document(document("email", "alice@example.invalid", "EMAIL"), preserve)
    except ValueError as error:
        assert "no reversible action" in str(error)
    else:
        raise AssertionError("unsafe policy accepted")


def test_unsafe_record_value_is_excluded_from_oracle() -> None:
    raw, eligible = arm.record_for_document(document("short", "12", "PHONENUMBER"), POLICY)
    assert raw is None
    assert not eligible


def test_registered_record_classes_cover_oracle_label_classes() -> None:
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/gaze-recognizers/examples/clean_for_bench.rs").read_text()
    declarations = source.split("fn record_registry_context", 1)[1].split("let mut context", 1)[0]
    builtin = set(re.findall(r"PiiClass::(Email|Name|Location)\b", declarations))
    custom = {f"custom:{name}" for name in re.findall(r'PiiClass::Custom\("([^"]+)"\.into\(\)\)', declarations)}
    assert builtin | custom == set(arm.LABEL_CLASS.values())
