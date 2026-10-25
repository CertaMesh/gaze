"""Independent negative controls for selected tuned computations and private replay."""
import copy
import dataclasses
import json
import sys
from pathlib import Path
from types import SimpleNamespace

import pytest

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE))
import corpus
import capture_produce
import capture_tune
import input_binding
from capture_identity import digest


@pytest.fixture
def prepared(tmp_path):
    model = tmp_path / "private-model"
    model.mkdir()
    (model / "weights").write_bytes(b"synthetic weights")
    source = tmp_path / "private-findings.jsonl"
    source.write_text("synthetic source")
    doc = corpus.compare.score.Document("private-synthetic-id", "alice@example.invalid", "en", "", "synthetic", ())
    @dataclasses.dataclass
    class Replay:
        uid: str
        document: object
        results: tuple
    replay = SimpleNamespace(docs=[Replay(doc.uid, doc, ((0, 2, "EMAIL", "synthetic", (0.1, .2, .3)),))],
        mapping={"EMAIL": ("EMAIL",)}, typed_mapping={"EMAIL": ("EMAIL",)})
    selection = {"choices": {"f2": {"start": "synthetic", "finals": {"synthetic": {"config": {
        "thresholds": {"EMAIL": .5}, "allow_list": ["alice@example.invalid"]}}}}}}
    paths = SimpleNamespace(spacy={"en": model}, dslim=None, davlan=None, openmed=None, gliner=None)
    calls = []
    def live(config, actual_paths, actual_replay):
        calls.append((copy.deepcopy(config), actual_paths, actual_replay))
        return {"documents": 1, "differences": {}}
    inputs = input_binding.Inputs(selection=selection, replayer=replay, paths=paths,
        files=[source], sample=lambda uid: True, live_verify=live, rows={"f2": "selected"})
    report = {"rows": {"selected": {"leaked_bytes": 0}}, "provenance": {"selected": {
        "config": copy.deepcopy(inputs.config("f2")), "live_verification": inputs.verify_live("f2"),
        "live_inputs": inputs.public()}}}
    return inputs, report, tmp_path / "private.json", calls


def test_sealed_private_capture_replays_exact_selected_computation(prepared):
    inputs, report, path, calls = prepared
    inputs.seal(report, path)
    inputs.admit(report, path, lambda objective: {"leaked_bytes": 0})
    assert len(calls) == 1
    assert calls[0][0] == inputs.config("f2")
    assert calls[0][1] is inputs.paths and calls[0][2] is inputs.replayer
    assert path.stat().st_mode & 0o777 == 0o600
    assert set(inputs.original) == {"schema_version", "selection", "configs", "models", "replay", "mapping",
                                   "typed_mapping", "gliner", "runtime", "sample", "files"}


@pytest.mark.parametrize("change", ["config", "text", "gold", "findings", "mapping", "typed_mapping", "models", "files", "sample"])
def test_every_selected_live_input_drift_refuses(prepared, change):
    inputs, report, path, _ = prepared
    if change == "config":
        inputs.config("f2")["thresholds"]["EMAIL"] = .9
    elif change in {"text", "gold"}:
        doc = inputs.replayer.docs[0]
        doc.document = dataclasses.replace(doc.document, **({"text": "changed@example.invalid"} if change == "text" else
            {"spans": (corpus.compare.score.Span(0, 2, "EMAIL"),)}))
    elif change == "findings":
        inputs.replayer.docs[0].results = ()
    elif change in {"mapping", "typed_mapping"}:
        getattr(inputs.replayer, change)["EMAIL"] = ()
    elif change == "models":
        (inputs.paths.spacy["en"] / "weights").write_bytes(b"changed")
    elif change == "files":
        inputs.files[0].write_text("changed")
    else:
        inputs.sample = lambda uid: False
    with pytest.raises(ValueError, match="selected live inputs changed"):
        inputs.seal(report, path)
    assert not path.exists()


@pytest.mark.parametrize("field", ["config", "live_verification", "live_inputs"])
def test_report_cannot_claim_another_live_computation(prepared, field):
    inputs, report, path, _ = prepared
    report["provenance"]["selected"][field] = {}
    with pytest.raises(ValueError, match="original live computation"):
        inputs.seal(report, path)


def test_private_replay_rejects_wrong_computation_and_input_rebinding(prepared):
    inputs, report, path, _ = prepared
    inputs.seal(report, path)
    with pytest.raises(ValueError, match="submitted computation"):
        inputs.admit(report, path, lambda objective: {"leaked_bytes": 1})
    private = json.loads(path.read_text())
    private["binding"]["configs"] = {}
    private["sha256"] = digest({k: v for k, v in private.items() if k != "sha256"})
    path.write_text(json.dumps(private))
    with pytest.raises(ValueError, match="input binding mismatch"):
        inputs.admit(report, path, lambda objective: {"leaked_bytes": 0})


def test_tuned_public_projection_contains_digest_and_counts_only(prepared):
    inputs, report, path, _ = prepared
    public = capture_tune.public_report(report)
    encoded = json.dumps(public)
    assert "example.invalid" not in encoded and "private-synthetic-id" not in encoded
    assert "private-model" not in encoded and "private-findings" not in encoded
    provenance = public["provenance"]["selected"]
    assert set(provenance["live_inputs"]) == {"schema_version", "binding_sha256", "sample_documents"}
    assert set(provenance["config_binding"]) == {"sha256", "allow_list_entries"}


@pytest.mark.parametrize("row", [[True, 2, "EMAIL", .1], [0, 9999, "EMAIL", .1],
    [0, 2, "EMAIL", True], [0, 2, "EMAIL", "unit", .1, .2, "bad"]])
def test_tuned_raw_prediction_admission_is_typed_and_bounded(prepared, row):
    doc = prepared[0].replayer.docs[0].document
    with pytest.raises(ValueError, match="invalid tuned raw finding"):
        capture_produce.validate_raw([row], doc, {})


def test_repeated_live_objective_is_refused(prepared):
    with pytest.raises(ValueError, match="repeated"):
        prepared[0].verify_live("f2")


def test_public_report_cannot_replace_sealed_private_report(prepared):
    inputs, report, path, _ = prepared
    inputs.seal(report, path)
    submitted = copy.deepcopy(report)
    submitted["rows"]["selected"]["leaked_bytes"] = 1
    with pytest.raises(ValueError, match="input binding mismatch"):
        inputs.admit(submitted, path, lambda objective: {"leaked_bytes": 1})


def test_raw_production_fresh_replay_and_private_file_admission(tmp_path, monkeypatch):
    from document_capture import Session
    import benchmark_spec
    from capture_identity import file_digest
    doc = corpus.compare.score.Document("synthetic", "alice@example.invalid", "en", "", "synthetic", ())
    layers = {"C": [doc]}
    spec = benchmark_spec.manifest({"corpus_sha256": "a" * 64, "generator": None, "seed": 1,
        "contracts": {"v1": "all"}, "harness": {"driver": "b" * 64}, "record_schema": [1],
        "protocol": {}, "population": {"C": {"documents": 1}}})
    subject = {"name": "tuned-presidio-base", "config": {"record": "tuned-raw-findings-v1"}}
    args = SimpleNamespace(mode="pool", pass_name="base", out=tmp_path / "raw", dataset=tmp_path / "dataset")
    args.out.mkdir()
    monkeypatch.setattr(capture_produce, "parse_args", lambda: args)
    monkeypatch.setattr(capture_produce.corpus, "load_measured", lambda *args: layers)
    monkeypatch.setattr(capture_produce.corpus, "read_comparison", lambda: {})
    monkeypatch.setattr(capture_produce.corpus, "LAYERS", ("C",))
    monkeypatch.setattr(capture_produce, "prepare", lambda *args: Session(tmp_path / "cache", spec, lambda: subject, {}))
    calls = []
    def record(args, layers, session):
        def predict(document):
            calls.append(document.uid)
            return [[0, 2, "EMAIL", "synthetic-unit", .1, .2, .3]]
        value, _ = session.capture_payload(doc, predict, capture_produce.validate_raw)
        sinks = capture_produce.split_sinks(args.out, "base")
        sinks[corpus.split_for_id(doc.uid)].write(json.dumps({"uid": doc.uid, "layer": "C", "r": value}, ensure_ascii=False) + "\n")
        binding = session.finish()
        capture_produce.finish(args.out, "base", sinks, {"capture_binding": binding, "seconds": len(calls),
            "harness_revision": "a" * 40, "harness_dirty": False})
    monkeypatch.setattr(capture_produce, "pool_pass", record)
    capture_produce.main()
    original = {path.name: path.read_bytes() for path in args.out.iterdir()}
    capture_produce.main()
    assert calls == [doc.uid]
    assert original == {path.name: path.read_bytes() for path in args.out.iterdir()}
    assert capture_produce.verify_output(args, layers) == subject
    path = args.out / "base.meta.json"
    metadata = json.loads(path.read_text())
    metadata["subject_sha256"] = "wrong"
    path.write_text(json.dumps(metadata))
    with pytest.raises(ValueError, match="metadata differs"):
        capture_produce.verify_output(args, layers)
    path.write_bytes(original[path.name])
    split = corpus.split_for_id(doc.uid)
    raw = args.out / f"base.{split}.jsonl"
    changed = json.loads(raw.read_text())
    changed["r"][0][0] = True
    raw.write_text(json.dumps(changed) + "\n")
    metadata = json.loads(path.read_text())
    metadata["sha256"][split] = file_digest(raw)
    path.write_text(json.dumps(metadata))
    with pytest.raises(ValueError, match="raw findings differ"):
        capture_produce.verify_output(args, layers)


def test_tuned_entrypoints_share_import_context_and_do_not_import_models():
    import subprocess
    script = ('import sys,json,importlib;sys.path.insert(0,sys.argv[1]);'
              'importlib.import_module(sys.argv[2]);import compare,corpus;'
              'print(json.dumps({"paths":sys.path,"same_span":compare.score.Span is corpus.compare.score.Span,'
              '"model_imported":any(name in sys.modules for name in ("torch","gliner","transformers"))}))')
    results = [json.loads(subprocess.check_output([sys.executable, "-c", script, str(HERE), module], text=True))
               for module in ("capture_produce", "capture_tune")]
    assert results[0] == results[1]
    assert results[0]["same_span"] and not results[0]["model_imported"]


def test_gliner_live_selector_cannot_differ_from_planned_request(prepared):
    inputs = prepared[0]
    inputs.gliner_plan = SimpleNamespace(recheck=lambda: None, descriptor={"request": {"wrong": True}})
    with pytest.raises(ValueError, match="live model selector differs"):
        inputs.binding()
