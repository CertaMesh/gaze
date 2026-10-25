"""Synthetic identity contracts only: no ML, payloads, datasets or old observations."""
from __future__ import annotations

import builtins
import copy
import hashlib
import inspect
import importlib.machinery
import importlib.util
import json
import marshal
import os
import subprocess
import struct
import sys
import tempfile
import types
import unittest
from contextlib import ExitStack
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import gliner_identity as identity


CONFIG_SOURCE = '''
class DebertaV2Config:
    model_type = "deberta-v2"
class UniEncoderSpanConfig:
    model_type = "gliner_uni_encoder_span"
    words_splitter_type = "whitespace"
    model_name = "synthetic/encoder"
    def __init__(self):
        self.encoder_config = DebertaV2Config()
class GLiNERConfig(UniEncoderSpanConfig):
    pass
'''
STATE_SOURCE = '''
def result():
    value = Result(missing_keys=list(MISSING), unexpected_keys=["ignored.extra"])
    RETURNS.append(value)
    return value
class Module:
    def load_state_dict(self, state_dict, strict=True, assign=False):
        CALLS.append((self, state_dict, strict, assign))
        if ERROR is not None:
            raise ERROR
        return result()
'''
SLOW_SOURCE = '''
class DebertaV2Tokenizer:
    def __init__(self):
        self.bytes = SPM_FILE.read_bytes()
'''
CONVERTER_SOURCE = '''
def convert_slow_tokenizer(transformer_tokenizer, from_tiktoken=False):
    CALLS.append((transformer_tokenizer, from_tiktoken))
    if ERROR is not None:
        raise ERROR
    return object()
'''
FAST_SOURCE = '''
class PreTrainedTokenizerFast:
    def __init__(self, slow):
        if slow is False:
            self._tokenizer = convert_slow_tokenizer(self, from_tiktoken=True)
        else:
            self._tokenizer = convert_slow_tokenizer(slow)
'''
FAST_DEBERTA_SOURCE = '''
class DebertaV2TokenizerFast(PreTrainedTokenizerFast):
    pass
'''
MODEL_SOURCE = '''
class UniEncoderSpanGLiNER:
    pass
class BaseGLiNER:
    @classmethod
    def from_pretrained(cls, model_id, **kwargs):
        instance = UniEncoderSpanGLiNER()
        instance.model = Module()
        instance.config = UniEncoderSpanConfig()
        slow = False if FALLBACK else DebertaV2Tokenizer()
        tokenizer = DebertaV2TokenizerFast(slow)
        instance.data_processor = Result(transformer_tokenizer=tokenizer)
        state_dict = WEIGHT_FILE.read_bytes()
        result = instance.model.load_state_dict(state_dict, strict=False)
        STATE_RESULTS.append(result)
        if ERROR is not None:
            raise ERROR
        RETURNS.append(instance)
        return instance
class GLiNER:
    @classmethod
    def from_pretrained(cls, model_id, **model_kwargs):
        CALLS.append((cls, model_id, model_kwargs))
        return BaseGLiNER.from_pretrained(model_id, **model_kwargs)
'''
ENGINE_SOURCE = '''
from functools import lru_cache
@lru_cache(maxsize=1)
def _get_gliner_annotator():
    try:
        return Result(model=GLiNER.from_pretrained("urchade/gliner_multi_pii-v1", local_files_only=True))
    except Exception:
        return Result(model=None)
'''


class IdentityContract(unittest.TestCase):
    def setUp(self):
        self.stack = ExitStack()
        self.addCleanup(self.stack.close)
        # Every fixture models a fresh collector, regardless of other tests'
        # installed Presidio/transformers imports in the shared pytest process.
        roots = {name.partition("/")[0] for name in identity.COMPATIBILITY}
        roots.update(name.partition(".")[0] for name in identity.DEPENDENCIES)
        self.stack.enter_context(patch.dict(sys.modules, {
            name: value for name, value in sys.modules.copy().items()
            if name.partition(".")[0] not in roots}, clear=True))
        self.temp = self.stack.enter_context(tempfile.TemporaryDirectory(prefix="gliner-synthetic-"))
        self.root = Path(self.temp)
        self.cwd = self.root / "cwd"
        self.cwd.mkdir()
        previous = Path.cwd()
        os.chdir(self.cwd)
        self.stack.callback(os.chdir, previous)
        self.hub = self.root / "hub"
        self.tf = self.root / "transformers"
        self.env = {"HOME": str(self.root / "home"), "HF_HUB_CACHE": str(self.hub),
                    "TRANSFORMERS_CACHE": str(self.tf), "HF_HUB_OFFLINE": "1"}
        self.runtime = {"interpreter_sha256": "1" * 64, "packages_sha256": "2" * 64}
        self.top = self.repository(self.hub, identity.DEFAULT_MODEL, "a" * 40, {
            "gliner_config.json": json.dumps({"model_name": "synthetic/encoder", "span_mode": "markerV0"}),
            "pytorch_model.bin": "asymmetric top bytes AAA-17"})
        self.nested = self.repository(self.tf, "synthetic/encoder", "b" * 40, {
            "config.json": json.dumps({"model_type": "deberta-v2"}),
            "tokenizer_config.json": json.dumps({"do_lower_case": False}),
            "spm.model": "asymmetric vocab BBB-29"})
        self.hub_nested = self.repository(self.hub, "synthetic/encoder", "c" * 40, {
            "config.json": json.dumps({"model_type": "deberta-v2"}),
            "tokenizer_config.json": "{}", "spm.model": "distinct hub template CCC-31"})
        self.sources = self.root / "source"
        texts = {name: "# synthetic compatibility source\n" for name in identity.COMPATIBILITY}
        texts.update({"gliner/config.py": CONFIG_SOURCE, "gliner/model.py": MODEL_SOURCE,
                      "torch/nn/modules/module.py": STATE_SOURCE,
                      "transformers/tokenization_utils_fast.py": FAST_SOURCE,
                      "transformers/convert_slow_tokenizer.py": CONVERTER_SOURCE,
                      "transformers/models/deberta_v2/tokenization_deberta_v2.py": SLOW_SOURCE,
                      "transformers/models/deberta_v2/tokenization_deberta_v2_fast.py": FAST_DEBERTA_SOURCE,
                      "datafog/engine.py": ENGINE_SOURCE})
        pins = {}
        for name, text in texts.items():
            file = self.sources / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text(text)
            pins[name] = hashlib.sha256(text.encode()).hexdigest()
            for parent in file.parents:
                if parent == self.sources:
                    break
                (parent / "__init__.py").touch()
        for module_name in identity.DEPENDENCIES:
            path = self.sources.joinpath(*module_name.split("."))
            path = path / "__init__.py" if module_name in {"tokenizers", "sentencepiece", "google.protobuf"} else path.with_suffix(".py")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("# synthetic inventoried tokenizer dependency\n")
        self.runtime["import_files"] = {
            str(path): hashlib.sha256(path.read_bytes()).hexdigest()
            for relative in identity.DEPENDENCY_TREES
            for path in (self.sources / relative).rglob("*.py")}
        self.stack.enter_context(patch.object(identity, "COMPATIBILITY", pins))
        self.stack.enter_context(patch.object(sys, "path", [str(self.sources), *sys.path]))
        self.modules = {}
        def module(name, globals=None):
            path = self.sources / (name.replace(".", "/") + ".py")
            value = types.ModuleType(name)
            value.__file__ = str(path)
            value.__dict__.update(globals or {})
            exec(compile(path.read_text(), str(path), "exec"), value.__dict__)
            self.modules[name] = value
            return value
        configs = module("gliner.config")
        self.state = module("torch.nn.modules.module", {"Result": types.SimpleNamespace, "CALLS": [], "RETURNS": [], "MISSING": [], "ERROR": None})
        self.slow = module("transformers.models.deberta_v2.tokenization_deberta_v2", {"SPM_FILE": self.nested / "spm.model"})
        self.converter = module("transformers.convert_slow_tokenizer", {"CALLS": [], "ERROR": None})
        self.fast = module("transformers.tokenization_utils_fast", {"convert_slow_tokenizer": self.converter.convert_slow_tokenizer})
        self.fast_deberta = module("transformers.models.deberta_v2.tokenization_deberta_v2_fast", {"PreTrainedTokenizerFast": self.fast.PreTrainedTokenizerFast})
        self.model = module("gliner.model", {"Result": types.SimpleNamespace, "Module": self.state.Module,
            "UniEncoderSpanConfig": configs.UniEncoderSpanConfig, "DebertaV2Tokenizer": self.slow.DebertaV2Tokenizer,
            "DebertaV2TokenizerFast": self.fast_deberta.DebertaV2TokenizerFast,
            "WEIGHT_FILE": self.top / "pytorch_model.bin", "FALLBACK": False, "ERROR": None,
            "CALLS": [], "RETURNS": [], "STATE_RESULTS": []})
        self.engine = module("datafog.engine", {"Result": types.SimpleNamespace, "GLiNER": self.model.GLiNER})
        # Synthetic source binding deliberately substitutes tiny reviewed sources;
        # production has no API to choose another compatibility table.
        self.stack.enter_context(patch.object(identity.importlib, "import_module", self.import_fake))
        # Model the fresh standard launcher independently of test-runner hooks.
        machinery = importlib.machinery
        self.stack.enter_context(patch.object(sys, "meta_path", [
            machinery.BuiltinImporter, machinery.FrozenImporter, machinery.PathFinder]))
        self.stack.enter_context(patch.object(sys, "path_importer_cache", {}))
        self.original_entry = inspect.getattr_static(self.model.GLiNER, "from_pretrained")
        self.original_state = inspect.getattr_static(self.state.Module, "load_state_dict")
        self.original_alias = self.fast.convert_slow_tokenizer
        self.request = identity.Request.datafog()

    def import_fake(self, name):
        if name not in self.modules:
            raise AssertionError("unexpected module import: " + name)
        return self.modules[name]

    def repository(self, cache, repo, revision, files):
        root = cache / ("models--" + repo.replace("/", "--"))
        path = root / "snapshots" / revision
        path.mkdir(parents=True)
        (root / "refs").mkdir(exist_ok=True)
        (root / "refs/main").write_text(revision)
        for name, text in files.items():
            (path / name).write_text(text)
        return path

    def plan(self, request=None):
        return identity.plan(request or self.request, source_root=self.sources,
                             runtime_identity=self.runtime, environment=self.env)

    def capture(self, plan=None):
        plan = plan or self.plan()
        with identity.observe(plan) as observed:
            annotator = self.engine._get_gliner_annotator()
            self.assertIs(self.engine._get_gliner_annotator(), annotator)
        return plan, observed, annotator.model

    def assert_restored(self):
        self.assertIs(inspect.getattr_static(self.model.GLiNER, "from_pretrained"), self.original_entry)
        self.assertIs(inspect.getattr_static(self.state.Module, "load_state_dict"), self.original_state)
        self.assertIs(self.fast.convert_slow_tokenizer, self.original_alias)

    def test_positive_original_capture_then_zero_import_validation(self):
        plan, observed, retained = self.capture()
        receipt = observed.receipt(retained)
        self.assert_restored()
        self.assertEqual(self.model.CALLS, [(self.model.GLiNER, identity.DEFAULT_MODEL, {"local_files_only": True})])
        self.assertEqual(len(self.state.CALLS), 1)
        self.assertIs(self.state.CALLS[0][0], retained.model)
        self.assertEqual(self.state.CALLS[0][1:], (b"asymmetric top bytes AAA-17", False, False))
        self.assertIs(self.model.RETURNS[0], retained)
        self.assertIs(self.model.STATE_RESULTS[0], self.state.RETURNS[0])
        self.assertEqual(len(self.converter.CALLS), 1)
        self.assertEqual(self.converter.CALLS[0][0].bytes, b"asymmetric vocab BBB-29")
        self.assertFalse(self.converter.CALLS[0][1])
        with patch.object(identity.importlib, "import_module", side_effect=AssertionError("ML import")):
            fresh = self.plan()
            self.assertEqual(fresh.descriptor, plan.descriptor)
            self.assertEqual(identity.verify_receipt(fresh, receipt), identity.digest(receipt))
        self.assertEqual(len(self.model.CALLS), 1)

    def test_same_class_successful_tiktoken_fallback_rejects(self):
        self.model.FALLBACK = True
        plan, observed, retained = self.capture()
        self.assertIs(type(retained.data_processor.transformer_tokenizer), self.fast_deberta.DebertaV2TokenizerFast)
        self.assertEqual(self.model.STATE_RESULTS[0].missing_keys, [])
        self.assertTrue(self.converter.CALLS[0][1])
        with self.assertRaises(identity.Uncacheable):
            observed.receipt(retained)
        self.assert_restored()

    def test_source_and_namespace_content_mutations(self):
        cases = [self.top / "pytorch_model.bin", self.nested / "spm.model",
                 self.hub_nested / "spm.model", self.top.parent.parent / "refs/main"]
        for path in cases:
            with self.subTest(path=path.name):
                before = self.plan()
                old = path.read_bytes()
                # Ref remains valid but selects an independently populated tree.
                if path.name == "main":
                    alternate = self.repository(self.hub, identity.DEFAULT_MODEL, "d" * 40, {
                        "gliner_config.json": '{"model_name":"synthetic/encoder"}',
                        "pytorch_model.bin": "independent changed top DDD-47"})
                    self.assertNotEqual(self.plan().sha256, before.sha256)
                    import shutil
                    shutil.rmtree(alternate)
                else:
                    path.write_bytes(old + b" CHANGED")
                    self.assertNotEqual(self.plan().sha256, before.sha256)
                with self.assertRaises(identity.Uncacheable):
                    before.recheck()
                path.write_bytes(old)
        source = self.sources / "gliner/config.py"
        source.write_text(source.read_text() + "# drift\n")
        with self.assertRaises(identity.Uncacheable):
            self.plan()

    def test_preferred_file_negative_marker_directory_changes(self):
        for path in (self.top / "model.safetensors", self.nested / "new-template.jinja",
                     self.nested.parent.parent / ".no_exist" / "missing"):
            before = self.plan()
            path.parent.mkdir(exist_ok=True)
            path.write_text("synthetic new preferred or negative file")
            self.assertNotEqual(self.plan().sha256, before.sha256)
            path.unlink()
        before = self.plan()
        (self.nested / "empty-listing").mkdir()
        self.assertNotEqual(self.plan().sha256, before.sha256)

    def test_top_and_nested_cwd_shadows_and_symlink_targets(self):
        for repo, source in ((identity.DEFAULT_MODEL, self.top), ("synthetic/encoder", self.nested)):
            with self.subTest(repo=repo):
                before = self.plan()
                shadow = self.cwd / repo
                shadow.parent.mkdir(exist_ok=True)
                shadow.symlink_to(source, target_is_directory=True)
                self.assertNotEqual(self.plan().sha256, before.sha256)
                shadow.unlink()
        weight = self.top / "pytorch_model.bin"
        weight.unlink()
        blob = self.top.parent.parent / "blobs"
        blob.mkdir()
        (blob / "first").write_bytes(b"distinct blob one 11")
        (blob / "second").write_bytes(b"distinct blob two 22")
        weight.symlink_to(blob / "first")
        before = self.plan()
        weight.unlink()
        weight.symlink_to(blob / "second")
        self.assertNotEqual(self.plan().sha256, before.sha256)
        weight.unlink()
        outside = self.root / "outside"
        outside.write_text("unbound resource")
        weight.symlink_to(outside)
        with self.assertRaises(identity.Uncacheable):
            self.plan()

    def test_unused_cli_assets_do_not_supply_effective_identity(self):
        cli = self.root / "unused-cli-model"
        cli.mkdir()
        asset = cli / "pytorch_model.bin"
        asset.write_bytes(b"unused asymmetric 15")
        before = self.plan()
        asset.write_bytes(b"unused asymmetric 26")
        self.assertEqual(self.plan().descriptor, before.descriptor)
        (self.top / "pytorch_model.bin").write_bytes(b"effective changed 37")
        self.assertNotEqual(self.plan().sha256, before.sha256)

    def test_selectors_split_roots_unset_empty_and_expansion(self):
        before = self.plan()
        self.env["DATA_GYM_CACHE_DIR"] = ""
        self.assertNotEqual(self.plan().sha256, before.sha256)
        del self.env["DATA_GYM_CACHE_DIR"]
        self.env["SYNTHETIC_HUB"] = str(self.hub)
        self.env["HF_HUB_CACHE"] = "$SYNTHETIC_HUB"
        expanded = self.plan()
        self.assertNotEqual(expanded.sha256, before.sha256)
        self.env["SYNTHETIC_HUB"] = str(self.tf)
        with self.assertRaises((identity.Uncacheable, OSError)):
            self.plan()
        self.env["HF_HUB_CACHE"] = str(self.hub)
        for selector in ("USE_FLASHDEBERTA", "TURBOT5_ATTN_TYPE"):
            self.env[selector] = "active"
            with self.assertRaises(identity.Uncacheable):
                self.plan()
            del self.env[selector]
        self.env["HF_HUB_OFFLINE"] = "0"
        self.env["TRANSFORMERS_OFFLINE"] = "1"
        with self.assertRaises(identity.Uncacheable):
            self.plan()
        del self.env["TRANSFORMERS_OFFLINE"]
        del self.env["HF_HUB_OFFLINE"]
        with self.assertRaises(identity.Uncacheable):
            self.plan()

    def test_unknown_metadata_and_external_overrides_reject(self):
        cases = [(self.top / "gliner_config.json", "words_splitter_type", "stanza"),
                 (self.top / "gliner_config.json", "labels_decoder", "other/model"),
                 (self.top / "gliner_config.json", "encoder_config", {}),
                 (self.nested / "config.json", "model_type", "unknown"),
                 (self.nested / "config.json", "auto_map", {"AutoModel": "remote.Class"}),
                 (self.nested / "tokenizer_config.json", "tokenizer_file", "/outside/tokenizer"),
                 (self.nested / "tokenizer_config.json", "tokenizer_class", "CustomTokenizer")]
        for path, field, value in cases:
            with self.subTest(field=field):
                old = path.read_text()
                metadata = json.loads(old)
                metadata[field] = value
                path.write_text(json.dumps(metadata))
                with self.assertRaises(identity.Uncacheable):
                    self.plan()
                path.write_text(old)

    def test_missing_state_keys_reject_without_changing_strict(self):
        self.state.MISSING.append("synthetic.missing")
        _, observed, retained = self.capture()
        self.assertEqual(self.state.CALLS[0][2], False)
        with self.assertRaises(identity.Uncacheable):
            observed.receipt(retained)
        self.assertEqual(len(self.state.CALLS), 1)
        self.assert_restored()

    def test_preloaded_objects_not_cleared_or_reloaded(self):
        retained = self.engine._get_gliner_annotator().model
        plan = self.plan()
        calls = len(self.model.CALLS)
        _, observed, same = self.capture(plan)
        self.assertIs(same, retained)
        self.assertEqual(len(self.model.CALLS), calls)
        with self.assertRaises(identity.Uncacheable):
            observed.receipt(retained)
        self.assertEqual(self.engine._get_gliner_annotator.cache_info().currsize, 1)

    def test_swallowed_errors_stay_out_of_band(self):
        for component in (self.converter, self.state, self.model):
            with self.subTest(component=component.__name__):
                self.engine._get_gliner_annotator.cache_clear()
                component.ERROR = RuntimeError("synthetic original failure")
                _, observed, retained = self.capture()
                self.assertIsNone(retained)
                with self.assertRaises(identity.Uncacheable):
                    observed.receipt(retained)
                component.ERROR = None
                self.assert_restored()

    def test_exception_identity_and_descriptor_restoration(self):
        plan = self.plan(identity.Request("standalone", str(self.top), {}))
        failure = RuntimeError("original sentinel")
        self.model.ERROR = failure
        with self.assertRaises(RuntimeError) as caught:
            with identity.observe(plan):
                self.model.GLiNER.from_pretrained(str(self.top))
        self.assertIs(caught.exception, failure)
        self.assert_restored()

    def test_retained_object_and_conversion_backend_must_match(self):
        _, observed, retained = self.capture()
        with self.assertRaises(identity.Uncacheable):
            observed.receipt(object())
        retained.data_processor.transformer_tokenizer._tokenizer = object()
        with self.assertRaises(identity.Uncacheable):
            observed.receipt(retained)

    def test_post_capture_drift_and_forged_record_owned_plan(self):
        plan, observed, retained = self.capture()
        receipt = observed.receipt(retained)
        foreign = copy.deepcopy(receipt)
        foreign["plan_sha256"] = "f" * 64
        foreign["roots"] = [str(self.root / "invented-record-root")]
        with self.assertRaises(identity.Uncacheable):
            identity.verify_receipt(self.plan(), foreign)
        for proof in ("entry", "state", "conversion", "retained", "plan_sha256"):
            missing = copy.deepcopy(receipt)
            del missing[proof]
            with self.assertRaises(identity.Uncacheable):
                identity.verify_receipt(plan, missing)
        (self.nested / "spm.model").write_bytes(b"post-capture drift")
        with self.assertRaises(identity.Uncacheable):
            observed.receipt(retained)
        with self.assertRaises(identity.Uncacheable):
            identity.verify_receipt(self.plan(), receipt)

    def test_unchanged_tuned_entry_args_and_return_sharing(self):
        kwargs = {"map_location": "cpu", "load_onnx_model": False, "onnx_model_file": "model.onnx"}
        plan = self.plan(identity.Request("tuned", str(self.top), kwargs))
        with identity.observe(plan) as observed:
            returned = self.model.GLiNER.from_pretrained(str(self.top), **kwargs)
            shallow = copy.copy(types.SimpleNamespace(gliner=returned, threshold=0.3, labels=["person"]))
        identity.verify_receipt(plan, observed.receipt(shallow.gliner))
        self.assertEqual(self.model.CALLS, [(self.model.GLiNER, str(self.top), kwargs)])
        self.assertIs(shallow.gliner, returned)
        self.assertEqual((shallow.threshold, shallow.labels), (0.3, ["person"]))
        self.assert_restored()

    def test_unrelated_state_load_cannot_supply_top_proof(self):
        plan = self.plan()
        with identity.observe(plan) as observed:
            self.state.Module().load_state_dict(b"unrelated", strict=False)
        with self.assertRaises(identity.Uncacheable):
            observed.receipt(object())

    def test_import_shadow_unverified_bytecode_and_preimported_modules_reject(self):
        initial = self.plan()
        shadow = self.cwd / "gliner.py"
        shadow.write_text("raise AssertionError('never execute a package shadow')\n")
        with patch.object(sys, "path", [str(self.cwd), *sys.path]):
            with self.assertRaises(identity.Uncacheable):
                self.plan()
        shadow.unlink()
        with patch.dict(sys.modules, {"huggingface_hub.constants": types.ModuleType("huggingface_hub.constants")}):
            with self.assertRaises(identity.Uncacheable):
                self.plan()
        import importlib.util
        cached = Path(importlib.util.cache_from_source(str(self.sources / "gliner/model.py")))
        cached.parent.mkdir()
        cached.write_bytes(b"synthetic unverified bytecode")
        with self.assertRaises(identity.Uncacheable):
            self.plan()
        cached.unlink()
        self.assertEqual(self.plan().sha256, initial.sha256)

    def test_unknown_import_hook_and_missing_runtime_proof_reject(self):
        with patch.object(sys, "meta_path", [*sys.meta_path, object()]):
            with self.assertRaises(identity.Uncacheable):
                self.plan()
        del self.runtime["packages_sha256"]
        with self.assertRaises(identity.Uncacheable):
            self.plan()

    def test_package_initializers_are_bound_but_never_executed_by_planning(self):
        initial = self.plan()
        package = self.sources / "gliner/__init__.py"
        package.write_text("raise AssertionError('planner must never execute ML initializers')\n")
        self.assertNotEqual(self.plan().sha256, initial.sha256)
        with self.assertRaises(identity.Uncacheable):
            initial.recheck()

    def test_observer_replacement_deletion_and_failure_restore_descriptors(self):
        plan = self.plan(identity.Request("standalone", str(self.top), {}))
        failure = RuntimeError("original outer exception")
        with self.assertRaises(RuntimeError) as caught:
            with identity.observe(plan) as observed:
                retained = self.model.GLiNER.from_pretrained(str(self.top))
                del self.model.GLiNER.from_pretrained
                raise failure
        self.assertIs(caught.exception, failure)
        self.assert_restored()
        with self.assertRaises(identity.Uncacheable):
            observed.receipt(retained)
        with patch.object(identity.importlib, "import_module", side_effect=RuntimeError("fake import failure")):
            with self.assertRaises(RuntimeError):
                with identity.observe(plan):
                    self.fail("failed observer installation must not enter the body")
        self.assert_restored()

    def test_entry_mismatch_and_multiple_loads_are_observed_without_interference(self):
        plan = self.plan(identity.Request("standalone", str(self.top), {}))
        with identity.observe(plan) as observed:
            first = self.model.GLiNER.from_pretrained(str(self.top), map_location="cpu")
            second = self.model.GLiNER.from_pretrained(str(self.top))
        self.assertIsNot(first, second)
        self.assertEqual(len(self.model.CALLS), 2)
        self.assertEqual(len(self.state.CALLS), 2)
        with self.assertRaises(identity.Uncacheable):
            observed.receipt(second)
        self.assert_restored()

    def test_metadata_link_escape_broken_links_and_duplicate_keys_reject(self):
        config = self.top / "gliner_config.json"
        old = config.read_text()
        outside = self.root / "outside-config.json"
        outside.write_text(old)
        config.unlink()
        config.symlink_to(outside)
        with self.assertRaises(identity.Uncacheable):
            self.plan()
        outside.unlink()
        with self.assertRaises(identity.Uncacheable):
            self.plan()
        config.unlink()
        config.write_text('{"model_name":"synthetic/encoder","model_name":"other/encoder"}')
        with self.assertRaises(identity.Uncacheable):
            self.plan()
        config.write_text(old)
        cycle = self.top / "cycle"
        cycle.symlink_to(self.top, target_is_directory=True)
        with self.assertRaises(identity.Uncacheable):
            self.plan()

    def test_all_legacy_root_defaults_and_empty_selector_preservation(self):
        # Compare identities against a separately chosen physical namespace;
        # no record-owned paths feed the planner.
        for selector in ("PYTORCH_PRETRAINED_BERT_CACHE", "PYTORCH_TRANSFORMERS_CACHE"):
            old = dict(self.env)
            del self.env["TRANSFORMERS_CACHE"]
            self.env[selector] = str(self.tf)
            planned = self.plan()
            self.assertEqual(planned.descriptor["context"]["selectors"][selector], {"set": True, "value": str(self.tf)})
            self.env.clear()
            self.env.update(old)
        old = dict(self.env)
        del self.env["HF_HUB_CACHE"]
        self.env["HUGGINGFACE_HUB_CACHE"] = str(self.hub)
        self.plan()
        del self.env["HUGGINGFACE_HUB_CACHE"]
        self.env["HF_HOME"] = str(self.root / "custom-home")
        (self.root / "custom-home").mkdir()
        (self.root / "custom-home/hub").symlink_to(self.hub, target_is_directory=True)
        home_plan = self.plan()
        self.env["HF_HOME"] = str(self.root / "different-home")
        with self.assertRaises(identity.Uncacheable):
            self.plan()
        self.env.clear()
        self.env.update(old)
        self.assertNotEqual(home_plan.sha256, self.plan().sha256)

    def test_variable_introduced_tilde_binds_effective_literal_tree(self):
        self.env.update(SYNTHETIC_HUB="~/alternative-hub", HF_HUB_CACHE="$SYNTHETIC_HUB")
        effective = self.cwd / "~/alternative-hub"
        unused = Path(self.env["HOME"]) / "alternative-hub"
        files = {"gliner_config.json": '{"model_name":"synthetic/encoder"}',
                 "pytorch_model.bin": "unused planned weight"}
        unused_top = self.repository(unused, identity.DEFAULT_MODEL, "a" * 40, files)
        actual = self.repository(effective, identity.DEFAULT_MODEL, "a" * 40,
                                 {**files, "pytorch_model.bin": "effective weight A"})
        self.model.WEIGHT_FILE = actual / "pytorch_model.bin"
        plan, observed, retained = self.capture()
        receipt = observed.receipt(retained)
        self.assertEqual(self.state.CALLS[0][1], b"effective weight A")
        (unused_top / "pytorch_model.bin").write_bytes(b"unused changed weight")
        self.assertEqual(self.plan().sha256, plan.sha256)
        self.model.WEIGHT_FILE.write_bytes(b"effective weight B")
        self.assertNotEqual(self.plan().sha256, plan.sha256)
        with self.assertRaises(identity.Uncacheable):
            identity.verify_receipt(self.plan(), receipt)

    def test_root_expansion_order_and_legacy_literals(self):
        cwd = self.cwd
        env = {"HOME": str(self.root / "home"), "HF_HUB_CACHE": "~/direct"}
        expected = self.root / "home/direct"
        self.assertEqual(identity._roots(env, cwd), (expected, expected))
        env.update(HF_HUB_CACHE="$ROOT", ROOT="~/literal", TRANSFORMERS_CACHE="$ROOT")
        self.assertEqual(identity._roots(env, cwd), (cwd / "~/literal", cwd / "$ROOT"))
        env = {"HOME": str(self.root / "home"), "HF_HOME": "$ROOT", "ROOT": "~/home"}
        # HF_HOME and then HF_HUB_CACHE each apply one expansion pass.
        self.assertEqual(identity._roots(env, cwd), (self.root / "home/home/hub",) * 2)

    def test_api_compatible_loose_tokenizer_cannot_supply_conversion_identity(self):
        external = self.root / "loose-pythonpath"
        external.mkdir()
        shadow = external / "tokenizers.py"
        shadow.write_text('class Tokenizer:\n    behavior = "outside inventory"\n')
        with patch.object(sys, "path", [str(external), *sys.path]):
            selected = importlib.machinery.PathFinder.find_spec("tokenizers", sys.path)
            self.assertEqual(Path(selected.origin), shadow)
            dependency = importlib.util.module_from_spec(selected)
            selected.loader.exec_module(dependency)
            # A compatible backend can actually flow through conversion and
            # remain the fast tokenizer's retained backend, not just exist.
            self.converter.__dict__["object"] = dependency.Tokenizer
            try:
                plan, observed, retained = self.capture()
            except identity.Uncacheable:
                return
            receipt = observed.receipt(retained)
            self.assertEqual(retained.data_processor.transformer_tokenizer._tokenizer.behavior,
                             "outside inventory")
            identity.verify_receipt(plan, receipt)
            self.fail("unverified loose dependency supplied an admitted conversion backend")

    def test_preloaded_tokenizer_dependencies_refuse_initial_plan(self):
        for name in ("tokenizers", "sentencepiece", "google", "google._upb", "google.protobuf", "google._upb._message"):
            with self.subTest(name=name), patch.dict(sys.modules, {name: types.ModuleType(name)}):
                with self.assertRaisesRegex(identity.Uncacheable, "fresh process"):
                    self.plan()

    def test_dependency_origin_lookup_is_source_only(self):
        external = self.root / "unexecuted-pythonpath"
        external.mkdir()
        (external / "sentencepiece.py").write_text("raise AssertionError('source-only lookup executed a dependency')\n")
        with patch.object(sys, "path", [str(external), *sys.path]), \
                patch.object(identity.importlib, "import_module", side_effect=AssertionError("ML import")):
            with self.assertRaisesRegex(identity.Uncacheable, "package shadow"):
                self.plan()

    def test_dependency_sources_and_native_origins_require_independent_inventory(self):
        initial = self.plan()
        for relative in ("tokenizers/__init__.py", "sentencepiece/sentencepiece_model_pb2.py",
                         "google/protobuf/__init__.py"):
            path = self.sources / relative
            old = path.read_bytes()
            path.write_bytes(old + b"# changed dependency behavior\n")
            with self.subTest(relative=relative), self.assertRaises(identity.Uncacheable):
                initial.recheck()
            path.write_bytes(old)
        native = self.sources / ("tokenizers/tokenizers" + importlib.machinery.EXTENSION_SUFFIXES[0])
        native.write_bytes(b"tiny fake native content, never loaded")
        with self.assertRaisesRegex(identity.Uncacheable, "runtime content inventory"):
            self.plan()

        self.runtime["import_files"][str(native)] = hashlib.sha256(native.read_bytes()).hexdigest()
        planned = self.plan()
        self.assertEqual(planned.descriptor["context"]["import_origins"]["tokenizers.tokenizers"]["path"], str(native))
        native.write_bytes(b"different fake native content")
        with self.assertRaises(identity.Uncacheable):
            planned.recheck()
        native.unlink()
        del self.runtime["import_files"]
        with self.assertRaisesRegex(identity.Uncacheable, "independent runtime import-file inventory"):
            self.plan()

    def test_plan_and_verify_do_not_import_any_ml_package(self):
        plan, observed, retained = self.capture()
        receipt = observed.receipt(retained)
        original = builtins.__import__
        def guarded(name, *args, **kwargs):
            if name.split(".")[0] in {"torch", "gliner", "transformers", "tokenizers", "sentencepiece", "datafog"}:
                raise AssertionError("forbidden ML import: " + name)
            return original(name, *args, **kwargs)
        with patch.object(builtins, "__import__", guarded), patch.object(identity.importlib, "import_module", side_effect=AssertionError("load")):
            current = self.plan()
            identity.verify_receipt(current, receipt)
            current.recheck()

    def test_effective_bytecode_must_match_source_under_every_header_mode(self):
        import py_compile
        source = self.sources / "gliner/config.py"
        initial = self.plan()
        cached = Path(importlib.util.cache_from_source(str(source)))
        for mode in py_compile.PycInvalidationMode:
            with self.subTest(mode=mode.name):
                py_compile.compile(str(source), doraise=True, invalidation_mode=mode)
                self.assertEqual(self.plan().sha256, initial.sha256)
                header = cached.read_bytes()[:16]
                different = compile("EFFECTIVE_BEHAVIOR='different cached program'\n", str(source), "exec")
                cached.write_bytes(header + marshal.dumps(different))
                loaded = importlib.machinery.SourceFileLoader("gliner.config", str(source)).get_code("gliner.config")
                self.assertEqual(loaded, different)
                with self.assertRaisesRegex(identity.Uncacheable, "bytecode"):
                    initial.recheck()
        py_compile.compile(str(source), doraise=True)
        valid = cached.read_bytes()
        for invalid in (b"invalid bytecode header", valid[:8] + struct.pack("<II", 0, 0) + valid[16:],
                        valid[:16] + b"invalid marshal body"):
            cached.write_bytes(invalid)
            with self.assertRaisesRegex(identity.Uncacheable, "bytecode"):
                self.plan()
        cached.write_bytes(valid)
        initial.recheck()
        source.write_bytes(source.read_bytes() + b"# source drift behind valid cache\n")
        with self.assertRaises(identity.Uncacheable):
            initial.recheck()

    def test_sourceless_unreviewed_module_and_nested_cache_refuse(self):
        import py_compile
        # This file is deliberately outside the compatibility table but within
        # a distribution whose other imports can execute during the load.
        path = self.sources / "gliner/other_module.py"
        path.write_text("BEHAVIOR='source'\n")
        initial = self.plan()
        cached = Path(py_compile.compile(str(path), doraise=True))
        valid = cached.read_bytes()
        changed = compile("BEHAVIOR='cached alternate'\n", str(path), "exec")
        cached.write_bytes(valid[:16] + marshal.dumps(changed))
        with self.assertRaisesRegex(identity.Uncacheable, "bytecode"):
            initial.recheck()
        cached.write_bytes(valid)
        initial.recheck()
        (self.sources / "gliner/loose.pyc").write_bytes(valid)
        with self.assertRaisesRegex(identity.Uncacheable, "sourceless"):
            self.plan()

    def test_bootstrap_source_live_code_and_state_are_verified(self):
        root = self.root / "bootstrap"
        root.mkdir()
        source = root / "_virtualenv.py"
        text = '''_DISTUTILS_PATCH = ("distutils.dist", "setuptools.dist")
class _Finder:
    fullname = None
    lock = []
    def find_spec(self, fullname, path, target=None):
        return None
    @staticmethod
    def exec_module(old, module):
        return old(module)
    @staticmethod
    def load_module(old, name):
        return old(name)
'''
        source.write_text(text)
        pth = root / "_virtualenv.pth"
        pth.write_text("import _virtualenv\n")
        pin = ("_virtualenv.py", "_Finder", hashlib.sha256(source.read_bytes()).hexdigest(),
               "_virtualenv.pth", hashlib.sha256(pth.read_bytes()).hexdigest())
        spec = importlib.util.spec_from_file_location("_virtualenv", source)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        hook = module._Finder()
        with patch.dict(identity.BOOTSTRAPS, {"_virtualenv": pin}), patch.dict(sys.modules, {"_virtualenv": module}), \
                patch.object(sys, "meta_path", [hook, *sys.meta_path]):
            plan = self.plan()
            source.write_text(text + "# bootstrap source drift\n")
            with self.assertRaises(identity.Uncacheable):
                plan.recheck()
            source.write_text(text)
            pth.write_text("import unknown_bootstrap\n")
            with self.assertRaises(identity.Uncacheable):
                self.plan()
            pth.write_text("import _virtualenv\n")
            original = module._Finder.find_spec
            namespace = {}
            exec(compile("def find_spec(self, fullname, path, target=None):\n return 'changed hook'\n", str(source), "exec"), namespace)
            module._Finder.find_spec = namespace["find_spec"]
            with self.assertRaises(identity.Uncacheable):
                self.plan()
            module._Finder.find_spec = original
            hook.find_spec = original
            with self.assertRaises(identity.Uncacheable):
                self.plan()
            del hook.find_spec
            module._DISTUTILS_PATCH = ("gliner",)
            with self.assertRaises(identity.Uncacheable):
                self.plan()


class OrdinaryImportContract(unittest.TestCase):
    """Tiny real packages, standard importlib, and normal cache writes only."""

    def setUp(self):
        importer, hooks = identity.importlib.import_module, [h for h in sys.meta_path if not type(h).__module__.startswith("_pytest.")]
        self.f = f = IdentityContract()
        f.setUp()
        self.addCleanup(f.doCleanups)
        f.stack.enter_context(patch.object(identity.importlib, "import_module", importer))
        f.stack.enter_context(patch.object(sys, "meta_path", hooks))
        before = set(sys.modules)
        f.stack.callback(lambda: [sys.modules.pop(name) for name in list(sys.modules)
                                 if name not in before and getattr(sys.modules[name], "__file__", None)
                                 and Path(sys.modules[name].__file__).is_relative_to(f.sources)])
        texts = {
            "torch/nn/modules/module.py": "from types import SimpleNamespace as Result\nCALLS=[]\nRETURNS=[]\nMISSING=[]\nERROR=None\n" + STATE_SOURCE,
            "transformers/models/deberta_v2/tokenization_deberta_v2.py":
                "from pathlib import Path\nSPM_FILE=Path(" + repr(str(f.nested / "spm.model")) + ")\n" + SLOW_SOURCE,
            "transformers/convert_slow_tokenizer.py":
                "from tokenizers import Tokenizer\nCALLS=[]\nERROR=None\n" + CONVERTER_SOURCE.replace("return object()", "return Tokenizer()"),
            "transformers/tokenization_utils_fast.py":
                "from .convert_slow_tokenizer import convert_slow_tokenizer\n" + FAST_SOURCE,
            "transformers/models/deberta_v2/tokenization_deberta_v2_fast.py":
                "from transformers.tokenization_utils_fast import PreTrainedTokenizerFast\n" + FAST_DEBERTA_SOURCE,
            "gliner/model.py": '''from pathlib import Path
from types import SimpleNamespace as Result
from gliner.config import UniEncoderSpanConfig
from torch.nn.modules.module import Module
from transformers.models.deberta_v2.tokenization_deberta_v2 import DebertaV2Tokenizer
from transformers.models.deberta_v2.tokenization_deberta_v2_fast import DebertaV2TokenizerFast
CALLS=[]
RETURNS=[]
STATE_RESULTS=[]
FALLBACK=False
ERROR=None
WEIGHT_FILE=Path(''' + repr(str(f.top / "pytorch_model.bin")) + ")\n" + MODEL_SOURCE,
            "datafog/engine.py": "from types import SimpleNamespace as Result\nfrom gliner.model import GLiNER\n" + ENGINE_SOURCE,
            "tokenizers/__init__.py": "from .tokenizers import Tokenizer\n",
            "tokenizers/tokenizers.py": "class Tokenizer:\n    behavior='inventoried backend'\n",
        }
        for relative, text in texts.items():
            (f.sources / relative).write_text(text)
        pins = {relative: hashlib.sha256((f.sources / relative).read_bytes()).hexdigest()
                for relative in identity.COMPATIBILITY}
        f.stack.enter_context(patch.object(identity, "COMPATIBILITY", pins))
        f.runtime["import_files"] = {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                                     for tree in identity.DEPENDENCY_TREES
                                     for path in (f.sources / tree).rglob("*.py")}

    def test_ordinary_import_cache_transition_capture_and_private_replay(self):
        self._capture_and_replay()

    def _capture_and_replay(self, preexisting=False):
        f = self.f
        before = f.plan()
        cached = Path(importlib.util.cache_from_source(str(f.sources / "gliner/model.py")))
        self.assertEqual(cached.exists(), preexisting)
        # Only this controlled fixture enables cache writes. The launcher itself
        # keeps its ordinary site/bootstrap configuration.
        with patch.object(sys, "dont_write_bytecode", False):
            with identity.observe(before) as observed:
                engine = importlib.import_module("datafog.engine")
                retained = engine._get_gliner_annotator().model
        self.assertTrue(cached.is_file())
        before.recheck()
        receipt = observed.receipt(retained)
        model = sys.modules["gliner.model"]
        state = sys.modules["torch.nn.modules.module"]
        fast = sys.modules["transformers.tokenization_utils_fast"]
        converter = sys.modules["transformers.convert_slow_tokenizer"]
        self.assertEqual(model.CALLS, [(model.GLiNER, identity.DEFAULT_MODEL, {"local_files_only": True})])
        self.assertEqual(state.CALLS[0][1:], (b"asymmetric top bytes AAA-17", False, False))
        self.assertIs(model.STATE_RESULTS[0], state.RETURNS[0])
        self.assertIs(model.RETURNS[0], retained)
        self.assertIs(fast.convert_slow_tokenizer, converter.convert_slow_tokenizer)
        self.assertIsInstance(inspect.getattr_static(model.GLiNER, "from_pretrained"), classmethod)
        self.assertEqual(retained.data_processor.transformer_tokenizer._tokenizer.behavior, "inventoried backend")
        # The original exception and descriptors survive an additional observed
        # standalone request using the already-imported ordinary modules.
        entry = inspect.getattr_static(model.GLiNER, "from_pretrained")
        state_method = state.Module.load_state_dict
        standalone = identity._plan(identity.Request("standalone", str(f.top), {}),
            source_root=f.sources, runtime_identity=f.runtime, environment=f.env)
        failure = RuntimeError("ordinary original failure")
        model.ERROR = failure
        with self.assertRaises(RuntimeError) as caught:
            with identity.observe(standalone):
                model.GLiNER.from_pretrained(str(f.top))
        self.assertIs(caught.exception, failure)
        self.assertIs(inspect.getattr_static(model.GLiNER, "from_pretrained"), entry)
        self.assertIs(state.Module.load_state_dict, state_method)
        self.assertIs(fast.convert_slow_tokenizer, converter.convert_slow_tokenizer)
        # Simulate a fresh replay process without importing/executing ML code.
        for name in list(sys.modules):
            if any(name == root or name.startswith(root + ".") for root in
                   ("gliner", "torch", "transformers", "datafog", "tokenizers")):
                sys.modules.pop(name)
        with patch.object(identity.importlib, "import_module", side_effect=AssertionError("replay import")):
            fresh = f.plan()
            self.assertEqual(fresh.sha256, before.sha256)
            identity.verify_receipt(fresh, receipt)
        self.assertTrue(cached.is_file())

    def test_preexisting_valid_cache_source_equivalence(self):
        f = self.f
        before = f.plan()
        path = f.sources / "gliner/model.py"
        loader = importlib.machinery.SourceFileLoader("gliner.model", str(path))
        with patch.object(sys, "dont_write_bytecode", False):
            code = loader.get_code("gliner.model")
        self.assertEqual(code, compile(path.read_bytes(), str(path), "exec", dont_inherit=True))
        self.assertEqual(f.plan().sha256, before.sha256)
        before.recheck()
        self._capture_and_replay(preexisting=True)


class ConstantBytecodeContract(unittest.TestCase):
    def ordinary(self):
        control = OrdinaryImportContract()
        control.setUp()
        self.addCleanup(control.doCleanups)
        return control.f

    @staticmethod
    def write_cache(path, code, flags):
        source = path.read_bytes()
        header = importlib.util.MAGIC_NUMBER + struct.pack("<I", flags)
        header += (importlib.util.source_hash(source) if flags else
                   struct.pack("<II", int(path.stat().st_mtime) & 0xffffffff, len(source)))
        cached = Path(importlib.util.cache_from_source(str(path)))
        cached.parent.mkdir(parents=True, exist_ok=True)
        cached.write_bytes(header + marshal.dumps(code))

    @staticmethod
    def change_constant(code, change):
        return code.replace(co_consts=tuple(
            ConstantBytecodeContract.change_constant(value, change)
            if type(value) is types.CodeType else change(value) for value in code.co_consts))

    def test_typed_constant_changes_refuse_plan_capture_and_private_receipt(self):
        cases = (
            ("float-zero", "repr(0.0)", float, lambda _: -0.0),
            ("complex-real-zero", "repr(0j)", complex, lambda _: complex(-0.0, 0.0)),
            ("complex-imag-zero", "repr(0j)", complex, lambda _: complex(0.0, -0.0)),
            ("nested-tuple-zero", "repr(((0.0,),))", tuple, lambda _: ((-0.0,),)),
            ("nested-tuple-type", "repr(((1,),))", tuple, lambda _: ((True,),)),
            ("frozenset-type", "sorted(type(x).__name__ for x in {1, 2, 3})",
             frozenset, lambda _: frozenset((1.0, 2.0, 3.0))),
            ("frozenset-tuple-type", "sorted(repr(x) for x in {(1,), (2,), (3,)})",
             frozenset, lambda _: frozenset(((1.0,), (2.0,), (3.0,)))),
            ("frozenset-zero", "sorted(repr(x) for x in {0.0, 2.0, 3.0})",
             frozenset, lambda _: frozenset((-0.0, 2.0, 3.0))),
        )
        for name, expression, kind, change in cases:
            for flags in (0, 1, 3):
                with self.subTest(case=name, flags=flags):
                    # Each case owns fresh modules, cache, plan and receipt.
                    with ExitStack() as cleanup:
                        control = OrdinaryImportContract()
                        control.setUp()
                        cleanup.callback(control.doCleanups)
                        f = control.f
                        path = f.sources / "tokenizers/tokenizers.py"
                        path.write_text("class Tokenizer:\n    behavior = " + expression + "\n")
                        f.runtime["import_files"][str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
                        source = compile(path.read_bytes(), str(path), "exec", dont_inherit=True)
                        alternate = self.change_constant(source, lambda c: change(c) if type(c) is kind else c)
                        # Source execution, independent of the identity comparator,
                        # defines the expected backend behavior.
                        namespace = {}
                        exec(source, namespace)
                        expected = namespace["Tokenizer"].behavior
                        before = f.plan()
                        with identity.observe(before) as observed:
                            retained = importlib.import_module("datafog.engine")._get_gliner_annotator().model
                        self.assertEqual(retained.data_processor.transformer_tokenizer._tokenizer.behavior, expected)
                        receipt = observed.receipt(retained)
                        self.write_cache(path, alternate, flags)
                        loader = importlib.machinery.SourceFileLoader("tokenizers.tokenizers", str(path))
                        actual = {}
                        exec(loader.get_code("tokenizers.tokenizers"), actual)
                        self.assertNotEqual(actual["Tokenizer"].behavior, expected)
                        # All admission boundaries must reject while the cache
                        # is present; no cache removal manufactures a pass.
                        with self.assertRaisesRegex(identity.Uncacheable, "bytecode"):
                            before.recheck()
                        with self.assertRaisesRegex(identity.Uncacheable, "bytecode"):
                            observed.receipt(retained)
                        with self.assertRaisesRegex(identity.Uncacheable, "bytecode"):
                            identity.verify_receipt(before, receipt)
                        for module in list(sys.modules):
                            if module.split(".")[0] in {"torch", "transformers", "gliner", "datafog", "tokenizers"}:
                                sys.modules.pop(module)
                        with patch.object(identity.importlib, "import_module", side_effect=AssertionError("replay import")):
                            with self.assertRaisesRegex(identity.Uncacheable, "bytecode"):
                                f.plan()
                        with self.assertRaisesRegex(identity.Uncacheable, "bytecode"):
                            with identity.observe(before):
                                self.fail("alternate cache admitted capture")

    def test_equivalent_typed_constants_and_relocated_filenames_keep_identity(self):
        f = self.ordinary()
        path = f.sources / "tokenizers/tokenizers.py"
        path.write_text('''class Tokenizer:
    values = (None, ..., True, 1, "text", b"bytes", 0.0, -0.0, 0j, -0j, ((1,),))
    members = {1, 2, 3}
    behavior = repr((values, sorted(members)))
''')
        f.runtime["import_files"][str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
        source = compile(path.read_bytes(), str(path), "exec", dont_inherit=True)
        expected = {}
        exec(source, expected)
        def relocate(code):
            return code.replace(co_filename="/synthetic/previous-location.py", co_consts=tuple(
                relocate(c) if type(c) is types.CodeType else c for c in code.co_consts))
        before = f.plan()
        for flags in (0, 1, 3):
            with self.subTest(flags=flags):
                self.write_cache(path, relocate(source), flags)
                self.assertEqual(f.plan().sha256, before.sha256)
                loader = importlib.machinery.SourceFileLoader("tokenizers.tokenizers", str(path))
                loaded = loader.get_code("tokenizers.tokenizers")
                self.assertEqual(loaded.co_filename, str(path))
                nested = next(c for c in loaded.co_consts if type(c) is types.CodeType)
                self.assertEqual(nested.co_filename, str(path))
                actual = {}
                exec(loaded, actual)
                self.assertEqual(actual["Tokenizer"].behavior, expected["Tokenizer"].behavior)
        with identity.observe(before) as observed:
            retained = importlib.import_module("datafog.engine")._get_gliner_annotator().model
        self.assertEqual(retained.data_processor.transformer_tokenizer._tokenizer.behavior, expected["Tokenizer"].behavior)
        identity.verify_receipt(before, observed.receipt(retained))

    def test_unsupported_constants_fail_closed_and_float_payload_bits_are_exact(self):
        nan = struct.unpack("!d", bytes.fromhex("7ff8000000000001"))[0]
        other_nan = struct.unpack("!d", bytes.fromhex("7ff8000000000002"))[0]
        self.assertTrue(identity._same_code(nan, nan))
        self.assertFalse(identity._same_code(nan, other_nan))
        for value in ([], {}, set(), object(), bytearray(b"x")):
            with self.subTest(kind=type(value).__name__):
                self.assertFalse(identity._same_code(value, value))


class BootstrapLifecycleContract(unittest.TestCase):
    def test_pinned_virtualenv_transition_admits_only_normal_inactive_states(self):
        control = OrdinaryImportContract()
        control.setUp()
        self.addCleanup(control.doCleanups)
        f = control.f
        installed = Path(__import__("sysconfig").get_path("purelib"))
        root = f.root / "copied-bootstrap"
        root.mkdir()
        for relative, expected in (("_virtualenv.py", identity.BOOTSTRAPS["_virtualenv"][2]),
                                   ("_virtualenv.pth", identity.BOOTSTRAPS["_virtualenv"][4])):
            data = (installed / relative).read_bytes()
            self.assertEqual(hashlib.sha256(data).hexdigest(), expected)
            (root / relative).write_bytes(data)
        spec = importlib.util.spec_from_file_location("_virtualenv", root / "_virtualenv.py")
        module = importlib.util.module_from_spec(spec)
        package = f.sources / "distutils"
        package.mkdir()
        (package / "__init__.py").write_text("")
        (package / "dist.py").write_text('''class Distribution:
    def parse_config_files(self):
        return "original parse result"
    def get_option_dict(self, name):
        return self.options
''')
        converter = f.sources / "transformers/convert_slow_tokenizer.py"
        converter.write_text("import distutils.dist\n" + converter.read_text())
        pins = dict(identity.COMPATIBILITY)
        pins["transformers/convert_slow_tokenizer.py"] = hashlib.sha256(converter.read_bytes()).hexdigest()
        standard = [importlib.machinery.BuiltinImporter, importlib.machinery.FrozenImporter,
                    importlib.machinery.PathFinder]
        with patch.object(identity, "COMPATIBILITY", pins), patch.dict(sys.modules, {"_virtualenv": module}), \
                patch.object(sys, "meta_path", standard):
            spec.loader.exec_module(module)
            hook = next(h for h in sys.meta_path if type(h) is module._Finder)
            self.assertEqual(vars(hook), {})
            self.assertEqual(module._Finder.lock, [])
            before = f.plan()
            with identity.observe(before) as observed:
                retained = importlib.import_module("datafog.engine")._get_gliner_annotator().model
            self.assertIsNotNone(retained)
            self.assertEqual(vars(hook), {"fullname": None})
            self.assertEqual(len(module._Finder.lock), 1)
            lock = module._Finder.lock[0]
            self.assertFalse(lock.locked())
            # Prove the copied hook actually patched the synthetic import.
            dist = sys.modules["distutils.dist"].Distribution()
            self.assertEqual(dist.parse_config_files.__code__.co_filename, str(root / "_virtualenv.py"))
            dist.options = {"prefix": ("synthetic", "unused"), "install_purelib": ("synthetic", "unused")}
            self.assertEqual(dist.parse_config_files(), "original parse result")
            self.assertEqual(dist.options, {"prefix": (str(root / "_virtualenv.py"), os.path.abspath(sys.prefix))})
            before.recheck()
            receipt = observed.receipt(retained)
            identity.verify_receipt(before, receipt)
            self.assertEqual(vars(hook), {"fullname": None})
            self.assertIs(module._Finder.lock[0], lock)
            # A fresh initial plan after the same normal transition also works;
            # replay executes no model/dependency package imports.
            for name in list(sys.modules):
                if name.split(".")[0] in {"torch", "transformers", "gliner", "datafog", "tokenizers"}:
                    sys.modules.pop(name)
            with patch.object(identity.importlib, "import_module", side_effect=AssertionError("replay import")):
                fresh = f.plan()
                self.assertEqual(fresh.sha256, before.sha256)
                identity.verify_receipt(fresh, receipt)
            # Deliberate negative mutations are restored only after the positive
            # lifecycle above; the hook is never reset to make capture succeed.
            for name, value in (("fullname", "distutils.dist"), ("fullname", False),
                                ("extra", None), ("find_spec", module._Finder.find_spec),
                                ("lock", [lock])):
                with self.subTest(instance_member=name, value=repr(value)):
                    with patch.object(hook, name, value, create=True):
                        with self.assertRaises(identity.Uncacheable):
                            before.recheck()
            for invalid in ([], [object()], [lock, lock], (lock,)):
                with self.subTest(class_lock=repr(invalid)), patch.object(module._Finder, "lock", invalid):
                    with self.assertRaises(identity.Uncacheable):
                        before.recheck()
            with patch.object(module._Finder, "fullname", "active"):
                with self.assertRaises(identity.Uncacheable):
                    before.recheck()
            lock.acquire()
            try:
                with self.assertRaises(identity.Uncacheable):
                    before.recheck()
            finally:
                lock.release()
            with patch.object(module._Finder, "extra", None, create=True):
                with self.assertRaises(identity.Uncacheable):
                    before.recheck()
            before.recheck()


class OrdinaryLauncherContract(unittest.TestCase):
    def test_normal_installed_launcher_with_bootstrap_hooks(self):
        script = '''import sys, unittest
hooks = [type(hook).__module__ for hook in sys.meta_path]
assert {"_virtualenv", "_distutils_hack"} <= set(hooks), hooks
print("Ordinary launcher retained bootstrap hooks:", hooks)
suite = unittest.defaultTestLoader.loadTestsFromName("test_gliner_identity.OrdinaryImportContract")
sys.exit(not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful())
'''
        result = subprocess.run([sys.executable, "-c", script],
            cwd=Path(__file__).parent, capture_output=True, text=True,
            env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        print(result.stdout.strip())


if __name__ == "__main__":
    unittest.main()
