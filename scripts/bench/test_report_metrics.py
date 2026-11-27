from __future__ import annotations

from pathlib import Path

import pytest

import report_metrics as metrics

ROOT = Path(__file__).resolve().parents[2]
V30_RECORD = ROOT / "docs/reference/benchmarks/history-generator-v30/v0.17.0/observations-v1.jsonl.gz"
TYPES = {"FIRSTNAME": frozenset({"name"}), "SURNAME": frozenset({"name"}),
         "CITY": frozenset({"location"}), "TELEPHONENUM": frozenset({"custom:phone"})}
UNTYPED = {"RECORD_ID": "tenant"}


def scored(*items):
    return metrics.score_items(items, TYPES, UNTYPED)


def item(text, gold, predictions=(), **kwargs):
    return metrics.Item("d", "A", text, gold, predictions, **kwargs)


def test_exposure_counts_letters_and_digits_only():
    text = "Anna Weber, Berlin, 0301234 and Kiel."
    gold = [(0, 10, "FIRSTNAME"), (12, 18, "CITY"), (20, 27, "TELEPHONENUM"), (32, 36, "CITY")]
    # "Anna" and "Weber" hidden by two spans with the space between visible: fully hidden.
    # "Berlin" loses only "Ber": partial with 3 residue. The phone and Kiel are missed.
    predictions = [(0, 4, "name"), (5, 10, "name"), (15, 18, "location")]
    exposure = scored(item(text, gold, predictions))["exposure"]
    assert (exposure["fully_hidden"], exposure["partially_exposed"], exposure["completely_missed"]) == (1, 1, 2)
    assert exposure["partial_residue_characters"] == 3
    assert exposure["missed_residue_characters"] == 7 + 4


def test_merged_exact_spans_join_only_one_type_across_spaces():
    text = "Anna Weber Berlin"
    gold = [(0, 4, "FIRSTNAME"), (5, 10, "SURNAME"), (11, 17, "CITY")]
    exact = scored(item(text, gold, [(0, 10, "name"), (11, 17, "location")]))["exact_span"]
    assert exact["raw"]["recall"] == pytest.approx(100 / 3, abs=0.01)
    # FIRSTNAME and SURNAME share a type and merge; CITY does not join them.
    assert exact["merged"] == {"precision": 100.0, "recall": 100.0, "f1": 100.0}


def test_type_agreement_is_judged_on_typed_labels_only():
    text = "Anna 0301234 R-1"
    gold = [(0, 4, "FIRSTNAME"), (5, 12, "TELEPHONENUM"), (13, 16, "RECORD_ID")]
    predictions = [(0, 4, "organization"), (5, 12, "custom:phone"), (13, 16, "custom:x")]
    types = scored(item(text, gold, predictions))["type_agreement"]
    assert (types["typed_fully_hidden"], types["agrees"], types["untyped_fully_hidden"]) == (2, 1, 1)
    assert types["top_disagreements"] == [{"label": "FIRSTNAME", "classes": "organization", "spans": 1}]


def test_an_unruled_label_stops_the_report():
    with pytest.raises(metrics.MetricsError, match="NEWLABEL"):
        scored(item("abc", [(0, 3, "NEWLABEL")]))


def test_neutral_characters_are_neither_predictions_nor_strays():
    text = "Anna met Acme"
    value = scored(item(text, [(0, 4, "FIRSTNAME")], [(0, 4, "name"), (9, 13, "organization")],
                        neutral=[(9, 13)]))["exact_span"]
    assert (value["predicted_spans"], value["stray_predicted_spans"]) == (1, 0)
    value = scored(item(text, [(0, 4, "FIRSTNAME")], [(0, 4, "name"), (9, 13, "organization")]))["exact_span"]
    assert (value["predicted_spans"], value["stray_predicted_spans"]) == (2, 1)


def test_harmless_masking_counts_touched_decoys_and_masked_benign_documents():
    benign = metrics.Item("b", "D", "order 4711 shipped", [], [(6, 10, "custom:phone")],
                          decoys=[(6, 10), (11, 18)], benign=True)
    clean = metrics.Item("c", "D", "nothing here", [], [], benign=True)
    harmless = scored(benign, clean)["harmless_masking"]
    assert (harmless["decoys"], harmless["decoys_masked"]) == (2, 1)
    assert (harmless["benign_documents"], harmless["benign_documents_masked"]) == (2, 1)


def test_every_generated_label_has_a_type_ruling():
    import agentic_layers as agentic

    labels = {g.label for record in agentic.generate(agentic.PUBLISHED_PARTITION) for g in record.gold}
    metrics.check_labels(labels, metrics.TYPE_CLASSES, metrics.UNTYPED)


def test_v30_release_record_metrics():
    # Pinned against the immutable v0.17.0 generator-v30 record, without the
    # holdout parquet: its 1,886 Dataiku documents are unread.
    value = metrics.report(V30_RECORD)
    assert value["skipped_documents"] == {"C:unread": 1886}
    layer_a, layer_d = value["layers"]["A"], value["layers"]["D"]
    assert layer_a["exposure"]["fully_hidden"] == 7006
    assert layer_a["type_agreement"]["agreement_pct"] == 97.52
    assert layer_d["harmless_masking"]["benign_documents_masked"] == 1925
    assert layer_d["harmless_masking"]["decoys_masked"] == 712
    assert value["layers"]["C"]["harmless_masking"]["benign_documents_masked"] == 321

