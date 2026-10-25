"""Synthetic, complete capture/replay and independent invalidation controls."""
import copy
import dataclasses
import json
from pathlib import Path
from unittest.mock import patch

import pytest
import compare
import capture_compare
import document_capture as captures
import subject_identity as identity
import benchmark_spec
from capture_identity import digest


@pytest.fixture
def example(tmp_path):
    spec = benchmark_spec.manifest({"corpus_sha256": "a" * 64, "generator": None, "seed": 1,
        "contracts": {"v1": "all"}, "harness": {"driver": "b" * 64}, "record_schema": [1],
        "protocol": {"mode": "synthetic"}, "population": {"C": {"documents": 2}}})
    subject = {"name": "synthetic", "config": {"threshold": .5}, "models": {"model": "a" * 64},
               "runtime": {"python": "synthetic"}, "adapter_sha256": "b" * 64}
    docs = [compare.score.Document(f"synthetic-{i}", text, "en", "", "synthetic", ())
            for i, text in enumerate(("alice@example.invalid", "bob@example.invalid"))]
    return tmp_path, spec, subject, docs


def session(example):
    root, spec, subject, _ = example
    return captures.Session(root, spec, lambda: copy.deepcopy(subject), {"email": ("EMAIL",)})


def predictor(document):
    return [compare.score.Span(0, len(document.text.encode()), "email")]


def capture_all(example):
    current = session(example)
    result = [current.capture(document, predictor) for document in example[-1]]
    current.finish()
    return current, result


def test_complete_fresh_and_reused_report_are_byte_identical(example):
    _, _, _, docs = example
    layers = {"C": docs}
    first = session(example)
    with patch("document_capture.time.perf_counter", side_effect=[1, 1.003, 2, 2.007, 3, 4, 5, 6]):
        row = capture_compare.measure(first, "synthetic", predictor, layers, {"email": ("EMAIL",)}, None, frozenset({"EMAIL"}))
    row["provenance"] = {"subject": digest(first.subject)}
    row, contention = first.retain_row(row, {"contended": False})
    fresh = {"tools": {"synthetic": row}, "contention_samples": contention}
    second = session(example)
    def no_inference(document):
        pytest.fail("replay executed the subject")
    reused = capture_compare.measure(second, "synthetic", no_inference, layers, {"email": ("EMAIL",)}, None, frozenset({"EMAIL"}))
    reused["provenance"] = {"subject": digest(second.subject)}
    reused, contention = second.retain_row(reused, {"contended": True})
    assert json.dumps(fresh, sort_keys=True) == json.dumps({"tools": {"synthetic": reused}, "contention_samples": contention}, sort_keys=True)
    assert (first.misses, second.hits, second.misses) == (2, 2, 0)


def test_one_document_recomputes_exactly_one_unit(example):
    capture_all(example)
    example[-1][1] = dataclasses.replace(example[-1][1], text="changed@example.invalid")
    current, _ = capture_all(example)
    assert (current.hits, current.misses) == (1, 1)


@pytest.mark.parametrize("field", ["text", "language", "region", "uid"])
def test_each_document_input_invalidates_its_unit(example, field):
    capture_all(example)
    example[-1][1] = dataclasses.replace(example[-1][1], **{field: "changed"})
    current, _ = capture_all(example)
    assert (current.hits, current.misses) == (1, 1)


@pytest.mark.parametrize("field", ["name", "config", "models", "runtime", "adapter_sha256"])
def test_each_subject_input_invalidates_all_units(example, field):
    capture_all(example)
    example[2][field] = "changed"
    current, _ = capture_all(example)
    assert (current.hits, current.misses) == (0, 2)


def test_spec_change_invalidates_all_units(example):
    capture_all(example)
    example[1]["inputs"]["seed"] += 1
    changed = benchmark_spec.manifest(example[1]["inputs"])
    current = captures.Session(example[0], changed, lambda: example[2], {"email": ("EMAIL",)})
    for document in example[-1]:
        current.capture(document, predictor)
    assert (current.hits, current.misses) == (0, 2)


def test_gaze_only_changes_leave_competitor_key_invariant(example, tmp_path):
    adapter = tmp_path / "adapter.py"
    adapter.write_text("pass")
    model = tmp_path / "model"
    model.mkdir()
    (model / "weights").write_bytes(b"synthetic weights")
    read = lambda: identity.subject(name="synthetic", config={}, models={"model": model},
        runtime_identity={"python": "synthetic"}, adapter=adapter)
    before = identity.document_key(example[1], read(), example[-1][0])
    (tmp_path / "gaze.rs").write_text("changed current Gaze")
    assert identity.document_key(example[1], read(), example[-1][0]) == before


@pytest.mark.parametrize("field", ["key", "predictions", "elapsed_ms", "gliner_receipt", "sha256"])
def test_tampered_entry_is_refused(example, field):
    current, _ = capture_all(example)
    path = example[0] / (digest(current.keys[0]) + ".json")
    entry = json.loads(path.read_text())
    if field in {"predictions", "elapsed_ms"}:
        entry["observation"][field] = "tampered"
    else:
        entry[field] = "tampered"
    path.write_text(json.dumps(entry))
    with pytest.raises(ValueError):
        session(example).capture(example[-1][0], predictor)


def test_rebound_invalid_prediction_and_timing_are_refused(example):
    current, _ = capture_all(example)
    path = example[0] / (digest(current.keys[0]) + ".json")
    original = json.loads(path.read_text())
    for bad in ({"start": True, "end": 2, "label": "email"},
                {"start": 0, "end": 999, "label": "email"},
                {"start": 0, "end": 2, "label": "foreign"}):
        entry = copy.deepcopy(original)
        entry["observation"]["predictions"] = [bad]
        entry["sha256"] = digest({k: v for k, v in entry.items() if k != "sha256"})
        path.write_text(json.dumps(entry))
        with pytest.raises(ValueError, match="invalid capture prediction"):
            session(example).capture(example[-1][0], predictor)


def test_endpoint_drift_publishes_nothing(example):
    current = session(example)
    current.capture(example[-1][0], predictor)
    example[2]["config"]["threshold"] = .9
    with pytest.raises(ValueError, match="inputs changed"):
        current.finish()
    assert not list(example[0].glob("*.json"))


def test_public_projection_contains_only_digest_and_count(example):
    current, _ = capture_all(example)
    binding = identity.public_binding(current.keys)
    assert set(binding) == {"schema_version", "binding_sha256", "documents"}
    assert "example.invalid" not in json.dumps(binding)
    assert example[-1][0].uid not in json.dumps(binding)


def test_named_gliner_cannot_remove_private_plan_or_receipt(example):
    example[2]["name"] = "gliner"
    with pytest.raises(ValueError, match="independent GLiNER"):
        session(example).capture(example[-1][0], predictor)
