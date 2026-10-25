"""Synthetic, complete capture/replay and independent invalidation controls."""
import copy
import dataclasses
import json
import sys
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


def test_driver_fresh_hit_and_private_admission_execute_no_replayed_subject(example, monkeypatch):
    from types import SimpleNamespace
    root, spec, _, docs = example
    layers = {"C": docs, "A": [], "D": [], "R": []}
    for layer, documents in layers.items():
        spec["inputs"]["population"][layer] = benchmark_spec.population(documents, layer)
    spec = benchmark_spec.manifest(spec["inputs"])
    spec_path = root / "spec.json"
    spec_path.write_text(json.dumps(spec))
    monkeypatch.setattr(benchmark_spec, "require_current", lambda *args, **kwargs: None)
    monkeypatch.setattr(compare, "load_corpus", lambda *args: (layers, {"synthetic": True}))
    monkeypatch.setattr(compare, "preflight_contracts", lambda *args: None)
    monkeypatch.setattr(identity, "runtime", lambda: {"python": "synthetic"})
    calls = []
    class Backend:
        def predict(self, doc):
            calls.append(doc.uid)
            return [compare.score.Span(0, len(doc.text), "EMAIL")]
    monkeypatch.setattr(capture_compare, "backend", lambda *args: (Backend(), None))
    args = SimpleNamespace(tool="datafog-core", benchmark_spec=spec_path, dataset=root / "dataset",
        pack_dir=None, cache_dir=root / "cache", predictions_dir=None, freshness_report=None,
        verify_report=None, output=root / "report.json")
    first = capture_compare.run(args)
    original = args.output.read_bytes()
    assert calls == ["warmup", *[doc.uid for doc in docs]]
    monkeypatch.setattr(capture_compare, "backend", lambda *args: pytest.fail("replay constructed a backend"))
    assert capture_compare.run(args) == first
    assert args.output.read_bytes() == original
    args.verify_report = args.output
    assert capture_compare.run(args) == first
    damaged = copy.deepcopy(first)
    damaged["tools"]["datafog-core"]["contracts"]["v1"]["C"]["leaked_bytes"] += 1
    args.output.write_text(json.dumps(damaged))
    with pytest.raises(ValueError, match="submitted report"):
        capture_compare.run(args)
    args.verify_report = None
    args.freshness_report = root / "freshness.json"
    args.freshness_report.write_text(json.dumps({"rows": [], "checked_on": "alice@example.invalid"}))
    previous = args.output.read_bytes()
    with pytest.raises(ValueError, match="unsupported freshness inventory"):
        capture_compare.run(args)
    assert args.output.read_bytes() == previous


def test_spec_endpoint_drift_and_population_are_refused(example, monkeypatch):
    path = example[0] / "spec.json"
    path.write_text(json.dumps(example[1]))
    monkeypatch.setattr(benchmark_spec, "require_current", lambda *args, **kwargs: None)
    read = identity.freeze_spec(path, example[0], path, lambda: example[2])
    assert read() == example[2]
    path.write_text("{}")
    with pytest.raises(ValueError, match="specification changed"):
        read()
    with pytest.raises(ValueError, match="population differs"):
        identity.require_population(example[1], {"C": example[-1]})


def test_read_only_admission_refuses_missing_capture_and_original_row(example):
    current = captures.Session(example[0], example[1], lambda: example[2], {}, read_only=True)
    with pytest.raises(ValueError, match="complete capture"):
        current.capture(example[-1][0], predictor)
    with pytest.raises(ValueError, match="original aggregate"):
        current.retain_row({}, {})


@pytest.mark.parametrize("field", ["name", "config", "models", "runtime", "adapter"])
def test_actual_subject_inputs_change_document_identity(example, field):
    root, spec, _, docs = example
    model = root / "model"
    model.mkdir()
    (model / "weights").write_bytes(b"synthetic")
    adapter = root / "adapter.py"
    adapter.write_text("synthetic")
    args = dict(name="synthetic", config={"threshold": .3}, models={"model": model},
        runtime_identity={"python": "synthetic"}, adapter=adapter)
    before = identity.document_key(spec, identity.subject(**args), docs[0])
    if field == "models":
        (model / "weights").write_bytes(b"changed")
    elif field == "adapter":
        adapter.write_text("changed")
    else:
        args["runtime_identity" if field == "runtime" else field] = {"changed": True} if field != "name" else "changed"
    assert identity.document_key(spec, identity.subject(**args), docs[0]) != before


def test_document_key_contract_has_complete_typed_inputs(example):
    _, spec, subject, docs = example
    key = identity.document_key(spec, subject, docs[0])
    assert key == {"schema_version": 1, "benchmark_spec_id": spec["id"], "subject": subject,
        "input": {"text_sha256": digest(docs[0].text), "utf8_bytes": len(docs[0].text.encode()),
                  "uid": docs[0].uid, "language": docs[0].language, "region": docs[0].region}}


@pytest.mark.parametrize("value", [-1, True, "invalid", float("inf")])
def test_rebound_invalid_timing_is_refused(example, value):
    current, _ = capture_all(example)
    path = example[0] / (digest(current.keys[0]) + ".json")
    entry = json.loads(path.read_text())
    entry["observation"]["elapsed_ms"] = value
    if value == float("inf"):
        # Non-finite JSON cannot be rebound under the canonical digest contract.
        with pytest.raises(ValueError):
            entry["sha256"] = digest({k: v for k, v in entry.items() if k != "sha256"})
        return
    entry["sha256"] = digest({k: v for k, v in entry.items() if k != "sha256"})
    path.write_text(json.dumps(entry))
    with pytest.raises(ValueError, match="invalid capture observation"):
        session(example).capture(example[-1][0], predictor)


def test_private_aggregate_tampering_is_refused(example):
    current, _ = capture_all(example)
    row = {"tool": "synthetic", "contracts": {"v1": {}}, "provenance": {},
           "capture_binding": identity.public_binding(current.observations)}
    current.retain_row(row, {})
    path = example[0] / ("row-" + digest(current.keys) + ".json")
    changed = json.loads(path.read_text())
    changed["row"]["contracts"] = {}
    path.write_text(json.dumps(changed))
    with pytest.raises(ValueError, match="aggregate replay mismatch"):
        current.retain_row(row, {})


def test_private_roots_and_model_link_boundaries(tmp_path):
    repo = tmp_path / "repository"
    repo.mkdir()
    with pytest.raises(ValueError, match="outside"):
        captures.private_root(repo / "cache", repo)
    with pytest.raises(ValueError, match="missing selected"):
        identity.model_digest(tmp_path / "missing")
    model = tmp_path / "model"
    model.mkdir()
    with pytest.raises(ValueError, match="empty selected"):
        identity.model_digest(model)
    (model / "linked-directory").symlink_to(repo)
    with pytest.raises(ValueError, match="directory link"):
        identity.model_digest(model)


def test_gliner_subject_requires_independent_runtime_plan(example):
    from types import SimpleNamespace
    root = example[0]
    adapter = root / "adapter.py"
    adapter.write_text("synthetic")
    args = dict(name="gliner", config={}, models={}, runtime_identity={"python": "actual"}, adapter=adapter)
    with pytest.raises(ValueError, match="missing independent"):
        identity.subject(**args)
    plan = SimpleNamespace(recheck=lambda: None, descriptor={"runtime": {"python": "wrong"}})
    with pytest.raises(ValueError, match="runtime differs"):
        identity.subject(**args, gliner_plan=plan)
    args["name"] = "synthetic"
    with pytest.raises(ValueError, match="unexpected GLiNER plan"):
        identity.subject(**args, gliner_plan=plan)
    with pytest.raises(ValueError, match="unexpected GLiNER computation"):
        identity.verify_gliner({"name": "synthetic"}, {"forged": True}, None)


@pytest.mark.parametrize("damage", ["inventory", "missing", "editable", "bytes", "hash_seed"])
def test_runtime_uses_full_installed_content_inventory(example, monkeypatch, damage):
    from types import SimpleNamespace
    root = example[0]
    package = root / "synthetic.py"
    package.write_text("synthetic bytes")
    distribution = SimpleNamespace(files=[Path("synthetic.py")], version="1", metadata={"Name": "synthetic"},
        locate_file=lambda file: root / file, read_text=lambda file: None)
    monkeypatch.setattr(identity.importlib.metadata, "distributions", lambda: [distribution])
    monkeypatch.setattr(identity.content, "native_runtime", lambda args: {"native": "synthetic"})
    monkeypatch.setattr(identity.runtime_closure, "import_inventory", lambda *args: "synthetic-imports")
    monkeypatch.setenv("PYTHONHASHSEED", "0")
    before = identity.runtime()
    assert set(before) == {"interpreter_sha256", "packages_sha256", "import_files", "imports_sha256", "native", "sys_path",
                           "environment_sha256", "python_optimization"}
    if damage == "inventory":
        distribution.files = None
    elif damage == "missing":
        package.unlink()
    elif damage == "editable":
        distribution.read_text = lambda file: '{"dir_info":{"editable":true}}'
    elif damage == "hash_seed":
        monkeypatch.setenv("PYTHONHASHSEED", "random")
    else:
        package.write_text("changed")
        assert identity.runtime() != before
        return
    with pytest.raises(ValueError):
        identity.runtime()


def test_subject_schema_is_complete(example):
    adapter = example[0] / "adapter.py"
    adapter.write_text("synthetic")
    subject = identity.subject(name="synthetic", config={}, models={}, runtime_identity={}, adapter=adapter)
    assert set(subject) == {"schema_version", "name", "config", "models", "runtime", "adapter_sha256"}


def test_capture_launch_preserves_external_slots_and_initializes_once(monkeypatch):
    import capture_launch
    monkeypatch.setattr(capture_launch, "_initialized", False)
    external = ["synthetic-external", "synthetic-external", ""]
    monkeypatch.setattr(sys, "path", list(external))
    capture_launch.initialize()
    first = list(sys.path)
    assert first[-len(external):] == external
    capture_launch.initialize()
    assert sys.path == first


def test_public_projection_and_prediction_paths_hide_private_pack_names(example):
    root, _, _, docs = example
    private_layer = "../private-pack-name/sealed"
    layers = {private_layer: docs}
    current = session(example)
    row = capture_compare.measure(current, "synthetic", predictor, layers, {"email": ("EMAIL",)}, None,
                                   frozenset({"EMAIL"}), root / "predictions")
    public, corpus = capture_compare.public_projection(row, {"packs": [{"path": "/private/raw-pack.jsonl"}]}, layers)
    encoded = json.dumps({"row": public, "corpus": corpus})
    assert "private-pack-name" not in encoded and "raw-pack" not in encoded and "/private" not in encoded
    assert (root / "predictions/extra-000/pred/synthetic.jsonl").is_file()
    assert not (root / "private-pack-name").exists()


def test_rehashed_wrong_observation_cannot_hide_behind_identical_totals(example):
    root, _, _, docs = example
    layers = {"C": docs}
    predict = lambda doc: [compare.score.Span(0, 3, "email")]
    first = session(example)
    row = capture_compare.measure(first, "synthetic", predict, layers, {"email": ("EMAIL",)}, None, frozenset({"EMAIL"}))
    row["provenance"] = {}
    first.retain_row(row, {})
    path = root / (digest(first.keys[0]) + ".json")
    changed = json.loads(path.read_text())
    changed["observation"]["predictions"] = [{"start": 5, "end": 8, "label": "email"}]
    changed["sha256"] = digest({key: value for key, value in changed.items() if key != "sha256"})
    path.write_text(json.dumps(changed))
    second = session(example)
    replayed = capture_compare.measure(second, "synthetic", predict, layers, {"email": ("EMAIL",)}, None, frozenset({"EMAIL"}))
    replayed["provenance"] = {}
    assert replayed["contracts"] == row["contracts"]
    with pytest.raises(ValueError, match="aggregate replay mismatch"):
        second.retain_row(replayed, {})
