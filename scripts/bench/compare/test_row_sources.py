"""The finalizer and renderer preserve measurement identities across a two-row supplement."""

from __future__ import annotations

import copy
import json
import sys
from pathlib import Path

import pytest

import compare
import finalize_report
import render
import row_sources
from comparison_availability import REASON, digest


def write(path: Path, value: dict) -> str:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return row_sources.file_hash(path)


@pytest.fixture
def inputs(tmp_path, monkeypatch):
    text = "alice@example.invalid"
    layers = {layer: [compare.score.Document(f"fictional-{layer}", text, "es", "ES", "synthetic",
                     (compare.score.Span(0, len(text), "EMAIL"),))] for layer in ("C", "A", "D", "R")}
    mapping = compare.load_mapping()["scrubadub"]
    measured = compare.measure("synthetic", lambda _: [], layers, mapping)
    old_scrub = compare.measure("synthetic", lambda _: [], layers, mapping, ("en", "de", "nl", "fr", "pt"))
    fresh_scrub = compare.measure("synthetic", lambda _: [compare.score.Span(0, len(text), "email")], layers, mapping)
    corpus = {
        "layers": {layer: {"documents": 1} for layer in layers},
        "main_dataset": {"sha256": compare.dataiku.DATASET_SHA256},
        "negative_corpus_sha256": render.digest_file(compare.REPO / "crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl"),
        "agentic": {"corpus_sha256": "c" * 64, "generator_version": compare.agentic.GENERATOR_VERSION},
        "packs": [],
    }
    monkeypatch.setattr(render, "agentic_identity", lambda _: ("c" * 64, "d" * 64))
    model = {"sha256": "e" * 64, "wheel_sha256": "f" * 64}
    provenance = {"analyzer_version": "fictional", "spacy_version": "fictional", "models": {"en": model},
                  "gliner_version": "fictional", "model_snapshot": "fictional", "model_sha256": "e" * 64,
                  "tokenizer_sha256": "f" * 64, "spacy_model": model, "transformer": model,
                  "supported_languages": ["en", "de", "nl", "fr", "pt"],
                  "measurement_harness_revision": "a" * 40, "measurement_compare_sha256": row_sources.ORIGINAL_ADAPTER}
    tools = {name: {**copy.deepcopy(measured), "tool": name, "provenance": copy.deepcopy(provenance)}
             for name in compare.TOOLS if name != "opf"}
    for name in row_sources.REPLACED:
        tools[name]["contracts"] = copy.deepcopy(old_scrub["contracts"])
    base = {
        "schema_version": 2, "harness_revision": "a" * 40, "harness_dirty": False,
        "generated_at": "2026-01-01T00:00:00+00:00", "compare_sha256": row_sources.ORIGINAL_ADAPTER,
        "corpus": corpus, "contracts": {**{v: compare.runner.load_scored_label_contract(compare.REPO, p).sha256
                                               for v, p in compare.CONTRACTS.items()}, "agentic": "d" * 64},
        "tools": tools, "skipped": {}, "hardware": "fictional CPU", "device": "cpu",
        "common_intersection_labels": ["EMAIL"], "heldout_split": {"fictional": True},
        "runner_sha256": "1" * 64, "policy_sha256_home_normalized": "2" * 64,
        "gaze": {version: {"gaze_revision": "a" * 40, "layers": copy.deepcopy(contract)}
                 for version, contract in measured["contracts"].items()},
        "gaze_ablations": {}, "gaze_inference": {"binary_sha256": "3" * 64},
        "selected_threshold_rows": {"gliner": "gliner-high-recall"},
        "threshold_validation": {"fictional": {"leaked_bytes": 0}},
        "contention_samples": {name: {"contended": False} for name in (*tools, "gaze-full", "gaze-rules-only", "gaze-rules-ner")},
    }
    pinned = {
        "scorer_sha256": render.BENCH / "gaze_bench_score.py",
        "dataset_loader_sha256": render.BENCH / "dataiku_en_de_gaze_bench.py",
        "mapping_sha256": compare.MAP_PATH, "model_pins_sha256": compare.MODEL_PINS_PATH,
        "opf_adapter_sha256": render.BENCH / "opf_daemon.py",
        "requirements_sha256": compare.MAP_PATH.with_name("requirements.lock"),
        "scrubadub_requirements_sha256": compare.MAP_PATH.with_name("requirements-scrubadub.lock"),
        "comparison_metrics_sha256": compare.MAP_PATH.with_name("comparison_metrics.py"),
        "cpu_contention_sha256": compare.MAP_PATH.with_name("cpu_contention.py"),
    }
    base.update({key: render.digest_file(path) for key, path in pinned.items()})
    receipt = dict(schema_version=1, tool="opf", status="unavailable", reason=REASON,
                   measurement_revision=base["harness_revision"], comparison_adapter_sha256=base["compare_sha256"],
                   corpus_sha256=digest(corpus), contracts_sha256=digest(base["contracts"]),
                   checkpoint_sha256="4" * 64, runtime_source_revision="5" * 40)
    configuration = digest(receipt)
    receipt["attempts"] = [dict(attempt=n, timeout_seconds=600, outcome="socket_timeout",
                                log_sha256=str(n) * 64, configuration_sha256=configuration) for n in (1, 2)]
    base["unavailable"] = {"opf": receipt}
    finalize_report.finalize(base)
    base["finalizer_sha256"] = row_sources.ORIGINAL_FINALIZER
    supplement = copy.deepcopy(base)
    for key in ("gaze_inference", "unavailable", "latency_validity", "chart_config_sha256", "finalizer_sha256"):
        del supplement[key]
    supplement.update(harness_revision="b" * 40, generated_at="2026-02-01T00:00:00+00:00",
                      compare_sha256=row_sources.SPANISH_ADAPTER,
                      tools={name: copy.deepcopy(tools[name]) for name in row_sources.REPLACED},
                      contention_samples={name: {"contended": True} for name in row_sources.REPLACED},
                      gaze={}, gaze_ablations={})
    for tool in supplement["tools"].values():
        tool["provenance"]["supported_languages"].append("es")
        for key in ("measurement_harness_revision", "measurement_compare_sha256"):
            del tool["provenance"][key]
        tool["contracts"] = copy.deepcopy(fresh_scrub["contracts"])
    base_path, supplement_path = tmp_path / "original.json", tmp_path / "spanish.json"
    write(base_path, base)
    write(supplement_path, supplement)
    base_native, supplement_native = tmp_path / "base-native", tmp_path / "supplement-native"
    for source, root in ((base, base_native), (supplement, supplement_native)):
        names = list(source["tools"])
        if source.get("gaze_inference"):
            names += ["gaze-full", "gaze-rules-only", "gaze-rules-ner"]
        for name in names:
            for layer in layers:
                path = compare.prediction_path(root, layer, name)
                path.parent.mkdir(parents=True, exist_ok=True)
                spans = ([{"start": 0, "end": len(text), "label": "email"}] if root == supplement_native else [])
                path.write_text(json.dumps({"uid": f"fictional-{layer}", "spans": spans}) + "\n")
    return base_path, supplement_path, base_native, supplement_native, tmp_path / "comparison.json"


def compose_cli(inputs, monkeypatch, *, base_sha=None, supplement_sha=None):
    base, supplement, old_native, new_native, output = inputs
    monkeypatch.setattr(sys, "argv", ["finalize_report.py", str(output), "--compose-base", str(base),
        "--base-sha256", base_sha or row_sources.file_hash(base), "--supplement", str(supplement),
        "--supplement-sha256", supplement_sha or row_sources.file_hash(supplement),
        "--base-predictions-dir", str(old_native), "--supplement-predictions-dir", str(new_native)])
    finalize_report.main()
    return json.loads(output.read_bytes())


def render_cli(output, page, monkeypatch):
    monkeypatch.setattr(sys, "argv", ["render.py", str(output), "--page", str(page)])
    render.main()


def test_composition_and_renderer_preserve_original_measurements(inputs, monkeypatch):
    base, supplement, _, _, output = inputs
    original_bytes, supplement_bytes = base.read_bytes(), supplement.read_bytes()
    old, new = json.loads(original_bytes), json.loads(supplement_bytes)
    result = compose_cli(inputs, monkeypatch)
    assert base.read_bytes() == original_bytes
    assert supplement.read_bytes() == supplement_bytes
    unchanged = set(old["tools"]) - set(row_sources.REPLACED)
    assert len(unchanged) == 11
    for name in unchanged:
        assert result["tools"][name] == old["tools"][name]
        assert result["row_sources"]["tool_sources"][name] == "base"
    for name in row_sources.REPLACED:
        assert result["tools"][name] == new["tools"][name]
        assert result["row_sources"]["tool_sources"][name] == "supplement"
    for key in ("unavailable", "gaze", "gaze_inference", "gaze_ablations", "selected_threshold_rows", "threshold_validation",
                "harness_revision", "generated_at", "compare_sha256"):
        assert result[key] == old[key]
    assert result["row_sources"]["aggregation"]["compare_sha256"] == row_sources.SPANISH_ADAPTER
    assert not result["latency_validity"]["publishable"]
    assert result["row_sources"]["base"]["native_records"]["gliner"]["A"]["sha256"] == row_sources.file_hash(inputs[2] / "A/pred/gliner.jsonl")
    page = output.with_suffix(".md")
    render_cli(output, page, monkeypatch)
    text = page.read_text()
    assert "Mixed-source publication" in text
    assert "eleven configurations" in text
    assert "only scrubadub-base and scrubadub-spacy" in text
    assert "OPF: N/A" in text
    assert "Timing comparisons remain withheld" in text
    # Running ordinary finalization cannot normalize or relabel composed rows.
    monkeypatch.setattr(sys, "argv", ["finalize_report.py", str(output)])
    finalize_report.main()
    assert json.loads(output.read_bytes()) == result


def test_original_snapshot_still_renders_with_its_legitimate_contract(inputs, monkeypatch):
    base = inputs[0]
    page = base.with_suffix(".md")
    render_cli(base, page, monkeypatch)
    assert "original scrubadub measurements predate the Spanish adapter correction" in page.read_text()


@pytest.mark.parametrize("alias", ("direct", "symlink", "hardlink"))
@pytest.mark.parametrize("target", ("base", "supplement", "report", "presidio-tuned.json",
    "presidio-tuned-selection.json", "presidio-tuned-overlap.json", "release-history.json", "release-char-level.json"))
def test_render_cli_preserves_input_evidence(inputs, monkeypatch, alias, target):
    compose_cli(inputs, monkeypatch)
    output = inputs[-1]
    source = {"base": inputs[0], "supplement": inputs[1], "report": output}.get(target)
    if source is None:
        source = output.with_name(target)
        source.write_bytes(b"previous evidence\n")
    before = {path: path.read_bytes() for path in (*inputs[:2], output, source)}
    page = output.with_suffix(".md")
    if alias == "direct":
        page = source
    elif alias == "symlink":
        page.symlink_to(source)
    else:
        try:
            page.hardlink_to(source)
        except OSError as error:
            pytest.skip(f"filesystem does not support hardlinks: {error}")
    with pytest.raises(ValueError, match="overwritten"):
        render_cli(output, page, monkeypatch)
    assert {path: path.read_bytes() for path in before} == before
    if alias == "symlink":
        assert page.is_symlink()
    if alias == "hardlink":
        assert page.samefile(source)


def test_render_cli_repeats_identical_output(inputs, monkeypatch):
    compose_cli(inputs, monkeypatch)
    output = inputs[-1]
    page = output.with_suffix(".md")
    render_cli(output, page, monkeypatch)
    before = page.read_bytes()
    render_cli(output, page, monkeypatch)
    assert page.read_bytes() == before
    monkeypatch.setattr(sys, "argv", ["render.py", str(output), "--page", str(page), "--check"])
    render.main()


@pytest.mark.parametrize("source", (0, 1))
def test_changed_snapshot_bytes_or_expected_hash_preserve_prior_output(inputs, monkeypatch, source):
    output = inputs[-1]
    output.write_bytes(b"previous publication\n")
    expected = row_sources.file_hash(inputs[source])
    inputs[source].write_bytes(inputs[source].read_bytes() + b"\n")
    with pytest.raises(ValueError, match="snapshot hash"):
        compose_cli(inputs, monkeypatch, **{("base_sha" if source == 0 else "supplement_sha"): expected})
    assert output.read_bytes() == b"previous publication\n"


@pytest.mark.parametrize("key", (*row_sources.COMPATIBLE, "compare_sha256"))
def test_incompatible_input_preserves_prior_output(inputs, monkeypatch, key):
    supplement = json.loads(inputs[1].read_bytes())
    supplement[key] = "different input"
    write(inputs[1], supplement)
    inputs[-1].write_bytes(b"previous publication\n")
    with pytest.raises(ValueError):
        compose_cli(inputs, monkeypatch)
    assert inputs[-1].read_bytes() == b"previous publication\n"


@pytest.mark.parametrize("change", ("old-row", "new-row", "native-hash", "snapshot", "snapshot-hash", "attribution", "opf", "aggregation"))
def test_renderer_rejects_corruption_without_replacing_page(inputs, monkeypatch, change):
    result = compose_cli(inputs, monkeypatch)
    output = inputs[-1]
    if change in ("old-row", "new-row"):
        name = "gliner" if change == "old-row" else "scrubadub-base"
        result["tools"][name]["contracts"]["v4"]["C"]["leaked_bytes"] += 1
    elif change == "native-hash":
        result["row_sources"]["base"]["native_records"]["gliner"]["C"]["sha256"] = "0" * 64
    elif change == "snapshot":
        inputs[0].write_bytes(inputs[0].read_bytes() + b"\n")
    elif change == "snapshot-hash":
        result["row_sources"]["supplement"]["sha256"] = "0" * 64
    elif change == "attribution":
        result["row_sources"]["tool_sources"]["gliner"] = "supplement"
    elif change == "opf":
        result["unavailable"]["opf"]["attempts"].pop()
    else:
        result["row_sources"]["aggregation"]["compare_sha256"] = "0" * 64
    write(output, result)
    page = output.with_suffix(".md")
    page.write_bytes(b"previous page\n")
    with pytest.raises(ValueError):
        render_cli(output, page, monkeypatch)
    assert page.read_bytes() == b"previous page\n"


@pytest.mark.parametrize("change", ("model", "skip", "population", "source", "extra-tool", "replay"))
def test_non_spanish_changes_cannot_enter_the_supplement(inputs, monkeypatch, change):
    supplement = json.loads(inputs[1].read_bytes())
    tool = supplement["tools"]["scrubadub-base"]
    if change == "model":
        tool["provenance"]["spacy_model"]["sha256"] = "0" * 64
    elif change == "skip":
        tool["contracts"]["v4"]["A"]["skipped_languages"] = {"es": 1}
    elif change == "population":
        tool["contracts"]["v4"]["A"]["documents"] = 0
    elif change == "source":
        tool["provenance"]["measurement_compare_sha256"] = row_sources.ORIGINAL_ADAPTER
    elif change == "extra-tool":
        supplement["tools"]["gliner"] = tool
    else:
        supplement["rescore_source_revision"] = "a" * 40
    write(inputs[1], supplement)
    with pytest.raises(ValueError):
        compose_cli(inputs, monkeypatch)
    assert not inputs[-1].exists()


def test_identically_stale_scorer_in_both_snapshots_still_fails_current_validation(inputs, monkeypatch):
    for path in inputs[:2]:
        source = json.loads(path.read_bytes())
        source["scorer_sha256"] = "0" * 64
        write(path, source)
    inputs[-1].write_bytes(b"previous publication\n")
    with pytest.raises(ValueError, match="scorer changed"):
        compose_cli(inputs, monkeypatch)
    assert inputs[-1].read_bytes() == b"previous publication\n"


def test_tuned_original_binding_keeps_byte_verdict_and_rejects_byte_changes(inputs, monkeypatch):
    compose_cli(inputs, monkeypatch)
    output = inputs[-1]
    selection = output.with_name("presidio-tuned-selection.json")
    selection.write_text('{}\n')
    label_difference = {"live_only": [[0, 5, "EMAIL"]], "replay_only": [[0, 5, "PERSON"]]}
    tuned = {
        "comparison_sha256": row_sources.file_hash(inputs[0]), "compare_sha256": row_sources.ORIGINAL_ADAPTER,
        "comparison_metrics_sha256": row_sources.file_hash(compare.MAP_PATH.with_name("comparison_metrics.py")),
        "scorer_sha256": row_sources.file_hash(render.BENCH / "gaze_bench_score.py"),
        "selection_sha256": row_sources.file_hash(selection),
        "code_sha256": {name: row_sources.file_hash(compare.MAP_PATH.parent / "tuned_presidio" / name)
                        for name in render.TUNED_CODE},
        "harness_dirty": False, "anchor": {"equal": True},
        "provenance": {"fictional-tuned": {"live_verification": {
            "persistent": ["fictional"], "differences": {"fictional": label_difference}}}},
    }
    original = copy.deepcopy(tuned)
    render.validate_tuned(tuned, output)
    assert tuned == original
    assert not (label_difference["live_only"] == label_difference["replay_only"])
    label_difference["live_only"][0][1] = 4
    with pytest.raises(ValueError, match="different bytes"):
        render.validate_tuned(tuned, output)
    label_difference["live_only"][0][1] = 5
    tuned["comparison_sha256"] = row_sources.file_hash(output)
    with pytest.raises(ValueError, match="comparison report changed"):
        render.validate_tuned(tuned, output)


@pytest.mark.parametrize("source", (0, 1))
def test_output_cannot_overwrite_a_source_snapshot(inputs, monkeypatch, source):
    before = inputs[source].read_bytes()
    redirected = (*inputs[:-1], inputs[source])
    with pytest.raises(ValueError, match="overwritten"):
        compose_cli(redirected, monkeypatch)
    assert inputs[source].read_bytes() == before


def test_composition_requires_the_original_opf_unavailable_receipt(inputs, monkeypatch):
    base = json.loads(inputs[0].read_bytes())
    del base["unavailable"]
    base["tools"]["opf"] = copy.deepcopy(base["tools"]["gliner"])
    base["tools"]["opf"]["tool"] = "opf"
    base["contention_samples"]["opf"] = {"contended": False}
    write(inputs[0], base)
    with pytest.raises(ValueError, match="original OPF unavailable receipt"):
        compose_cli(inputs, monkeypatch)
    assert not inputs[-1].exists()
