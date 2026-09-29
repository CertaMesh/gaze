import json
import re
from pathlib import Path
from unittest.mock import patch

import gaze_bench_score as score
import known_record_arm as arm
import known_record_cells as cells


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


def test_oracle_context_uses_real_scorer_request_id() -> None:
    item = document("known-record-fixture", "Alice Smith", "GIVENNAME")
    requests = []

    class StoppingTransport:
        message_deadline = None

        def __init__(self, *args, **kwargs):
            pass

        def __enter__(self):
            return self

        def __exit__(self, *args):
            return False

        def exchange(self, request):
            requests.append(request)
            raise RuntimeError("request captured")

    with patch.object(score, "BenchSubprocess", StoppingTransport):
        try:
            arm.run_with_record_context(
                {item.uid: '{"record":{}}'},
                repo_root=Path(__file__).resolve().parents[2],
                binary=Path("synthetic-binary"),
                config="policy-file",
                documents=[item],
                model_dir=Path("synthetic-model"),
                opf_command=None,
                opf_checkpoint=None,
                opf_daemon_socket=None,
                threshold=0.3,
                diagnostics_dir=Path("synthetic-diagnostics"),
            )
        except RuntimeError:
            pass
        else:
            raise AssertionError("real scorer request was not sent")

    assert len(requests) == 1
    assert requests[0]["fixture_id"] == item.uid
    assert requests[0]["context_json"] == '{"record":{}}'


def test_gold_value_becomes_explicit_record_field() -> None:
    raw, eligible = arm.record_for_document(
        document("email", "alice@example.invalid", "EMAIL"), POLICY
    )
    parsed = json.loads(raw)
    assert parsed == {
        "record": {"v00": {"email": "alice@example.invalid"}},
    }
    assert eligible == {"EMAIL": len("alice@example.invalid")}


def test_name_multi_measurement_opts_in_and_counts_input_spans() -> None:
    items = [
        document(f"full-name-{index}", text, "GIVENNAME")
        for index, text in enumerate((
            "Alice Smith", "ALICE SMITH", "Alice  Smith", "ALICE  SMITH"
        ))
    ]
    context = json.dumps({"record": {"v00": {"name": "Alice Smith"}}})
    measured = arm.enable_name_multi_measurement(context)
    assert json.loads(measured)["record_match_kinds"]["name_multi"] == [
        "exact", "case_folded", "whitespace_flexible", "whitespace_case_folded"
    ]
    assert arm.name_multi_positive_spans(items, {item.uid: measured for item in items}) == {
        "exact": 1, "case_folded": 1, "whitespace_flexible": 1,
        "whitespace_case_folded": 1,
    }
    email = '{"record":{"v00":{"email":"alice@example.invalid"}}}'
    assert arm.enable_name_multi_measurement(email) == email


def test_split_name_gold_adds_full_name_record_and_counts_each_kind() -> None:
    cases = ("Alice Smith", "ALICE SMITH", "Alice  Smith", "ALICE  SMITH")
    items = []
    for index, text in enumerate(cases):
        gap = text.index(" ")
        last = text.rfind(" ") + 1
        items.append(score.Document(
            uid=f"split-name-{index}", text=text, language="en", region="US",
            source_dataset="synthetic",
            spans=(score.Span(0, gap, "GIVENNAME"), score.Span(last, len(text), "SURNAME")),
            negative_category=None,
        ))
    raw, eligible = arm.record_for_document(items[0], POLICY)
    assert [fields["name"] for fields in json.loads(raw)["record"].values()] == [
        "Alice", "Smith", "Alice Smith",
    ]
    assert eligible == {"GIVENNAME": 5, "SURNAME": 5}
    measured = arm.enable_name_multi_measurement(raw)
    assert arm.name_multi_positive_spans(items, {item.uid: measured for item in items}) == {
        "exact": 1, "case_folded": 1, "whitespace_flexible": 1,
        "whitespace_case_folded": 1,
    }


def test_repeated_split_name_variants_keep_one_full_name_record() -> None:
    text = "Alice Smith, ALICE SMITH"
    item = score.Document(
        uid="name-variants", text=text, language="en", region="US",
        source_dataset="synthetic",
        spans=(
            score.Span(0, 5, "GIVENNAME"), score.Span(6, 11, "SURNAME"),
            score.Span(13, 18, "GIVENNAME"), score.Span(19, 24, "SURNAME"),
        ),
        negative_category=None,
    )
    raw, _ = arm.record_for_document(item, POLICY)
    names = [fields["name"] for fields in json.loads(raw)["record"].values()]
    assert names.count("Alice Smith") == 1
    assert "ALICE SMITH" not in names
    assert arm.name_multi_positive_spans([item], {item.uid: raw}) == {
        "exact": 1, "case_folded": 1, "whitespace_flexible": 0,
        "whitespace_case_folded": 0,
    }
    separated = item.__class__(
        uid="separated", text="Alice\nSmith", language="en", region="US",
        source_dataset="synthetic",
        spans=(score.Span(0, 5, "GIVENNAME"), score.Span(6, 11, "SURNAME")),
        negative_category=None,
    )
    assert arm.full_name_gold_spans(separated) == []


def test_middle_name_gold_joins_full_name_without_losing_single_slots() -> None:
    item = score.Document(
        uid="middle-name", text="Alice Beth Smith", language="en", region="US",
        source_dataset="synthetic",
        spans=(
            score.Span(0, 5, "FIRSTNAME"), score.Span(6, 10, "MIDDLENAME"),
            score.Span(11, 16, "SURNAME"),
        ),
        negative_category=None,
    )
    raw, eligible = arm.record_for_document(item, POLICY)
    assert [fields["name"] for fields in json.loads(raw)["record"].values()] == [
        "Alice", "Beth", "Smith", "Alice Beth Smith",
    ]
    assert eligible == {"FIRSTNAME": 5, "MIDDLENAME": 4, "SURNAME": 5}
    assert arm.name_multi_positive_spans([item], {item.uid: raw})["exact"] == 1


def test_layer_k_supplies_positive_full_name_targets_under_both_contracts() -> None:
    root = Path(__file__).resolve().parents[2]
    pairs = cells.generate()
    measurement = arm.kind_contexts_for_measurement(pairs)
    iban = next(pair.positive for pair in pairs if pair.positive.variant == "iban_double_space")
    assert "whitespace_flexible" in json.loads(measurement[iban.uid])["record_match_kinds"]["custom:iban"]
    for contract in ("v2", "v1"):
        documents, _ = cells.documents(root, contract, pairs)
        assert set(measurement) == {document.uid for document in documents}
        contexts = {uid: arm.enable_name_multi_measurement(raw) for uid, raw in measurement.items()}
        counts = arm.name_multi_positive_spans(documents, contexts, kind_pairs=pairs)
        assert all(counts[kind] > 0 for kind in arm.NAME_MULTI_KINDS)


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


def test_oracle_filters_values_that_record_parser_rejects() -> None:
    for value, class_name in [("A12", "custom:tag"), ("A1234", "custom:tag"), ("12", "custom:phone")]:
        assert not arm.safe_record_value(value, class_name)
    for value in [" Alice Smith ", "Alice  Smith", "Alice\u00a0Smith", "Will"]:
        assert arm.safe_record_value(value, "Name")
    assert arm.safe_record_value("Jörg Straße", "Name")
    for iban in ["DE36000000000000000000", "AT180000000000000000", "FR7600000000000000000000000"]:
        assert arm.safe_record_value(iban, "custom:iban")
    assert not arm.safe_record_value("DE00000000000000000000", "custom:iban")


def test_repeated_gold_spans_share_one_record_slot_but_both_remain_eligible() -> None:
    value = "DE36000000000000000000"
    text = f"{value} and {value}"
    second = text.index(value, len(value))
    item = score.Document(
        uid="repeat", text=text, language="de", region="DE", source_dataset="synthetic",
        spans=(score.Span(0, len(value), "IBAN"), score.Span(second, second + len(value), "IBAN")),
        negative_category=None,
    )
    raw, eligible = arm.record_for_document(item, POLICY)
    assert len(json.loads(raw)["record"]) == 1
    assert eligible == {"IBAN": 2 * len(value)}


def test_registered_record_classes_cover_oracle_label_classes() -> None:
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/gaze-recognizers/examples/clean_for_bench.rs").read_text()
    declarations = source.split("fn record_registry_context", 1)[1].split("let mut context", 1)[0]
    builtin = set(re.findall(r"PiiClass::(Email|Name|Location)\b", declarations))
    custom = {f"custom:{name}" for name in re.findall(r'PiiClass::Custom\("([^"]+)"\.into\(\)\)', declarations)}
    assert builtin | custom == set(arm.LABEL_CLASS.values())
