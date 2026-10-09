from __future__ import annotations

import comparison_metrics as metrics
import gaze_bench_score as score
import compare


def test_split_is_deterministic_and_disjoint() -> None:
    ids = [f"synthetic-{i}" for i in range(100)]
    assert {metrics.split_for_id(uid) for uid in ids} == {"validation", "test"}
    assert [metrics.split_for_id(uid) for uid in ids] == [metrics.split_for_id(uid) for uid in ids]


def test_exact_typed_and_document_metrics() -> None:
    text = "Dr. Schmidt alice@example.invalid"
    gold = (
        score.Span(4, 11, "SURNAME"),
        score.Span(12, 33, "EMAIL"),
    )
    document = score.Document("synthetic", text, "en", "", "synthetic", gold)
    predictions = [score.Span(4, 11, "PERSON"), score.Span(12, 33, "WRONG")]
    accumulator = metrics.ComparisonMetrics({"PERSON": ("SURNAME",), "WRONG": ()})
    accumulator.add(document, predictions)
    result = accumulator.result()
    assert result["pii_documents"] == 1
    assert result["leaking_documents"] == 0
    assert result["leaking_entities"] == 0
    assert result["typed_entities"]["tp"] == 1
    assert result["typed_entities"]["fp"] == 1
    assert result["typed_entities"]["fn"] == 1


def test_per_label_bytes_separate_gold_leaks_from_emitted_false_positives() -> None:
    document = score.Document(
        "synthetic", "abcdefghijklmno", "en", "", "synthetic",
        (score.Span(0, 4, "FIRSTNAME"), score.Span(6, 10, "EMAIL")),
    )
    predictions = [
        score.Span(0, 2, "name"),
        score.Span(5, 8, "email"),
        score.Span(12, 15, "custom:phone"),
    ]
    accumulator = metrics.ComparisonMetrics({
        "name": ("FIRSTNAME",),
        "email": ("EMAIL",),
        "custom:phone": ("PHONE",),
    })

    accumulator.add(document, predictions)

    result = accumulator.result()
    assert result["per_label_bytes"] == {
        "leaked_by_gold_label": {"EMAIL": 2, "FIRSTNAME": 2},
        "false_positive_by_prediction_label": {"custom:phone": 3, "email": 1},
    }
    assert sum(result["per_label_bytes"]["leaked_by_gold_label"].values()) == result["leaked_bytes"]
    assert sum(result["per_label_bytes"]["false_positive_by_prediction_label"].values()) == result["false_positive_bytes"]


def test_family_mapping_keeps_bytes_but_gives_no_typed_credit() -> None:
    text = "Dr. Schmidt"
    document = score.Document("synthetic", text, "en", "", "synthetic",
                              (score.Span(4, 11, "SURNAME"),))
    mapping = {"custom:family:name.counter": ("SURNAME",)}
    accumulator = metrics.ComparisonMetrics(
        mapping, typed_mapping=compare.typed_mapping_for_contract(mapping, "v3")
    )
    accumulator.add(document, [score.Span(4, 11, "custom:family:name.counter")])
    result = accumulator.result()
    assert result["leaked_bytes"] == 0
    assert result["false_positive_bytes"] == 0
    assert result["typed_entities"]["tp"] == 0
    assert result["typed_entities"]["fp"] == 1
    assert result["typed_entities"]["fn"] == 1


def test_v1_secret_types_get_credit_only_under_v1() -> None:
    for native, gold in (("custom:password", "PASSWORD"),
                         ("custom:security_token", "SECURITYTOKEN"),
                         ("custom:secret", "PASSWORD"),
                         ("secret", "SECURITYTOKEN")):
        mapping = {native: ()}
        document = score.Document("synthetic", "secret", "en", "", "synthetic",
                                  (score.Span(0, 6, gold),))
        prediction = [score.Span(0, 6, native)]
        for version, expected_tp in (("v1", 1), ("v2", 0), ("v3", 0)):
            accumulator = metrics.ComparisonMetrics(
                mapping, typed_mapping=compare.typed_mapping_for_contract(mapping, version)
            )
            accumulator.add(document, prediction)
            result = accumulator.result()
            assert result["leaked_bytes"] == 0
            assert result["typed_entities"]["tp"] == expected_tp


def test_common_view_ignores_noncommon_gold() -> None:
    text = "alice@example.invalid Dr. Schmidt"
    document = score.Document(
        "synthetic", text, "en", "", "synthetic",
        (score.Span(0, 21, "EMAIL"), score.Span(26, 33, "SURNAME")),
    )
    predictions = [score.Span(26, 33, "PERSON")]
    accumulator = metrics.ComparisonMetrics(
        {"PERSON": ("SURNAME",)}, frozenset({"EMAIL"})
    )
    accumulator.add(document, predictions)
    result = accumulator.result()
    assert result["leaked_bytes"] == 21
    assert result["false_positive_bytes"] == 0
    assert result["typed_entities"]["fn"] == 1


def test_threshold_selection_uses_validation_and_ignores_test() -> None:
    import compare

    def row(validation_leak: int, test_leak: int) -> dict:
        return {"contracts": {"v3": {"C": {"metrics": {"product_coverage": {
            "validation": {"leaked_bytes": validation_leak, "false_positive_bytes": 0},
            "test": {"leaked_bytes": test_leak, "false_positive_bytes": 0},
        }}}}}}

    report = {"tools": {
        "gliner": row(1, 100),
        "gliner-high-recall": row(2, 0),
    }}
    compare.select_thresholds(report)
    assert report["selected_threshold_rows"]["gliner"] == "gliner"


def test_policy_ablations_preserve_other_sections() -> None:
    import prepare_policies
    import tomllib

    source = '[ner]\nmodel_dir = "synthetic"\n\n[safety_net]\nbackend = "nym"\n\n[safety_net.nym]\nmodel_dir = "synthetic"\n\n[policy.rulepacks]\nbundled = ["core"]\n'
    result = prepare_policies.without_sections(
        source, ("ner", "safety_net", "safety_net.nym")
    )
    assert tomllib.loads(result) == {"policy": {"rulepacks": {"bundled": ["core"]}}}


def _char(text: str, gold: tuple[score.Span, ...], predictions: list[score.Span],
          **document_fields: object) -> tuple[dict, dict]:
    document = score.Document("synthetic", text, "en", "", "synthetic", gold, **document_fields)
    accumulator = metrics.ComparisonMetrics({"PERSON": ("SURNAME",), "ANY": ("SURNAME",)})
    accumulator.add(document, predictions)
    result = accumulator.result()
    return result["char_level"], result


def test_char_level_counts_code_points_not_bytes_for_umlauts() -> None:
    # "Grüße" is 7 bytes / 5 chars, so "Müller" is bytes 8..15 (7 bytes, 6 chars).
    text = "Grüße Müller"
    gold = (score.Span(8, 15, "SURNAME"),)
    char, result = _char(text, gold, [score.Span(7, 15, "PERSON")])  # also the space
    assert (char["tp"], char["fp"], char["fn"]) == (6, 1, 0)
    assert (result["true_positive_bytes"], result["false_positive_bytes"]) == (7, 1)
    assert char["unit"] == "unicode_code_point"
    assert char["precision"] == 6 / 7 and char["recall"] == 1.0
    assert abs(char["f2"] - 30 / 31) < 1e-12  # 5*(6/7)/(4*(6/7)+1)
    assert abs(char["f1"] - 12 / 13) < 1e-12


def test_char_level_differs_from_byte_level_for_cjk_partial_cover() -> None:
    # Four 3-byte characters; the prediction covers the first two only.
    text = "田中太郎 bob"
    gold = (score.Span(0, 12, "SURNAME"),)
    char, result = _char(text, gold, [score.Span(0, 6, "PERSON")])
    assert (char["tp"], char["fp"], char["fn"]) == (2, 0, 2)
    assert (result["true_positive_bytes"], result["leaked_bytes"]) == (6, 6)
    # Mixed script: 1 leaked ASCII char next to 2 leaked CJK chars.
    text = "田中 x"
    gold = (score.Span(0, 6, "SURNAME"), score.Span(7, 8, "SURNAME"))
    char, result = _char(text, gold, [score.Span(0, 3, "PERSON")])
    assert (char["tp"], char["fn"]) == (1, 2)
    assert (result["true_positive_bytes"], result["leaked_bytes"]) == (3, 4)


def test_char_level_merges_overlapping_gold_and_predictions() -> None:
    text = "abcdefghijkl"
    gold = (score.Span(0, 5, "SURNAME"), score.Span(3, 8, "SURNAME"))  # merged 0..8
    predictions = [score.Span(2, 6, "PERSON"), score.Span(5, 10, "ANY")]  # merged 2..10
    char, _ = _char(text, gold, predictions)
    assert (char["tp"], char["fp"], char["fn"]) == (6, 2, 2)
    for key in ("precision", "recall", "f1", "f2", "f5"):
        assert char[key] == 0.75


def test_char_level_recall_weighting() -> None:
    text = "a" * 100
    gold = (score.Span(0, 10, "SURNAME"),)
    leaky, _ = _char(text, gold, [score.Span(0, 5, "PERSON")])          # P=1 R=.5
    noisy, _ = _char(text, gold, [score.Span(0, 10, "PERSON"), score.Span(20, 30, "PERSON")])  # P=.5 R=1
    assert abs(leaky["f2"] - 5 * 1 * 0.5 / (4 * 1 + 0.5)) < 1e-12
    assert abs(noisy["f2"] - 5 * 0.5 * 1 / (4 * 0.5 + 1)) < 1e-12
    assert noisy["f2"] > leaky["f2"] and leaky["f1"] == noisy["f1"]
    assert noisy["f5"] > leaky["f5"]
    # Hand-computed pins: F5 = 26 P R / (25 P + R).
    assert abs(leaky["f5"] - 26 * 0.5 / (25 + 0.5)) < 1e-12  # 0.5098039...
    assert abs(noisy["f5"] - 26 * 0.5 / (25 * 0.5 + 1)) < 1e-12  # 0.9629629...


def test_char_level_empty_denominators_score_zero_not_one() -> None:
    assert metrics.f_beta(0.0, 0.0, 2) == 0.0
    empty, _ = _char("nothing here", (), [])
    assert (empty["tp"], empty["fp"], empty["fn"]) == (0, 0, 0)
    assert all(empty[key] == 0.0 for key in ("precision", "recall", "f1", "f2", "f5"))
    only_gold, _ = _char("alice", (score.Span(0, 5, "SURNAME"),), [])
    assert only_gold["fn"] == 5
    assert all(only_gold[key] == 0.0 for key in ("precision", "recall", "f1", "f2", "f5"))
    only_noise, _ = _char("alice", (), [score.Span(0, 5, "PERSON")])
    assert only_noise["fp"] == 5
    assert all(only_noise[key] == 0.0 for key in ("precision", "recall", "f1", "f2", "f5"))


def test_char_level_matches_byte_level_on_ascii() -> None:
    text = "Dr. Schmidt alice@example.invalid"
    gold = (score.Span(4, 11, "SURNAME"), score.Span(12, 33, "SURNAME"))
    char, result = _char(text, gold, [score.Span(4, 11, "PERSON"), score.Span(10, 20, "ANY")])
    assert char["tp"] == result["true_positive_bytes"]
    assert char["fp"] == result["false_positive_bytes"]
    assert char["fn"] == result["leaked_bytes"]


def test_char_level_respects_contract_ignored_bytes() -> None:
    text = "abcdefghijk"
    gold = (score.Span(0, 5, "SURNAME"),)
    excluded = (score.Span(6, 11, "OTHER"),)
    # Only ignored bytes covered: dropped, neither TP nor FP.
    char, _ = _char(text, gold, [score.Span(6, 11, "PERSON")], excluded_spans=excluded)
    assert (char["tp"], char["fp"], char["fn"]) == (0, 0, 5)
    # Overlaps gold tail and ignored bytes: the ignored part is not a false positive.
    char, _ = _char(text, gold, [score.Span(4, 11, "PERSON")], excluded_spans=excluded)
    assert (char["tp"], char["fp"], char["fn"]) == (1, 1, 4)


def test_char_level_rejects_offsets_inside_a_character() -> None:
    import pytest

    document = score.Document("synthetic", "Müller", "en", "", "synthetic",
                              (score.Span(0, 7, "SURNAME"),))
    accumulator = metrics.ComparisonMetrics({"PERSON": ("SURNAME",)})
    with pytest.raises(ValueError, match="inside a UTF-8 character"):
        accumulator.add(document, [score.Span(0, 2, "PERSON")])  # byte 2 is ü's second byte
