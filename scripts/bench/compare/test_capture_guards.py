"""Boundary controls retained alongside the full capture/replay scenarios."""
import copy
import importlib.machinery
import importlib.util
import json
import marshal
import os
from pathlib import Path
import struct
import sys
from types import SimpleNamespace

import pytest
import document_capture
import gliner_identity as identity
import test_gliner_identity as legacy
import capture_compare
import subject_identity
from capture_identity import digest, file_digest


@pytest.fixture
def synthetic():
    fixture = legacy.IdentityContract()
    fixture.setUp()
    try:
        yield fixture
    finally:
        fixture.doCleanups()


def test_prediction_container_must_be_a_list():
    with pytest.raises(ValueError, match="invalid capture predictions"):
        document_capture.validate_predictions({}, SimpleNamespace(text="synthetic"), {})


def test_parent_traversal_and_missing_or_unresolved_home_are_refused(tmp_path):
    with pytest.raises(identity.Uncacheable, match="parent traversal"):
        identity._absolute("../synthetic", tmp_path)
    with pytest.raises(identity.Uncacheable, match="missing HOME"):
        identity._roots({"HOME": ""}, tmp_path)
    with pytest.raises(identity.Uncacheable, match="expansion"):
        identity._roots({"HOME": str(tmp_path), "HF_HOME": "~synthetic-user"}, tmp_path)


@pytest.mark.parametrize("selected_request", [identity.Request("unknown", "synthetic/model", {}),
    identity.Request("datafog", "synthetic/other", {"local_files_only": True})])
def test_unsupported_requests_are_refused_before_resolution(selected_request):
    with pytest.raises(identity.Uncacheable):
        selected_request.descriptor()


@pytest.mark.parametrize("damage", ["magic", "flags", "source_hash"])
def test_bytecode_headers_are_independently_bound(tmp_path, damage):
    source = tmp_path / "synthetic.py"
    source.write_text("synthetic = 1\n")
    raw = source.read_bytes()
    magic = importlib.util.MAGIC_NUMBER if damage != "magic" else b"BAD!"
    flags = 4 if damage == "flags" else 3 if damage == "source_hash" else 0
    stamp = (b"0" * 8 if damage == "source_hash" else
             struct.pack("<II", int(source.stat().st_mtime) & 0xffffffff, len(raw)))
    cached = Path(importlib.util.cache_from_source(str(source)))
    cached.parent.mkdir()
    cached.write_bytes(magic + struct.pack("<I", flags) + stamp + marshal.dumps(compile(raw, str(source), "exec", dont_inherit=True)))
    with pytest.raises(identity.Uncacheable, match="unverified import bytecode"):
        identity._check_bytecode(source)


@pytest.mark.parametrize("damage", ["runtime", "path_hook", "cached_importer"])
def test_unreviewed_import_context_is_refused(tmp_path, monkeypatch, damage):
    monkeypatch.setattr(identity, "COMPATIBILITY", {})
    monkeypatch.setattr(identity, "DEPENDENCIES", ())
    monkeypatch.setattr(identity, "DEPENDENCY_TREES", ())
    monkeypatch.setattr(sys, "path_hooks", [])
    monkeypatch.setattr(sys, "path_importer_cache", {})
    if damage == "runtime":
        monkeypatch.setattr(sys, "version_info", (3, 14))
    elif damage == "path_hook":
        monkeypatch.setattr(sys, "path_hooks", [lambda path: None])
    else:
        sys.path_importer_cache["synthetic"] = object()
    with pytest.raises(identity.Uncacheable):
        identity._import_context(tmp_path, {})


@pytest.mark.parametrize("damage", ["namespace", "missing", "loader", "extension", "linked_directory"])
def test_import_origins_and_package_directory_links_are_refused(tmp_path, monkeypatch, damage):
    root, shadow = tmp_path / "source", tmp_path / "shadow"
    root.mkdir()
    shadow.mkdir()
    monkeypatch.setattr(identity, "DEPENDENCIES", ())
    monkeypatch.setattr(identity, "DEPENDENCY_TREES", ())
    monkeypatch.setattr(sys, "path", [str(root)])
    monkeypatch.setattr(sys, "path_importer_cache", {})
    if damage == "namespace":
        (shadow / "synthetic").mkdir()
        monkeypatch.setattr(sys, "path", [str(shadow)])
        monkeypatch.setattr(identity, "COMPATIBILITY", {"synthetic.py": "synthetic"})
    elif damage == "missing":
        monkeypatch.setattr(identity, "COMPATIBILITY", {"missing.py": "synthetic"})
        monkeypatch.setattr(importlib.machinery.PathFinder, "_get_spec", lambda *args: None)
    elif damage in {"loader", "extension"}:
        source = root / "synthetic.py"
        source.write_text("synthetic = 1")
        monkeypatch.setattr(identity, "COMPATIBILITY", {"synthetic.py": "synthetic"})
        loader = (object() if damage == "loader" else importlib.machinery.ExtensionFileLoader("synthetic", str(source)))
        spec = SimpleNamespace(loader=loader, origin=str(source), submodule_search_locations=None)
        monkeypatch.setattr(importlib.machinery.PathFinder, "_get_spec", lambda *args: spec)
    else:
        package = root / "synthetic"
        package.mkdir()
        (package / "__init__.py").write_text("")
        (package / "module.py").write_text("synthetic = 1")
        (package / "linked").symlink_to(shadow)
        monkeypatch.setattr(identity, "COMPATIBILITY", {"synthetic/module.py": "synthetic"})
    with pytest.raises(identity.Uncacheable):
        identity._import_context(root, {})


@pytest.mark.parametrize("damage", ["outside", "oversized", "container", "duplicate"])
def test_model_metadata_has_closed_namespace_size_and_shape(tmp_path, damage):
    root = tmp_path / "source"
    root.mkdir()
    path = (tmp_path if damage == "outside" else root) / "config.json"
    path.write_text("{" + " " * 65536 + "}" if damage == "oversized" else
                    "[]" if damage == "container" else '{"x":1,"x":2}' if damage == "duplicate" else "{}")
    with pytest.raises(identity.Uncacheable):
        identity._json(path, (root,))


@pytest.mark.parametrize("damage", ["unknown", "encoder", "tokenizer", "weights"])
def test_top_model_metadata_rejects_unreviewed_branches(synthetic, damage):
    path = synthetic.top / "gliner_config.json"
    config = json.loads(path.read_text())
    if damage == "unknown":
        config["unreviewed_selector"] = True
    elif damage == "encoder":
        config["model_name"] = "../synthetic"
    elif damage == "tokenizer":
        (synthetic.top / "tokenizer_config.json").write_text("{}")
    else:
        for name in ("pytorch_model.bin", "model.safetensors"):
            (synthetic.top / name).unlink(missing_ok=True)
    path.write_text(json.dumps(config))
    with pytest.raises(identity.Uncacheable):
        identity._top(synthetic.top, (synthetic.top,))


@pytest.mark.parametrize("damage", ["architecture", "converted_tokenizer"])
def test_nested_model_branches_are_refused(synthetic, damage):
    if damage == "architecture":
        path = synthetic.nested / "config.json"
        config = json.loads(path.read_text())
        config["architectures"] = ["UnreviewedModel"]
        path.write_text(json.dumps(config))
    else:
        (synthetic.nested / "tokenizer.json").write_text("{}")
    with pytest.raises(identity.Uncacheable):
        identity._nested(synthetic.nested, (synthetic.nested,))


@pytest.mark.parametrize("damage", ["directory_ref", "invalid_revision"])
def test_offline_snapshot_ref_is_a_complete_file_identity(synthetic, damage):
    ref = synthetic.hub / "models--urchade--gliner_multi_pii-v1/refs/main"
    if damage == "directory_ref":
        ref.unlink()
        ref.mkdir()
    else:
        ref.write_text("z" * 40)
    with pytest.raises(identity.Uncacheable):
        identity._snapshot(synthetic.hub, identity.DEFAULT_MODEL)


@pytest.mark.parametrize("damage", ["origin", "reused", "descriptor", "converter_alias", "model_class", "config", "function_source"])
def test_capture_owns_the_loaded_object_and_live_configuration(synthetic, damage):
    plan = synthetic.plan()
    capture = identity.observe(plan)
    if damage == "origin":
        synthetic.model.__file__ = str(synthetic.root / "other.py")
        with pytest.raises(identity.Uncacheable):
            capture._module("gliner.model")
        return
    if damage == "descriptor":
        synthetic.model.GLiNER.from_pretrained = synthetic.original_entry.__func__
    elif damage == "converter_alias":
        function = synthetic.fast.convert_slow_tokenizer
        synthetic.fast.convert_slow_tokenizer = type(function)(function.__code__, function.__globals__)
    elif damage == "function_source":
        function = synthetic.state.Module.load_state_dict
        function.__code__ = function.__code__.replace(co_filename=str(synthetic.root / "unreviewed.py"))
    if damage in {"descriptor", "converter_alias", "function_source"}:
        with pytest.raises(identity.Uncacheable):
            with capture:
                pass
        return
    _, capture, retained = synthetic.capture(plan)
    if damage == "reused":
        with pytest.raises(identity.Uncacheable):
            with capture:
                pass
    else:
        if damage == "model_class":
            retained.__class__ = type("UnreviewedModel", (type(retained),), {})
        else:
            retained.config.model_type = "unreviewed"
        with pytest.raises(identity.Uncacheable):
            capture.receipt(retained)


@pytest.mark.parametrize("damage", ["state", "alias", "descriptor", "compiler_metadata"])
def test_bootstrap_live_members_match_the_pinned_source(tmp_path, monkeypatch, damage):
    source = tmp_path / "_distutils_hack" / "__init__.py"
    source.parent.mkdir()
    source.write_text('''class DistutilsMetaFinder:
    sensitive_tests = ["test.test_distutils"]
    def spec_for_sensitive_tests(self):
        return None
    @staticmethod
    def is_cpython():
        return True
    @staticmethod
    def frame_file_is_setup():
        return False
    @classmethod
    def pip_imported_during_build(cls):
        return False
setattr(DistutilsMetaFinder, "spec_for_test.test_distutils", DistutilsMetaFinder.spec_for_sensitive_tests)
''')
    pth = tmp_path / "distutils-precedence.pth"
    pth.write_text("import _distutils_hack\n")
    spec = importlib.util.spec_from_file_location("_distutils_hack", source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    monkeypatch.setitem(sys.modules, "_distutils_hack", module)
    monkeypatch.setitem(identity.BOOTSTRAPS, "_distutils_hack", ("_distutils_hack/__init__.py", "DistutilsMetaFinder",
        identity.file_digest(source), "distutils-precedence.pth", identity.file_digest(pth)))
    hook = module.DistutilsMetaFinder()
    identity._bootstrap_context(hook)
    if damage == "state":
        hook.unreviewed = True
    elif damage == "alias":
        setattr(type(hook), "spec_for_test.test_distutils", type(hook).is_cpython)
    elif damage == "descriptor":
        type(hook).is_cpython = vars(type(hook))["is_cpython"].__func__
    else:
        if sys.version_info[:2] != (3, 13):
            pytest.skip("CPython 3.13 compiler metadata")
        type(hook).__firstlineno__ += 1
    with pytest.raises(identity.Uncacheable):
        identity._bootstrap_context(hook)


@pytest.mark.parametrize("guard", ["symlink", "directory"])
def test_namespace_cycle_guards_each_reject_aliases(tmp_path, monkeypatch, guard):
    import stat
    root = tmp_path / "namespace"
    root.mkdir()
    alias = root / "alias"
    alias.symlink_to(root)
    monkeypatch.setattr(identity, "_links", lambda path: [])
    original_stat = Path.lstat
    if guard == "symlink":
        visits = []
        def lstat(path, *args, **kwargs):
            if path == root:
                visits.append(path)
                if len(visits) > 1:
                    return SimpleNamespace(st_mode=stat.S_IFREG)
            return original_stat(path, *args, **kwargs)
        monkeypatch.setattr(Path, "lstat", lstat)
        monkeypatch.setattr(identity, "file_digest", lambda path: "synthetic")
    else:
        monkeypatch.setattr(Path, "lstat", lambda path, *args, **kwargs:
            SimpleNamespace(st_mode=stat.S_IFDIR) if path == alias else original_stat(path, *args, **kwargs))
        monkeypatch.setattr(Path, "iterdir", lambda path: iter([alias] if path == root else []))
    with pytest.raises(identity.Uncacheable, match="cyclic namespace"):
        identity._inventory({root})


def test_comparison_refuses_missing_selected_models():
    with pytest.raises(ValueError, match="missing selected competitor model"):
        capture_compare.configuration(SimpleNamespace(en_model=None), "presidio-en", capture_compare.compare.load_mapping())


def test_private_report_predictor_is_an_independent_no_execution_boundary(tmp_path, monkeypatch):
    import benchmark_spec
    spec = benchmark_spec.manifest({"corpus_sha256": "a" * 64, "generator": None, "seed": 1,
        "contracts": {"v1": "all"}, "harness": {"driver": "b" * 64}, "record_schema": [1],
        "protocol": {}, "population": {"C": {"documents": 1}}})
    path = tmp_path / "spec.json"
    path.write_text(json.dumps(spec))
    monkeypatch.setattr(benchmark_spec, "require_current", lambda *args, **kwargs: None)
    monkeypatch.setattr(subject_identity, "require_population", lambda *args: None)
    monkeypatch.setattr(subject_identity, "runtime", lambda: {"python": "synthetic"})
    monkeypatch.setattr(capture_compare.compare, "load_corpus", lambda *args: ({}, {}))
    monkeypatch.setattr(capture_compare.compare, "preflight_contracts", lambda *args: None)
    doc = capture_compare.compare.score.Document("synthetic", "synthetic", "en", "", "synthetic", ())
    monkeypatch.setattr(capture_compare, "measure", lambda session, name, predict, *args, **kwargs: predict(doc))
    monkeypatch.setattr(capture_compare, "backend", lambda *args: pytest.fail("admission constructed a backend"))
    args = SimpleNamespace(benchmark_spec=path, dataset=path, pack_dir=None, cache_dir=tmp_path / "cache",
        predictions_dir=None, tool="datafog-core", verify_report=tmp_path / "report.json")
    with pytest.raises(ValueError, match="complete capture"):
        capture_compare.run(args)


@pytest.fixture
def tuned_plan(tmp_path, monkeypatch):
    import dataclasses
    from contextlib import nullcontext
    import capture_tune as tune
    model = tmp_path / "model"
    model.mkdir()
    (model / "weights").write_text("synthetic weights")
    dataset = tmp_path / "dataset"
    dataset.write_text("synthetic dataset")
    pool, authors = tmp_path / "pool", tmp_path / "authors"
    pool.mkdir()
    authors.mkdir()
    for name, directory in [(name, pool) for name in tune.POOL_PASSES] + [("authors", authors)]:
        for split in ("validation", "test"):
            (directory / f"{name}.{split}.jsonl").write_text("")
        (directory / f"{name}.meta.json").write_text("{}")
    sha = file_digest(pool / "base.validation.jsonl")
    selection = {"code_sha256": {"space.py": tune.sha256(tune.HERE / "space.py")},
                 "pool": {name: {"validation_sha256": sha} for name in tune.POOL_PASSES},
                 "choices": {objective: {"start": "synthetic", "finals": {"synthetic": {
                     "config": {"thresholds": {}, "allow_list": []}, "validation": {"leaked_bytes": 1, "false_positive_bytes": 0}}}}
                     for objective in tune.space.OBJECTIVES}}
    selected = tmp_path / "selection.json"
    selected.write_text(json.dumps(selection))
    plans_path = tmp_path / "plans.json"
    plans = {name: [name] for name in (*tune.POOL_PASSES, "authors")}
    plans_path.write_text(json.dumps(plans))
    spec = tmp_path / "spec.json"
    spec.write_text(json.dumps({"id": "synthetic-spec"}))
    args = SimpleNamespace(dataset=dataset, pool=pool, authors=authors, production_plans=plans_path,
        **{f"{lang}_model": model for lang in tune.space.LANGUAGES},
        dslim=model, davlan=model, openmed=model, gliner=model,
        budget_source_revision="a" * 40, allow_anchor_mismatch=False)
    production = {name: SimpleNamespace(mode="authors" if name == "authors" else "pool", pass_name=name,
        out=authors if name == "authors" else pool, dataset=dataset, benchmark_spec=spec) for name in plans}
    models = {lang: subject_identity.model_digest(model) for lang in tune.space.LANGUAGES}
    subjects = {name: {"models": {**models, **({name: subject_identity.model_digest(model)} if name != "base" else {})}}
                for name in tune.POOL_PASSES}
    subjects["authors"] = {"models": {"openmed": subject_identity.model_digest(model)}}
    descriptor = {"request": identity.Request("tuned", str(model), {
        "map_location": "cpu", "load_onnx_model": False, "onnx_model_file": "model.onnx"}).descriptor()}
    plan = SimpleNamespace(descriptor=descriptor, recheck=lambda: None)
    doc = tune.compare.score.Document("synthetic", "synthetic", "en", "", "synthetic", ())
    replay_doc = tune.search.Doc(doc.uid, "C", doc.language, doc, ())
    metas = {name: {"sha256": {"validation": sha, "test": sha}} for name in tune.POOL_PASSES}
    monkeypatch.setattr(tune, "SELECTION", selected)
    monkeypatch.setattr(tune, "pool_meta", lambda path: metas)
    monkeypatch.setattr(tune, "contracts", lambda: {})
    monkeypatch.setattr(tune, "tuned_mapping", lambda: {"EMAIL": ("EMAIL",)})
    monkeypatch.setattr(tune.corpus, "read_comparison", lambda: {"corpus": {}, "common_intersection_labels": ["EMAIL"]})
    monkeypatch.setattr(tune.corpus, "load_measured", lambda *args: {"C": [doc]})
    monkeypatch.setattr(tune.corpus, "measured_agentic_contract", lambda *args: nullcontext())
    monkeypatch.setattr(tune.corpus, "LAYERS", ("C",))
    monkeypatch.setattr(tune.search, "read_pool", lambda *args: {})
    monkeypatch.setattr(tune.search, "make_docs", lambda *args: [replay_doc])
    monkeypatch.setattr(tune.search, "Replayer", lambda docs, mapping, typed: SimpleNamespace(docs=docs, mapping=mapping, typed_mapping=typed))
    monkeypatch.setattr(tune.capture_produce, "parse_args", lambda argv: production[argv[0]])
    monkeypatch.setattr(tune.capture_produce, "verify_output", lambda args, layers: subjects[args.pass_name])
    monkeypatch.setattr(tune.capture_produce, "prepare", lambda *args: SimpleNamespace(gliner_plan=plan))
    monkeypatch.setattr(subject_identity, "runtime", lambda: {"python": "synthetic"})
    monkeypatch.setenv("PYTHONHASHSEED", "0")
    return SimpleNamespace(tune=tune, args=args, selection=selection, selected=selected, plans=plans,
                           production=production, subjects=subjects, metas=metas, spec=spec)


@pytest.mark.parametrize("damage", ["plans", "routing", "spec", "models", "search_space", "selection"])
def test_independent_tuned_plan_guards_reject_wrong_computation(tuned_plan, damage):
    case = tuned_plan
    if damage == "plans":
        case.plans.pop("authors")
        case.args.production_plans.write_text(json.dumps(case.plans))
    elif damage == "routing":
        case.production["base"].out = case.args.authors
    elif damage == "spec":
        other = case.spec.with_name("other.json")
        other.write_text(json.dumps({"id": "other-spec"}))
        case.production["authors"].benchmark_spec = other
    elif damage == "models":
        case.subjects["base"]["models"] = {}
    elif damage == "search_space":
        case.selection["code_sha256"]["space.py"] = "wrong"
        case.selected.write_text(json.dumps(case.selection))
    else:
        case.metas["base"]["sha256"]["validation"] = "wrong"
    with pytest.raises(ValueError):
        case.tune.prepare(case.args)


@pytest.mark.parametrize("damage", ["anchor", "validation"])
def test_tuned_measurement_refuses_unreproduced_anchor_or_selection(tuned_plan, monkeypatch, tmp_path, damage):
    case, tune = tuned_plan, tuned_plan.tune
    if damage == "anchor":
        for choice in case.selection["choices"].values():
            choice["finals"]["synthetic"]["validation"]["leaked_bytes"] = 0
        case.selected.write_text(json.dumps(case.selection))
    inputs = tune.prepare(case.args)
    monkeypatch.setattr(inputs, "verify_live", lambda objective: {"documents": 0, "differences": {}})
    monkeypatch.setattr(tune, "budget", lambda revision: {})
    monkeypatch.setattr(tune, "git_state", lambda: {})
    cell = {"metrics": {"product_coverage": {"validation": {"leaked_bytes": 0, "false_positive_bytes": 0}}}, "latency": {}}
    monkeypatch.setattr(tune.compare, "measure", lambda *args, **kwargs: {"contracts": {"v3": {"C": cell}}})
    monkeypatch.setattr(tune, "anchor", lambda *args: {"equal": damage != "anchor", "mismatches": ["synthetic"]})
    with pytest.raises(SystemExit):
        tune.measure(case.args, inputs, tmp_path / "private.json")


@pytest.mark.parametrize("damage", ["authors", "public"])
def test_tuned_admission_reproduces_authors_and_public_projection(tuned_plan, monkeypatch, tmp_path, damage):
    tune = tuned_plan.tune
    inputs = tune.prepare(tuned_plan.args)
    monkeypatch.setattr(inputs, "admit", lambda *args: None)
    monkeypatch.setattr(tune, "authors_predictions", lambda *args: {"synthetic": []})
    monkeypatch.setattr(tune.compare, "measure", lambda *args, **kwargs: {"leaked_bytes": 0})
    monkeypatch.setattr(tune, "strip_latency", lambda row: row)
    original = {"rows": {tune.ROWS["authors"]: {"leaked_bytes": 1 if damage == "authors" else 0}}, "provenance": {}}
    private = tmp_path / "private.json"
    private.write_text(json.dumps({"report": original}))
    submitted = copy.deepcopy(original)
    if damage == "public":
        submitted["unbound"] = True
    with pytest.raises(ValueError):
        tune.admit(tuned_plan.args, inputs, private, submitted)


@pytest.mark.parametrize("damage", ["private_output", "hash_seed", "smoke"])
def test_production_plan_refuses_unadmitted_execution_modes(tmp_path, monkeypatch, damage):
    import benchmark_spec
    import capture_produce as produce
    model = tmp_path / "model"
    model.mkdir()
    (model / "weights").write_text("synthetic")
    spec = benchmark_spec.manifest({"corpus_sha256": "a" * 64, "generator": None, "seed": 1,
        "contracts": {"v1": "all"}, "harness": {"driver": "b" * 64}, "record_schema": [1],
        "protocol": {}, "population": {"C": {"documents": 1}}})
    path = tmp_path / "spec.json"
    path.write_text(json.dumps(spec))
    args = SimpleNamespace(mode="pool", pass_name="base", out=tmp_path / "raw", benchmark_spec=path,
        dataset=tmp_path / "dataset", cache_dir=tmp_path / "cache", smoke=damage == "smoke",
        **{f"{language}_model": model for language in produce.space.LANGUAGES})
    monkeypatch.setattr(benchmark_spec, "require_current", lambda *args, **kwargs: None)
    monkeypatch.setattr(subject_identity, "require_population", lambda *args: None)
    monkeypatch.setattr(subject_identity, "runtime", lambda: {"python": "synthetic"})
    monkeypatch.setenv("PYTHONHASHSEED", "1" if damage == "hash_seed" else "0")
    if damage == "private_output":
        monkeypatch.setattr(produce.corpus.compare, "REPO", tmp_path / "repository")
        args.out = produce.corpus.compare.REPO / "raw"
    with pytest.raises(ValueError):
        produce.prepare(args, {})


def test_production_model_pin_is_enforced(tmp_path):
    import capture_produce as produce
    model = tmp_path / "model"
    model.mkdir()
    (model / "weights").write_text("synthetic")
    with pytest.raises(SystemExit):
        produce.check_pin("synthetic", model, {"synthetic": "wrong"})
    with pytest.raises(ValueError, match="directory is missing"):
        produce.tree_sha256(tmp_path / "missing")


def test_model_pin_rechecks_bytes_even_after_an_earlier_check(tmp_path):
    import capture_produce as produce
    model = tmp_path / "model"
    model.mkdir()
    weights = model / "weights"
    weights.write_text("synthetic")
    pins = {"synthetic": produce.tree_sha256(model)}
    produce.check_pin("synthetic", model, pins)
    weights.write_text("changed")
    with pytest.raises(SystemExit):
        produce.check_pin("synthetic", model, pins)


def test_raw_production_rejects_non_list_and_reordered_context_results(tmp_path):
    import capture_produce as produce
    from presidio_analyzer import RecognizerResult
    doc = SimpleNamespace(text="synthetic")
    with pytest.raises(ValueError, match="raw findings"):
        produce.validate_raw({}, doc, {})
    recognizer = SimpleNamespace(id="synthetic-unit", name="synthetic-unit",
        analyze=lambda **kwargs: [RecognizerResult("EMAIL", 0, 1, .1), RecognizerResult("EMAIL", 2, 3, .2)])
    engine = SimpleNamespace(registry=SimpleNamespace(get_recognizers=lambda **kwargs: [recognizer]),
        get_supported_entities=lambda **kwargs: ["EMAIL"], nlp_engine=SimpleNamespace(process_text=lambda *args: None),
        _enhance_using_context=lambda text, results, *args: list(reversed(results)))
    with pytest.raises(RuntimeError, match="reordered results"):
        produce.record_document(engine, {"synthetic-unit": "synthetic-unit"}, {"default": object()}, "synthetic", "en", True)


@pytest.mark.parametrize("damage", ["head", "dirty"])
def test_authors_source_checkout_is_verified_before_loading(tmp_path, monkeypatch, damage):
    import types
    import capture_produce as produce
    import tagged_gaze
    import theirbench
    here = tmp_path / "tuned"
    vendor = here.parent / "theirbench"
    vendor.mkdir()
    (vendor / "vendor-tuned.json").write_text('{"presidio-research":{"model":{}}}')
    monkeypatch.setattr(produce, "HERE", here)
    monkeypatch.setattr(produce, "tuned_presidio_tree", lambda path: "synthetic-model")
    monkeypatch.setattr(tagged_gaze, "check_model_receipt", lambda *args: None)
    def no_load(*args):
        pytest.fail("source refusal must precede model loading")
    repro = SimpleNamespace(COMMIT="a" * 40, custom_analyzer=no_load)
    monkeypatch.setattr(theirbench, "presidio_research_repro", repro, raising=False)
    package = types.ModuleType("presidio_evaluator")
    package.__path__ = []
    module = types.ModuleType("presidio_evaluator.models")
    module.PresidioAnalyzerWrapper = object
    monkeypatch.setitem(sys.modules, "presidio_evaluator", package)
    monkeypatch.setitem(sys.modules, "presidio_evaluator.models", module)
    monkeypatch.setattr(produce.subprocess, "check_output", lambda command, **kwargs:
        ("changed" if damage == "dirty" else "") if "--porcelain" in command else
        ("b" * 40 if damage == "head" else "a" * 40))
    args = SimpleNamespace(openmed=tmp_path / "model", checkout=tmp_path / "checkout")
    with pytest.raises(SystemExit):
        produce.authors_pass(args, {}, None)
