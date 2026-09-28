from __future__ import annotations

import comparison_metrics as metrics
import gaze_bench_score as score


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
