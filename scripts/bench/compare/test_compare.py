from __future__ import annotations

import json
import copy
import hashlib
import os
import re
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace

import pytest

import compare
import render
import layer_display
from layer_display import layer_display_name


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


def test_gaze_byte_equality_fails_on_each_mismatch() -> None:
    expected = {field: 0 for field in ("leaked_bytes", "false_positive_bytes",
                                       "gold_gap_protected_bytes", "false_positive_bytes_after_gold_gap")}
    compare.assert_gaze_byte_equality(expected, expected, "v3", "C")
    for field in expected:
        measured = dict(expected, **{field: 1})
        with pytest.raises(ValueError, match=f"Gaze byte mismatch: v3/C/{field}"):
            compare.assert_gaze_byte_equality(measured, expected, "v3", "C")


def test_public_page_lists_no_gaze_row_and_names_why() -> None:
    report = json.loads((compare.REPO / "docs/reference/benchmarks/comparison.json").read_text())
    page = render.render(report, "comparison.json")
    assert "| gaze |" not in page
    assert "Where Gaze trails" not in page and "Gaze ablations" not in page
    assert "Gaze is not listed" in page
    assert "presidio-strong" in page
    # Mutating the untagged Gaze run cannot change the public page.
    report["gaze"]["v3"]["layers"]["C"]["leaked_bytes"] += 999
    assert render.render(report, "comparison.json") == page


def _assert_table_columns_match(markdown: str) -> int:
    lines = markdown.splitlines()
    tables = 0
    for index, line in enumerate(lines[1:], 1):
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if not line.startswith("|") or not all(re.fullmatch(r":?-{3,}:?", cell) for cell in cells):
            continue
        header = lines[index - 1]
        assert header.startswith("|"), f"delimiter without header at line {index + 1}"
        assert header.count("|") == line.count("|"), f"table width differs at line {index + 1}"
        tables += 1
    assert tables, "no Markdown tables found"
    return tables


def test_every_generated_table_has_matching_header_and_delimiter() -> None:
    from render_benchmark_doc import (
        empty_history, load_history, render_current_release, render_history, render_latency,
    )

    report = json.loads((compare.REPO / "docs/reference/benchmarks/comparison.json").read_text())
    history = load_history(compare.REPO / "docs/reference/benchmarks/release-history.json")
    outputs = (
        render.render(report, "comparison.json"),
        render_current_release(history),
        render_history(history),
        render_history(empty_history()),
        render_latency(history, {}),
    )
    assert sum(_assert_table_columns_match(output) for output in outputs) >= 8
    broken = outputs[0].replace("| --- |", "| --- | --- |", 1)
    with pytest.raises(AssertionError, match="table width differs"):
        _assert_table_columns_match(broken)


def test_corpus_display_names_drive_both_public_renderers(monkeypatch: pytest.MonkeyPatch) -> None:
    from render_benchmark_doc import chart_files, load_history

    assert set(layer_display.LAYER_DISPLAY_NAMES) == {"C", "A", "D", "R"}
    report = json.loads((compare.REPO / "docs/reference/benchmarks/comparison.json").read_text())
    history = load_history(compare.REPO / "docs/reference/benchmarks/release-history.json")
    their = json.loads(
        (compare.REPO / "docs/reference/benchmarks/their-benchmarks.json").read_text()
    )["benchmarks"]
    current_page = render.render(report, "comparison.json")
    current_chart = "".join(chart_files(history, report, their).values())
    assert layer_display_name("C") in current_page
    assert layer_display_name("C") in current_chart
    monkeypatch.setitem(layer_display.LAYER_DISPLAY_NAMES, "C", "Mutated corpus name")
    assert "Mutated corpus name" in render.render(report, "comparison.json")
    assert "Mutated corpus name" in "".join(chart_files(history, report, their).values())
    assert current_page != render.render(report, "comparison.json")
    assert current_chart != "".join(chart_files(history, report, their).values())


def test_local_prediction_replay_matches_live_metrics_without_text(tmp_path: Path) -> None:
    document = compare.score.Document(
        "synthetic-1", "alice@example.invalid", "en", "", "synthetic",
        (compare.score.Span(0, 21, "EMAIL"),),
    )
    layers = {"C": [document]}
    mapping = {"EMAIL": ("EMAIL",)}
    predictions = [compare.score.Span(0, 21, "EMAIL")]
    live = compare.measure("synthetic-tool", lambda _document: predictions, layers, mapping,
                           common_labels=frozenset({"EMAIL"}), predictions_dir=tmp_path)
    saved = compare.prediction_path(tmp_path, "C", "synthetic-tool")
    row = json.loads(saved.read_text())
    assert row == {"uid": "synthetic-1", "spans": [{"start": 0, "end": 21, "label": "EMAIL"}]}
    replay = compare.rescore_predictions("synthetic-tool", layers, mapping,
                                         frozenset({"EMAIL"}), tmp_path)
    for version in compare.CONTRACTS:
        current = live["contracts"][version]["C"]
        restored = replay["contracts"][version]["C"]
        assert current["metrics"] == restored["metrics"]
        for field in ("leaked_bytes", "false_positive_bytes", "gold_gap_protected_bytes",
                      "false_positive_bytes_after_gold_gap", "documents", "processed_documents"):
            assert current[field] == restored[field]
    saved.write_text(json.dumps({**row, "text": document.text}) + "\n")
    with pytest.raises(ValueError, match="shape mismatch"):
        compare.rescore_predictions("synthetic-tool", layers, mapping,
                                    frozenset({"EMAIL"}), tmp_path)


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
        "gaze": {version: {"layers": {"A": copy.deepcopy(row)}, "gaze_revision": "synthetic",
                           "policy_sha256": "synthetic"} for version in ("v1", "v2", "v3")},
        "tools": {
            name: {
                "contracts": {version: {"A": copy.deepcopy(row)} for version in ("v1", "v2", "v3")},
                "provenance": {"analyzer_version": "test", "spacy_version": "test",
                               "gliner_version": "test", "model_snapshot": "test",
                               "models": {"en": {"sha256": "test", "wheel_sha256": "test"}},
                               "model_sha256": "test"},
            }
            for name in ("presidio-all", "presidio-en", "presidio-en-de", "gliner")
        },
        "skipped": {"opf": "synthetic test"},
    }
    page = render.render(report, "comparison.json")
    assert "| gaze |" not in page
    assert f"| v3 | {layer_display_name('A')} | presidio-all | 1 | 2 | 1 | 0 | 0 | 3.0 | 4.0 |" in page
    assert "Gaze p50 exceeds" not in page
    for field, value in (("documents", 2), ("processed_documents", 2)):
        broken = copy.deepcopy(report)
        broken["tools"]["presidio-all"]["contracts"]["v3"]["A"][field] = value
        with pytest.raises(ValueError, match="document count"):
            render.render(broken, "comparison.json")
    broken = copy.deepcopy(report)
    broken["tools"]["presidio-all"]["contracts"]["v3"]["A"]["latency"]["samples"] = 0
    with pytest.raises(ValueError, match="latency includes skipped documents"):
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


def test_presidio_partial_overlap_keeps_raw_coordinates() -> None:
    from presidio_analyzer import RecognizerResult
    from presidio_anonymizer import AnonymizerEngine

    text = "x" * 127 + "Northwind Demo Postal 12345" + "x" * 15
    found = [
        RecognizerResult("ORGANIZATION", 127, 150, 0.9),
        RecognizerResult("DE_PLZ", 147, 152, 0.8),
    ]
    backend = compare.Presidio.__new__(compare.Presidio)
    backend.languages = ["en"]
    backend.analyzer = SimpleNamespace(analyze=lambda **_kwargs: found)
    backend.anonymizer = AnonymizerEngine()
    document = compare.score.Document("synthetic", text, "en", "", "synthetic", ())
    actual = backend.predict(document)
    expected = [
        compare.score.Span(127, 150, "ORGANIZATION"),
        compare.score.Span(147, 152, "DE_PLZ"),
    ]
    assert actual == expected
    assert compare.resolved_presidio_spans(backend.anonymizer, text, found) == expected


def test_presidio_languages_cover_each_configuration() -> None:
    assert compare.presidio_languages("presidio-all") == ("en", "de", "nl", "fr", "pt")
    assert compare.presidio_languages("presidio-en") == ("en",)
    assert compare.presidio_languages("presidio-en-de") == ("en", "de")
    with pytest.raises(ValueError, match="unknown Presidio configuration"):
        compare.presidio_languages("gliner")


def test_presidio_anonymizer_version_mismatch_fails_before_model_load(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
) -> None:
    assert compare.package_version("presidio-anonymizer") == compare.PRESIDIO_ANONYMIZER_VERSION
    monkeypatch.setattr(compare, "package_version", lambda _name: "2.2.365")
    with pytest.raises(RuntimeError, match="requires presidio-anonymizer==2.2.364"):
        compare.Presidio({"en": tmp_path / "missing-model"})


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


def test_run_full_wrapper_parses_every_model_and_policy_arg() -> None:
    env = os.environ.copy()
    env["GAZE_COMPARE_PYTHON"] = sys.executable
    for language in compare.PRESIDIO_LANGUAGES:
        env[f"GAZE_COMPARE_{language.upper()}_MODEL"] = f"/synthetic/{language}_model"
    env["GAZE_COMPARE_GLINER_MODEL"] = "/synthetic/gliner_model"
    result = subprocess.run(
        ["bash", str(compare.MAP_PATH.with_name("run-full.sh")), "--dry-run"],
        cwd=compare.REPO, env=env, text=True, capture_output=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "COMPARISON_ARGS_OK"


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
        "comparison_metrics_sha256": render.digest_file(compare.MAP_PATH.with_name("comparison_metrics.py")),
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
