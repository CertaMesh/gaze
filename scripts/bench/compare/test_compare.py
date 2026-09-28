from __future__ import annotations

import json
import copy
import hashlib
from pathlib import Path
from types import SimpleNamespace

import pytest

import compare
import render


def test_native_character_offsets_become_utf8_bytes() -> None:
    spans = compare.byte_spans("Ä alice@example.invalid", [(2, 23, "EMAIL_ADDRESS")])
    assert spans == [compare.score.Span(3, 24, "EMAIL_ADDRESS")]


def test_published_paths_do_not_expose_home_directory() -> None:
    assert compare.portable_path(Path.home() / ".local" / "bin" / "python") == "$HOME/.local/bin/python"


def test_pack_requires_byte_aligned_gold_and_unique_ids(tmp_path: Path) -> None:
    row = {
        "id": "synthetic-1", "partition": "sealed", "layer": "V",
        "language": "de", "region": "DE", "text": "Ä", "gold": [
            {"start": 0, "end": 2, "label": "FIRSTNAME"}
        ],
    }
    path = tmp_path / "pack.jsonl"
    path.write_text(json.dumps(row) + "\n", encoding="utf-8")
    layers, provenance = compare.load_pack(path)
    assert list(layers) == ["V/pack/sealed"]
    assert layers["V/pack/sealed"][0].spans == (compare.score.Span(0, 2, "FIRSTNAME"),)
    assert provenance["documents"] == 1

    row["gold"][0]["end"] = 1
    path.write_text(json.dumps(row) + "\n", encoding="utf-8")
    with pytest.raises(ValueError, match="UTF-8"):
        compare.load_pack(path)


def test_contract_v3_uses_reviewed_native_mapping_for_repeat_credit() -> None:
    contract = compare.runner.load_scored_label_contract(
        compare.REPO, compare.CONTRACTS["v3"]
    )
    text = "Dr. Schmidt met Dr. Schmidt."
    start = text.index("Schmidt")
    document = compare.score.Document(
        "synthetic", text, "de", "DE", "synthetic",
        (compare.score.Span(start, start + 7, "SURNAME"),),
    )
    applied = compare.score.apply_scored_label_contract([document], contract)[0]
    native = compare.byte_spans(text, [(text.rindex("Schmidt"), text.rindex("Schmidt") + 7, "PERSON")])
    accumulator = compare.score.MetricAccumulator()
    accumulator.add(compare.mapped_document(applied, compare.load_mapping()["presidio"]), native)
    assert accumulator.result()["gold_gap"]["gold_gap_protected_bytes"] == 7


def test_unmapped_tool_label_fails_closed() -> None:
    with pytest.raises(ValueError, match="unmapped"):
        compare.validate_labels([compare.score.Span(0, 1, "UNKNOWN")], {})


def test_v3_layer_without_gold_gap_uses_raw_false_positives() -> None:
    row = {
        "leaked_bytes": 1,
        "false_positive_bytes": 2,
        "false_positive_bytes_after_gold_gap": None,
        "documents": 1, "processed_documents": 1, "skipped_documents": 0,
        "skipped_gold_bytes": 0,
        "latency": {"p50_ms": 3, "p95_ms": 4, "samples": 1},
    }
    report = {
        "corpus": {"layers": {"A": {"documents": 1}}},
        "policy_sha256_home_normalized": "synthetic",
        "harness_dirty": False,
        "latest_release_at_measurement": {"version": "v0.0.0"},
        "gaze": {version: {"layers": {"A": row}, "gaze_revision": "synthetic",
                           "policy_sha256": "synthetic"} for version in ("v1", "v2", "v3")},
        "tools": {
            name: {
                "contracts": {version: {"A": row} for version in ("v1", "v2", "v3")},
                "provenance": {"analyzer_version": "test", "spacy_version": "test",
                               "gliner_version": "test", "model_snapshot": "test",
                               "models": {"en": {"sha256": "test", "wheel_sha256": "test"}},
                               "model_sha256": "test"},
            }
            for name in ("presidio-all", "presidio-en", "presidio-en-de", "gliner")
        },
        "skipped": {"opf": "synthetic test"},
    }
    assert "| v3 | A | gaze | 1 | 2 | 1 | 0 | 0 | 3.0 | 4.0 |" in render.render(report, "comparison.json")
    for field, value in (("documents", 2), ("processed_documents", 2)):
        broken = copy.deepcopy(report)
        broken["tools"]["presidio-all"]["contracts"]["v3"]["A"][field] = value
        with pytest.raises(ValueError, match="document count"):
            render.render(broken, "comparison.json")
    broken = copy.deepcopy(report)
    broken["tools"]["gliner"]["provenance"]["model_sha256"] = ""
    with pytest.raises(ValueError, match="model hash"):
        render.render(broken, "comparison.json")
    for name in ("presidio-all", "presidio-en", "presidio-en-de"):
        broken = copy.deepcopy(report)
        broken["tools"][name]["provenance"]["models"]["en"]["wheel_sha256"] = ""
        with pytest.raises(ValueError, match="model hash"):
            render.render(broken, "comparison.json")
    broken = copy.deepcopy(report)
    broken["gaze"]["v1"]["gaze_revision"] = "other"
    with pytest.raises(ValueError, match="different measured revisions"):
        render.render(broken, "comparison.json")
    broken = copy.deepcopy(report)
    broken["harness_dirty"] = True
    with pytest.raises(ValueError, match="clean harness"):
        render.render(broken, "comparison.json")
    broken = copy.deepcopy(report)
    broken["tools"]["opf"] = {"provenance": {"runtime": {"source_dirty": True}}}
    with pytest.raises(ValueError, match="OPF source"):
        render.render(broken, "comparison.json")
    broken["tools"]["opf"]["provenance"]["runtime"]["source_dirty"] = False
    with pytest.raises(ValueError, match="model hash"):
        render.render(broken, "comparison.json")


def test_presidio_keep_spans_match_previous_resolution_on_synthetic_text() -> None:
    from presidio_analyzer import RecognizerResult
    from presidio_anonymizer import AnonymizerEngine
    from presidio_anonymizer.entities import ConflictResolutionStrategy

    text = "Dr. Schmidt emailed alice@example.invalid."
    found = [
        RecognizerResult("PERSON", 0, 11, 0.9),
        RecognizerResult("PERSON", 4, 11, 0.7),
        RecognizerResult("EMAIL_ADDRESS", 20, 41, 0.9),
    ]
    backend = compare.Presidio.__new__(compare.Presidio)
    backend.languages = ["en"]
    backend.analyzer = SimpleNamespace(analyze=lambda **_kwargs: found)
    backend.anonymizer = AnonymizerEngine()
    document = compare.score.Document("synthetic", text, "en", "", "synthetic", ())
    actual = backend.predict(document)

    copied = backend.anonymizer._copy_recognizer_results(found)
    copied.sort(key=lambda item: (item.start, item.end))
    resolved = backend.anonymizer._remove_conflicts_and_get_text_manipulation_data(
        copied, ConflictResolutionStrategy.MERGE_SIMILAR_OR_CONTAINED
    )
    resolved = backend.anonymizer._merge_entities_with_spaces_between(text, resolved)
    expected = compare.byte_spans(text, [(item.start, item.end, item.entity_type) for item in resolved])
    assert sorted(actual, key=lambda item: item.start) == sorted(expected, key=lambda item: item.start)


def test_german_presidio_labels_are_reviewed() -> None:
    from presidio_analyzer import predefined_recognizers

    mapping = compare.load_mapping()["presidio"]
    for name in compare.GERMAN_RECOGNIZERS:
        for label in getattr(predefined_recognizers, name + "Recognizer")().supported_entities:
            assert label in mapping


def test_processed_latency_excludes_skipped_language() -> None:
    text = "alice@example.invalid"
    documents = [
        compare.score.Document("en", "hello", "en", "", "synthetic", ()),
        compare.score.Document("de", text, "de", "DE", "synthetic",
                               (compare.score.Span(0, len(text), "EMAIL"),)),
    ]
    result = compare.measure("presidio-en", lambda _doc: [], {"C": documents},
                             compare.load_mapping()["presidio"], ["en"])
    row = result["contracts"]["v3"]["C"]
    assert (row["documents"], row["processed_documents"], row["skipped_documents"]) == (2, 1, 1)
    assert row["skipped_gold_bytes"] == len(text)
    assert row["latency"]["samples"] == 1


def test_policy_hash_normalizes_home_after_raw_verification(tmp_path: Path) -> None:
    raw = f'root = "{Path.home()}/models"\n'.encode()
    path = tmp_path / "policy.toml"
    path.write_bytes(raw)
    expected = hashlib.sha256(raw.replace(str(Path.home()).encode(), b"$HOME")).hexdigest()
    assert compare.normalized_policy_sha256(path, hashlib.sha256(raw).hexdigest()) == expected
    with pytest.raises(ValueError, match="differs"):
        compare.normalized_policy_sha256(path, "wrong")


def test_spacy_model_provenance_uses_pinned_wheel(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    model = tmp_path / "model"
    model.mkdir()
    (model / "meta.json").write_text('{"version":"3.7.1","license":"MIT"}')
    monkeypatch.setattr(compare, "digest_tree", lambda _path: "installed-content")
    info = compare.model_info(model, "en")
    assert info["sha256"] == "installed-content"
    assert info["wheel_sha256"] == json.loads(compare.MODEL_PINS_PATH.read_text())["en_core_web_lg"]["sha256"]
    (model / "meta.json").write_text('{"version":"0.0.0","license":"MIT"}')
    with pytest.raises(ValueError, match="pinned wheel"):
        compare.model_info(model, "en")


def test_validate_current_flags_competitor_input_but_not_gaze_drift(monkeypatch: pytest.MonkeyPatch) -> None:
    report = json.loads((compare.REPO / "docs/reference/benchmarks/comparison.json").read_text())
    report.update({
        "scorer_sha256": render.digest_file(render.BENCH / "gaze_bench_score.py"),
        "dataset_loader_sha256": render.digest_file(render.BENCH / "dataiku_en_de_gaze_bench.py"),
        "mapping_sha256": render.digest_file(compare.MAP_PATH),
        "model_pins_sha256": render.digest_file(compare.MODEL_PINS_PATH),
        "compare_sha256": render.digest_file(Path(compare.__file__)),
        "opf_adapter_sha256": render.digest_file(render.BENCH / "opf_daemon.py"),
        "requirements_sha256": render.digest_file(compare.MAP_PATH.with_name("requirements.lock")),
    })
    monkeypatch.setattr(compare.agentic, "prepare", lambda _repo: SimpleNamespace(manifest=report["corpus"]["agentic"]))
    monkeypatch.setattr(compare.agentic, "load_contract", lambda _repo: SimpleNamespace(sha256=report["contracts"]["agentic"]))
    report["runner_sha256"] = "changed Gaze runner"
    report["gaze_crates_tree"] = "changed Gaze crates"
    report["latest_release_at_measurement"] = {"version": "older release", "scorecard_sha256": "changed"}
    render.validate_current(report)
    report["mapping_sha256"] = "changed competitor mapping"
    with pytest.raises(ValueError, match="mapping changed"):
        render.validate_current(report)
    report["mapping_sha256"] = render.digest_file(compare.MAP_PATH)
    report["compare_sha256"] = "changed adapter"
    with pytest.raises(ValueError, match="comparison adapter changed"):
        render.validate_current(report)
    report["compare_sha256"] = render.digest_file(Path(compare.__file__))
    report["model_pins_sha256"] = "changed model pins"
    with pytest.raises(ValueError, match="model pins changed"):
        render.validate_current(report)


def test_public_page_rejects_partial_competitor_run() -> None:
    with pytest.raises(ValueError, match="every configured competitor"):
        render.render({"harness_dirty": False,
                       "gaze": {version: {} for version in ("v1", "v2", "v3")},
                       "tools": {}, "corpus": {"layers": {}}}, "comparison.json")


def test_timeout_reports_layer_position_without_document_text() -> None:
    document = compare.score.Document(
        "synthetic", "alice@example.invalid", "en", "", "synthetic", (),
    )

    def timeout(_document: compare.score.Document) -> list[compare.score.Span]:
        raise TimeoutError("socket timeout")

    with pytest.raises(RuntimeError, match=r"opf timed out in layer C at document 1/1"):
        compare.measure("opf", timeout, {"C": [document]}, compare.load_mapping()["opf"])
