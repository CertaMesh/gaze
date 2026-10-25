"""Conservative GLiNER identity for a future, already authorized capture.

Planning and receipt validation read local bytes only. They do not load a model.
The enclosing cache still owns full runtime/key/record-hash/private admission
verification. Endpoint drift checks assume ordinary serialized job ownership;
they do not establish host immutability. This module changes no caller yet.
"""
from __future__ import annotations

import _thread
import copy
import dis
import importlib
import importlib.machinery
import importlib.util
import inspect
import json
import os
import re
import stat
import sys
import types
import zipimport
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping

import capture_launch
from capture_launch import _same_code
from capture_identity import digest, file_digest

SCHEMA = 1
# Source compatibility, not package-version or model-name attestation.
COMPATIBILITY = {
    'gliner/model.py': 'cc1c03ed1540de4c6b836f544921e0a2de33cfae6ad4d8aeb35dc08845d867fe',
    'gliner/config.py': '8a5a521da4fe5299868b846c51cc67a904b6327a72efa1648c22df938c96587e',
    'gliner/modeling/encoder.py': '473204b1378ab462a7dbe24ea7f66f1291266b63d3d6afac9ab888b692108b7f',
    'gliner/data_processing/tokenizer.py': '2dd8c7762ae6cd8fcd8dbf062e16f5441d44814373611d7b6be3a87ed08c5d9a',
    'datafog/engine.py': 'eeb4562e84de554209f40da398bb0c476a3baf4932d0fce1c423efb8094b8145',
    'datafog/processing/text_processing/gliner_annotator.py': '092a1111caf7fea599c6e947b19404144d3e0ecf445f3881dc18761a65b43e8c',
    'presidio_analyzer/predefined_recognizers/ner/gliner_recognizer.py': '446889970c522d21cb6b21f3e55dec763d4db77d57157bacfd12d1c1c78081ea',
    'transformers/utils/hub.py': 'bc8f49e9360b904c4382c830b58b5b70d7df5ed66fd90b298bfb0edc23e2e846',
    'transformers/configuration_utils.py': '08a3889c1b8a73d340a37c93f72346ee0e385e339b7a72a03b5c026d4943baf7',
    'transformers/tokenization_utils_base.py': 'a2d62f7e5d58f908b3a2d0b24ce6329e1e52c815082cedde29a5c007a23e7e83',
    'transformers/tokenization_utils_fast.py': '558e454ee6850e90e8bc5d1782e35878dfda1da9bfdf997ae113fa7ff771a0f3',
    'transformers/models/auto/tokenization_auto.py': '5265e6def5a83c1b37871fa46ff2a59440ef80b377c249bc0c2135353157f730',
    'transformers/models/deberta_v2/tokenization_deberta_v2.py': '5a146019d1b8e3446cc84669eb257d22e74a4f41765bcb977f6ccbb93bdc758e',
    'transformers/models/deberta_v2/tokenization_deberta_v2_fast.py': 'fe2dbc8be30b2035a1a800a734a86b0dc0cea298f2dae9496f74741d8ab106d7',
    'huggingface_hub/_snapshot_download.py': '6fe37361072f92db008ab21f1902a0cd0c2ef30d12ea58414ef9c9e52eac6b0f',
    'huggingface_hub/file_download.py': '0bbe85320d5183be34d4af54a703809c5775506da4a346cbf40112da633b36d8',
    'huggingface_hub/constants.py': '9c82ec780a78aea2eefca413643a4f18e85ea5500f6a70fb6b36e8980be8be3d',
    'torch/nn/modules/module.py': '9543831476ced9a1ff8772aaed917ff5ff369c45e5a19679d95256eb22a52929',
    'transformers/convert_slow_tokenizer.py': 'f313339804780ce54c673212838b545d8b7c3ac298d2932380bc166baa22fcdf',
}

SELECTORS = (
    "HOME", "XDG_CACHE_HOME", "HF_HOME", "HF_HUB_CACHE", "HUGGINGFACE_HUB_CACHE",
    "PYTORCH_PRETRAINED_BERT_CACHE", "PYTORCH_TRANSFORMERS_CACHE", "TRANSFORMERS_CACHE",
    "HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE", "USE_FLASHDEBERTA", "TURBOT5_ATTN_TYPE",
    "PYTHONPATH", "PYTHONHOME", "PYTHONNOUSERSITE", "PYTHONSAFEPATH",
    "TIKTOKEN_CACHE_DIR", "DATA_GYM_CACHE_DIR", "TMPDIR", "TEMP", "TMP",
)
DEFAULT_MODEL = "urchade/gliner_multi_pii-v1"
FAMILY = "uni-span/external-deberta-v2/fast-sentencepiece/whitespace"
_REPO = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\Z")
_HEX = re.compile(r"[a-f0-9]{40}\Z")
_SHA = re.compile(r"[a-f0-9]{64}\Z")
# The pinned ordinary converter uses SentencePiece's protobuf module and the
# installed upb backend as well as both tokenizer extensions. No imports here.
DEPENDENCIES = ("tokenizers", "tokenizers.tokenizers", "sentencepiece",
                "sentencepiece._sentencepiece", "sentencepiece.sentencepiece_model_pb2",
                "google.protobuf", "google._upb._message")
DEPENDENCY_TREES = ("tokenizers", "sentencepiece", "google/protobuf", "google/_upb")
BOOTSTRAPS = {
    "_virtualenv": ("_virtualenv.py", "_Finder",
                    "cfb3db86aaa53bb62b5ff764970bec2d71c9228590a0ebec57f6ec926cc0bf1a",
                    "_virtualenv.pth", "69ac3d8f27e679c81b94ab30b3b56e9cd138219b1ba94a1fa3606d5a76a1433d"),
    "_distutils_hack": ("_distutils_hack/__init__.py", "DistutilsMetaFinder",
                        "ced72e54431ecf2d8d7dbd8a1e27724f1b171af3ca1503c304bf11c59a7fe861",
                        "distutils-precedence.pth", "2638ce9e2500e572a5e0de7faed6661eb569d1b696fcba07b0dd223da5f5d224"),
}


class Uncacheable(ValueError):
    """No verified identity; the existing loader's selection must stay unchanged."""


@dataclass(frozen=True)
class Request:
    entry: str
    model_id: str
    kwargs: Mapping

    @classmethod
    def datafog(cls):
        return cls("datafog", DEFAULT_MODEL, {"local_files_only": True})

    def descriptor(self):
        ordinary = {"local_files_only": True, "map_location": "cpu",
                    "load_onnx_model": False, "onnx_model_file": "model.onnx"}
        if (self.entry not in {"standalone", "tuned", "datafog"}
                or not isinstance(self.model_id, str) or not self.model_id
                or not isinstance(self.kwargs, Mapping)
                or any(key not in ordinary or type(value) is not type(ordinary[key])
                       or value != ordinary[key] for key, value in self.kwargs.items())):
            raise Uncacheable("unsupported GLiNER request or override")
        if self.entry == "datafog" and (self.model_id != DEFAULT_MODEL
                                       or dict(self.kwargs) != {"local_files_only": True}):
            raise Uncacheable("DataFog request differs from source default")
        return {"entry": self.entry, "model_id": self.model_id, "kwargs": dict(self.kwargs)}


def _absolute(value, cwd):
    path = Path(value)
    if ".." in path.parts:
        raise Uncacheable("parent traversal in lookup path is unsupported")
    return Path(os.path.abspath(path if path.is_absolute() else cwd / path))


def _check_bytecode(path):
    try:
        capture_launch._check_bytecode(path)
    except capture_launch.HarnessImportError as error:
        raise Uncacheable(str(error)) from None


def _bootstrap_context(hook):
    cls = type(hook)
    pin = BOOTSTRAPS.get(cls.__module__)
    module = sys.modules.get(cls.__module__)
    if pin is None or module is None or cls.__name__ != pin[1] or getattr(module, pin[1], None) is not cls:
        raise Uncacheable("unsupported import hook")
    path = Path(module.__file__)
    suffix = Path(pin[0])
    root = path.parents[len(suffix.parts) - 1]
    pth = root / pin[3]
    if (path != root / suffix or file_digest(path) != pin[2] or file_digest(pth) != pin[4]
            or not isinstance(module.__loader__, importlib.machinery.SourceFileLoader)
            or Path(module.__loader__.path) != path):
        raise Uncacheable("unverified bootstrap source or hook state")
    # These hooks are already loaded at normal process startup. Check their
    # live code against pinned source, without reading installed bootstrap pyc.
    compiled = compile(path.read_bytes(), str(path), "exec", dont_inherit=True)
    class_code = next(code for code in compiled.co_consts
                      if isinstance(code, types.CodeType) and code.co_name == cls.__name__)
    methods = {code.co_name: code for code in class_code.co_consts if isinstance(code, types.CodeType)}
    allowed = {"__module__", "__dict__", "__weakref__", "__doc__", *methods}
    if sys.version_info[:2] == (3, 13):
        # CPython 3.13 emits these two class attributes. Bind their compiler
        # constants rather than admitting arbitrary additional live members.
        instructions = list(dis.get_instructions(class_code))
        for index, instruction in enumerate(instructions):
            if instruction.opname == "STORE_NAME" and instruction.argval in {"__firstlineno__", "__static_attributes__"}:
                previous = instructions[index - 1]
                if previous.opname != "LOAD_CONST" or not _same_code(vars(cls).get(instruction.argval), previous.argval):
                    raise Uncacheable("unverified compiler class metadata")
                allowed.add(instruction.argval)
    if cls.__module__ == "_virtualenv":
        allowed.update(("fullname", "lock"))
        if (module._DISTUTILS_PATCH != ("distutils.dist", "setuptools.dist")
                or cls.fullname is not None or type(cls.lock) is not list):
            raise Uncacheable("unverified virtualenv hook state")
        state = vars(hook)
        pristine = not state and not cls.lock
        inactive = (set(state) == {"fullname"} and state["fullname"] is None
                    and len(cls.lock) == 1 and type(cls.lock[0]) is _thread.LockType
                    and not cls.lock[0].locked())
        if not (pristine or inactive):
            raise Uncacheable("unverified virtualenv hook state")
        static, classmethods = {"exec_module", "load_module"}, set()
    else:
        aliases = {"spec_for_test.test_distutils"}
        allowed.update(("sensitive_tests", *aliases))
        if vars(hook) or cls.sensitive_tests != ["test.test_distutils"]:
            raise Uncacheable("unverified setuptools hook state")
        for alias in aliases:
            if vars(cls).get(alias) is not vars(cls)["spec_for_sensitive_tests"]:
                raise Uncacheable("unverified setuptools hook alias")
        static, classmethods = {"is_cpython", "frame_file_is_setup"}, {"pip_imported_during_build"}
    if set(vars(cls)) != allowed or cls.__bases__ != (object,):
        raise Uncacheable("unverified bootstrap hook members")
    for name, code in methods.items():
        value = vars(cls)[name]
        descriptor = staticmethod if name in static else classmethod if name in classmethods else types.FunctionType
        if type(value) is not descriptor:
            raise Uncacheable("unverified bootstrap hook descriptor")
        function = value.__func__ if isinstance(value, (staticmethod, classmethod)) else value
        if (function.__globals__ is not vars(module) or function.__closure__ is not None
                or function.__defaults__ != ((None,) if name == "find_spec" else None)
                or function.__kwdefaults__ is not None
                or not _same_code(function.__code__, code)):
            raise Uncacheable("unverified bootstrap hook code")
    return {"path": str(path), "sha256": pin[2], "pth": str(pth), "pth_sha256": pin[4]}


def _import_context(source_root, import_files):
    """Resolve reviewed source origins without executing a package initializer."""
    if sys.implementation.name != "cpython" or sys.version_info[:2] not in {(3, 12), (3, 13)}:
        raise Uncacheable("unsupported CPython import runtime")
    machinery = importlib.machinery
    for hook in sys.path_hooks:
        if not ((hook.__module__, hook.__name__) in {
                ("zipimport", "zipimporter"),
                ("_frozen_importlib_external", "path_hook_for_FileFinder")}):
            raise Uncacheable("unsupported path import hook")
    for finder in sys.path_importer_cache.values():
        if finder is not None and not isinstance(finder, (machinery.FileFinder, zipimport.zipimporter)):
            raise Uncacheable("unsupported cached path importer")
    machinery.PathFinder.invalidate_caches()
    origins = {}
    modules = [relative[:-3].replace("/", ".") for relative in COMPATIBILITY]
    for module in [*modules, *DEPENDENCIES]:
        components = module.split(".")
        search = list(sys.path)
        for index in range(len(components)):
            name = ".".join(components[:index + 1])
            if name in origins:
                search = [str(source_root.joinpath(*components[:index + 1]))]
                continue
            # CPython's raw path search avoids _NamespacePath consulting a
            # parent in sys.modules: planning must not import google first.
            spec = machinery.PathFinder._get_spec(name, search)
            expected = source_root.joinpath(*components[:index + 1])
            if spec is None:
                raise Uncacheable("unresolved source import origin")
            if spec.origin is None and spec.submodule_search_locations is not None:
                if ({Path(path).resolve() for path in spec.submodule_search_locations}
                        != {expected.resolve()}):
                    raise Uncacheable("namespace package shadow changes source selection")
                origins[name] = {"namespace": str(expected)}
                search = list(spec.submodule_search_locations)
                continue
            if not isinstance(spec.loader, (machinery.SourceFileLoader, machinery.ExtensionFileLoader)):
                raise Uncacheable("unsupported import loader")
            if spec.submodule_search_locations is not None:
                expected = expected / "__init__.py"
            elif isinstance(spec.loader, machinery.ExtensionFileLoader):
                suffix = next((suffix for suffix in machinery.EXTENSION_SUFFIXES
                               if spec.origin.endswith(suffix)), None)
                if suffix is None:
                    raise Uncacheable("unsupported native import origin")
                expected = expected.with_name(expected.name + suffix)
            else:
                expected = expected.with_suffix(".py")
            if Path(spec.origin).resolve() != expected.resolve():
                raise Uncacheable("package shadow changes source selection")
            if isinstance(spec.loader, machinery.SourceFileLoader):
                _check_bytecode(expected)
            origins[name] = {"path": str(expected), "sha256": file_digest(expected)}
            search = list(spec.submodule_search_locations or ())
    # Membership comes from the independent full runtime inventory, never from
    # selected-module metadata or an output record. Cover Python submodules and
    # native extensions throughout these small dependency namespaces.
    if not isinstance(import_files, Mapping):
        raise Uncacheable("missing independent runtime import-file inventory")
    for relative in DEPENDENCY_TREES:
        root = source_root / relative
        for path in sorted(root.rglob("*")):
            if not path.is_file() or not (path.suffix == ".py" or
                    any(path.name.endswith(suffix) for suffix in machinery.EXTENSION_SUFFIXES)):
                continue
            actual = file_digest(path)
            if import_files.get(str(path)) != actual:
                raise Uncacheable("dependency origin absent from runtime content inventory")
            origins[str(path.relative_to(source_root))] = {"path": str(path), "sha256": actual}
    # Other modules in the same pinned distributions can be imported by their
    # reviewed entrypoints too. Runtime inventory binds their source bytes;
    # its deliberate pyc exclusion requires checking cache equivalence here.
    trees = {relative.partition("/")[0] for relative in COMPATIBILITY} | set(DEPENDENCY_TREES)
    for tree in sorted(trees):
        for path in (source_root / tree).rglob("*"):
            if path.is_symlink() and path.is_dir():
                raise Uncacheable("unsupported linked import package directory")
            if path.suffix == ".py":
                _check_bytecode(path)
            elif path.suffix == ".pyc" and path.parent.name != "__pycache__":
                raise Uncacheable("unsupported sourceless import bytecode")
    return origins


def _roots(env, cwd):
    if not env.get("HOME"):
        raise Uncacheable("missing HOME for cache default resolution")
    def expand(value):
        # Hub constants use expandvars(expanduser(value)), once per constant.
        # A tilde introduced by a variable stays literal in this pass.
        if value.startswith("~") and value != "~" and not value.startswith("~/"):
            raise Uncacheable("unresolved cache-root expansion")
        if value == "~" or value.startswith("~/"):
            value = (env["HOME"].rstrip("/") + value[1:]) or "/"
        return re.sub(r"\$(\w+|\{[^}]*\})",
                      lambda m: env.get(m[1][1:-1] if m[1].startswith("{") else m[1], m[0]),
                      value, flags=re.ASCII)
    default_home = os.path.join(env["HOME"].rstrip("/") or "/", ".cache")
    default = os.path.join(env.get("XDG_CACHE_HOME", default_home), "huggingface")
    home = expand(env.get("HF_HOME", default))
    hub = expand(env.get("HF_HUB_CACHE", env.get("HUGGINGFACE_HUB_CACHE", os.path.join(home, "hub"))))
    legacy = env.get("PYTORCH_PRETRAINED_BERT_CACHE", hub)
    legacy = env.get("PYTORCH_TRANSFORMERS_CACHE", legacy)
    legacy = env.get("TRANSFORMERS_CACHE", legacy)
    return _absolute(hub, cwd), _absolute(legacy, cwd)


def _links(path):
    """Bind aliasing in ancestors too, including a missing root's parents."""
    return [(str(part), os.readlink(part), str(part.resolve(strict=True)))
            for part in reversed((path, *path.parents)) if part.is_symlink()]


def _inventory(roots):
    allowed = tuple(path.resolve() for path in roots)
    rows = []
    def visit(path, active):
        try:
            mode = path.lstat().st_mode
        except FileNotFoundError:
            rows.append((str(path), "absent"))
            return
        if stat.S_ISLNK(mode):
            try:
                target = path.resolve(strict=True)
            except (OSError, RuntimeError) as error:
                raise Uncacheable("unresolved namespace symlink") from error
            if not any(target == root or target.is_relative_to(root) for root in allowed):
                raise Uncacheable("namespace symlink escapes relevant roots")
            rows.append((str(path), "symlink", os.readlink(path), str(target)))
            if target in active:
                raise Uncacheable("cyclic namespace symlink")
            visit(target, active)
        elif stat.S_ISDIR(mode):
            resolved = path.resolve()
            if resolved in active:
                raise Uncacheable("cyclic namespace directory")
            active = active | {resolved}
            rows.append((str(path), "directory"))
            for child in sorted(path.iterdir()):
                visit(child, active)
        elif stat.S_ISREG(mode):
            rows.append((str(path), "file", file_digest(path)))
        else:
            raise Uncacheable("unsupported namespace file kind")
    for root in sorted(set(roots)):
        rows.append((str(root), "ancestor-links", _links(root)))
        visit(root, set())
    return rows


def _within(path, roots):
    try:
        target = path.resolve(strict=True)
    except (OSError, RuntimeError) as error:
        raise Uncacheable("unresolved metadata reference") from error
    if not any(target.is_relative_to(root.resolve()) for root in roots):
        raise Uncacheable("metadata reference escapes relevant namespace")


def _json(path, roots):
    _within(path, roots)
    if not path.is_file() or path.stat().st_size > 65536:
        raise Uncacheable("missing or oversized model metadata")
    def unique(pairs):
        obj = {}
        for key, value in pairs:
            if key in obj:
                raise Uncacheable("duplicate metadata key")
            obj[key] = value
        return obj
    try:
        value = json.loads(path.read_text(), object_pairs_hook=unique,
                           parse_constant=lambda _: (_ for _ in ()).throw(Uncacheable("nonfinite metadata")))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise Uncacheable("invalid model metadata") from error
    if not isinstance(value, dict):
        raise Uncacheable("model metadata must be an object")
    return value


_TOP_FIELDS = frozenset((
    "model_name", "name", "max_width", "hidden_size", "dropout", "fine_tune",
    "subtoken_pooling", "span_mode", "post_fusion_schema", "num_post_fusion_layers",
    "vocab_size", "max_neg_type_ratio", "max_types", "max_len", "num_rnn_layers",
    "fuse_layers", "embed_ent_token", "class_token_index", "ent_token", "sep_token",
    "token_loss_coef", "span_loss_coef", "represent_spans", "neg_spans_ratio",
    "precomputed_prompts_mode", "id_to_classes", "model_type", "words_splitter_type",
    "encoder_config", "span_encoder_config", "labels_encoder", "labels_decoder",
    "relations_layer", "_attn_implementation", "transformers_version"))
_ENCODER_FIELDS = frozenset((
    "model_type", "architectures", "_name_or_path", "transformers_version",
    "attention_probs_dropout_prob", "hidden_act", "hidden_dropout_prob", "hidden_size",
    "initializer_range", "intermediate_size", "layer_norm_eps", "max_position_embeddings",
    "num_attention_heads", "num_hidden_layers", "pad_token_id", "pooler_dropout",
    "pooler_hidden_act", "pooler_hidden_size", "pos_att_type", "position_biased_input",
    "relative_attention", "type_vocab_size", "vocab_size", "max_relative_positions",
    "position_buckets", "norm_rel_ebd", "share_att_key", "conv_kernel_size",
    "conv_groups", "conv_act", "torch_dtype"))
_TOKENIZER_FIELDS = frozenset((
    "do_lower_case", "model_max_length", "tokenizer_class", "clean_up_tokenization_spaces",
    "split_by_punct", "legacy", "bos_token", "eos_token", "unk_token", "sep_token",
    "pad_token", "cls_token", "mask_token", "added_tokens_decoder"))


def _top(path, roots):
    value = _json(path / "gliner_config.json", roots)
    if set(value) - _TOP_FIELDS:
        raise Uncacheable("unknown GLiNER metadata")
    required = {"model_type": {None, "gliner", "gliner_uni_encoder_span"},
                "span_mode": {"markerV0"}, "words_splitter_type": {"whitespace"},
                "encoder_config": {None}, "span_encoder_config": {None},
                "labels_encoder": {None}, "labels_decoder": {None},
                "relations_layer": {None}, "_attn_implementation": {None}}
    defaults = {"span_mode": "markerV0", "words_splitter_type": "whitespace"}
    for key, choices in required.items():
        item = value.get(key, defaults.get(key))
        if not isinstance(item, (str, type(None))) or item not in choices:
            raise Uncacheable("unsupported GLiNER architecture/splitter/backend")
    nested = value.get("model_name", "microsoft/deberta-v3-small")
    if not isinstance(nested, str) or not _REPO.fullmatch(nested) or ".." in nested.split("/"):
        raise Uncacheable("unsupported external encoder reference")
    if any((path / name).exists() for name in
           ("tokenizer_config.json", "labels_tokenizer", "decoder_tokenizer", "adapter_config.json")):
        raise Uncacheable("unsupported local tokenizer/adapter branch")
    if not any((path / name).is_file() for name in ("model.safetensors", "pytorch_model.bin")):
        raise Uncacheable("missing GLiNER weights")
    return nested


def _nested(path, roots):
    value = _json(path / "config.json", roots)
    if set(value) - _ENCODER_FIELDS or value.get("model_type") != "deberta-v2":
        raise Uncacheable("unsupported encoder metadata")
    if "architectures" in value and value["architectures"] not in (["DebertaV2Model"], ["DebertaV2ForMaskedLM"]):
        raise Uncacheable("unsupported encoder architecture")
    value = _json(path / "tokenizer_config.json", roots)
    if set(value) - _TOKENIZER_FIELDS or value.get("tokenizer_class") not in (None, "DebertaV2Tokenizer", "DebertaV2TokenizerFast"):
        raise Uncacheable("unsupported tokenizer metadata/reference")
    if (path / "tokenizer.json").exists() or (path / "adapter_config.json").exists() or not (path / "spm.model").is_file():
        raise Uncacheable("unsupported tokenizer conversion/adapter branch")


def _repo_root(cache, repo):
    return cache / ("models--" + repo.replace("/", "--"))


def _snapshot(cache, repo):
    root = _repo_root(cache, repo)
    ref = root / "refs/main"
    if not ref.is_file() or ref.stat().st_size != 40:
        raise Uncacheable("missing ordinary offline main ref")
    _within(ref, (root,))
    revision = ref.read_text()
    if not _HEX.fullmatch(revision) or not (root / "snapshots" / revision).is_dir():
        raise Uncacheable("unresolved offline snapshot")
    return root / "snapshots" / revision


@dataclass(frozen=True)
class Plan:
    _descriptor_json: str
    _read: object
    source_root: Path

    @property
    def descriptor(self):
        return json.loads(self._descriptor_json)

    @property
    def sha256(self):
        return digest(self.descriptor)

    def recheck(self):
        try:
            current = self._read()
        except (OSError, RuntimeError) as error:
            raise Uncacheable("unreadable GLiNER inputs during drift check") from error
        if current.descriptor != self.descriptor:
            raise Uncacheable("GLiNER source/context/namespace drift")


def plan(request: Request, *, source_root: Path, runtime_identity: Mapping,
         environment: Mapping[str, str] | None = None) -> Plan:
    """Derive expected roots from launch inputs, never from an observation record.

    runtime_identity must be the current independently measured full interpreter,
    package/native-content identity already owned by benchmark_identity.runtime.
    Its import_files map (absolute path -> SHA-256) must be collected from that
    SAME installed-content pass for DEPENDENCY_TREES, not from this planner's
    selected origins or from a receipt. Existing callers do not yet supply it.
    This bounded helper neither replaces that inventory nor authenticates a record.
    Call before ML imports in the fresh collector process.
    """
    packages = ({name.partition("/")[0] for name in COMPATIBILITY}
                | {name.partition(".")[0] for name in DEPENDENCIES})
    if any(name.partition(".")[0] in packages for name in sys.modules):
        raise Uncacheable("plan requires a fresh process before ML imports")
    try:
        return _plan(request, source_root=source_root, runtime_identity=runtime_identity,
                     environment=environment)
    except (OSError, RuntimeError) as error:
        raise Uncacheable("unreadable GLiNER identity inputs") from error


def _plan(request, *, source_root, runtime_identity, environment):
    env = os.environ if environment is None else environment
    cwd = Path.cwd()
    request_value = request.descriptor()
    offline = env.get("HF_HUB_OFFLINE") or env.get("TRANSFORMERS_OFFLINE") or ""
    if offline.upper() not in {"1", "ON", "YES", "TRUE"}:
        raise Uncacheable("online resolution is unsupported")
    if env.get("USE_FLASHDEBERTA") or env.get("TURBOT5_ATTN_TYPE"):
        raise Uncacheable("optional accelerated backend is unsupported")
    if any(not isinstance(runtime_identity.get(key), str) or not _SHA.fullmatch(runtime_identity[key])
           for key in ("interpreter_sha256", "packages_sha256")):
        raise Uncacheable("missing full runtime content identity")
    source_root = _absolute(source_root, cwd)
    sources = {}
    for name, expected in COMPATIBILITY.items():
        path = source_root / name
        if not path.is_file() or file_digest(path) != expected:
            raise Uncacheable("unsupported source compatibility")
        sources[name] = expected
    hub, transformers = _roots(env, cwd)
    top_shadow = _absolute(request.model_id, cwd)
    repo = request.model_id if _REPO.fullmatch(request.model_id) else None
    if top_shadow.exists():
        top = top_shadow
        # Local snapshot requests still bind their complete enclosing repo.
        if repo is None:
            for parent in top.parents:
                if parent.name.startswith("models--"):
                    candidate = parent.name[8:].replace("--", "/")
                    if _REPO.fullmatch(candidate):
                        repo = candidate
                    break
    elif repo:
        top = _snapshot(hub, repo)
    else:
        raise Uncacheable("missing selected local GLiNER tree")
    roots = {top_shadow}
    if repo:
        roots.update(_repo_root(cache, repo) for cache in (hub, transformers))
    nested = _top(top, roots)
    nested_shadow = cwd / nested
    roots.add(nested_shadow)
    roots.update(_repo_root(cache, nested) for cache in (hub, transformers))
    # Validate metadata at every present snapshot: resolver fallbacks must not
    # introduce a different family or external file-reference namespace.
    for cache in {hub, transformers}:
        root = _repo_root(cache, nested)
        if root.exists():
            for snapshot in (root / "snapshots").iterdir():
                _nested(snapshot, roots)
    if nested_shadow.exists():
        _nested(nested_shadow, roots)
    else:
        _nested(_snapshot(transformers, nested), roots)
    namespace = _inventory(roots)
    selectors = {key: {"set": key in env, "value": env.get(key)} for key in SELECTORS}
    # All expansion variables and import/backend selectors are additionally bound.
    context = {"cwd": str(cwd), "selectors": selectors, "environment_sha256": digest(dict(env)),
               "python_optimization": sys.flags.optimize,
               "sys_path": list(sys.path), "meta_path": [type(hook).__module__ + "." + type(hook).__qualname__
                                                        if not isinstance(hook, type) else hook.__module__ + "." + hook.__qualname__
                                                        for hook in sys.meta_path]}
    supported_hooks = {"_frozen_importlib.BuiltinImporter", "_frozen_importlib.FrozenImporter",
                       "_frozen_importlib_external.PathFinder"}
    context["bootstrap_hooks"] = [_bootstrap_context(hook) for hook, name in zip(sys.meta_path, context["meta_path"])
                                  if name not in supported_hooks]
    context["import_origins"] = _import_context(source_root, runtime_identity.get("import_files"))
    descriptor = {"schema": SCHEMA, "request": request_value, "family": FAMILY,
                  "sources": sources, "source_root": str(source_root), "encoder_repo": nested,
                  "observer_sha256": file_digest(Path(__file__)), "runtime": copy.deepcopy(dict(runtime_identity)),
                  "context": context, "namespace_sha256": digest(namespace)}
    # Capture references to current launch inputs; rechecks re-read all bytes.
    read = lambda: _plan(request, source_root=source_root, runtime_identity=runtime_identity, environment=env)
    return Plan(json.dumps(descriptor, sort_keys=True, allow_nan=False), read, source_root)


def verify_receipt(expected: Plan, receipt: Mapping) -> str:
    """Validate private proof structure under an independently reconstructed plan.

    The enclosing cache must also verify the original full key and record hash.
    A public digest or caller's boolean never substitutes for these observations.
    """
    required = {"schema": SCHEMA, "plan_sha256": expected.sha256,
                "entry": {"calls": 1, "returns": 1, "request_sha256": digest(expected.descriptor["request"])},
                "state": {"calls": 1, "returns": 1, "strict": False, "missing_keys": []},
                "conversion": {"calls": 1, "returns": 1, "from_tiktoken": False,
                               "slow_class": "DebertaV2Tokenizer", "retained_backend": True},
                "retained": {"model_class": "UniEncoderSpanGLiNER", "tokenizer_class": "DebertaV2TokenizerFast",
                             "same_returned_object": True}, "violations": []}
    # Canonical JSON comparison also distinguishes bools from integer counts.
    if not isinstance(receipt, Mapping) or digest(dict(receipt)) != digest(required):
        raise Uncacheable("missing or invalid private GLiNER capture proof")
    expected.recheck()
    return digest(required)


class Capture:
    """Three temporary observers around an existing initialization region.

    This never initializes, warms, clears, reloads or predicts. receipt() takes
    the actual object retained by the caller, after the context has restored all
    descriptors. Importing the ML modules happens only inside __enter__.
    """

    def __init__(self, expected: Plan):
        self.plan = expected
        self.violations = []
        self._restores = []
        self._entered = False
        self._closed = False
        self._active = False
        self._entries = []
        self._states = []
        self._conversions = []

    def _module(self, name):
        module = importlib.import_module(name)
        path = self.plan.source_root / (name.replace(".", "/") + ".py")
        if Path(module.__file__).resolve() != path.resolve() or file_digest(path) != self.plan.descriptor["sources"].get(path.relative_to(self.plan.source_root).as_posix()):
            raise Uncacheable("imported module origin differs from planned source")
        return module

    def _replace(self, owner, name, replacement):
        original = inspect.getattr_static(owner, name)
        self._restores.append((owner, name, original))
        setattr(owner, name, replacement)

    def __enter__(self):
        if self._entered:
            raise Uncacheable("capture context cannot be reused")
        self._entered = True
        self.plan.recheck()
        try:
            gliner = self._module("gliner.model")
            configs = self._module("gliner.config")
            torch = self._module("torch.nn.modules.module")
            fast = self._module("transformers.tokenization_utils_fast")
            slow = self._module("transformers.models.deberta_v2.tokenization_deberta_v2")
            fast_deberta = self._module("transformers.models.deberta_v2.tokenization_deberta_v2_fast")
            if self.plan.descriptor["request"]["entry"] == "datafog":
                engine = self._module("datafog.engine")
                if engine._get_gliner_annotator.cache_info().currsize:
                    self.violations.append("preloaded DataFog annotator")
            entry_class = gliner.GLiNER
            descriptor = inspect.getattr_static(entry_class, "from_pretrained")
            if not isinstance(descriptor, classmethod):
                raise Uncacheable("unexpected entry descriptor")
            original_entry = descriptor.__func__
            original_state = inspect.getattr_static(torch.Module, "load_state_dict")
            converter = self._module("transformers.convert_slow_tokenizer")
            original_convert = fast.convert_slow_tokenizer
            if original_convert is not converter.convert_slow_tokenizer:
                raise Uncacheable("fast-module converter alias is not the original converter")
            load_code = gliner.BaseGLiNER.from_pretrained.__func__.__code__
            conversion_code = fast.PreTrainedTokenizerFast.__init__.__code__
            for function, filename in ((original_entry, "gliner/model.py"), (original_state, "torch/nn/modules/module.py"),
                                       (original_convert, "transformers/convert_slow_tokenizer.py"),
                                       (gliner.BaseGLiNER.from_pretrained.__func__, "gliner/model.py"),
                                       (fast.PreTrainedTokenizerFast.__init__, "transformers/tokenization_utils_fast.py")):
                if Path(function.__code__.co_filename).resolve() != (self.plan.source_root / filename).resolve():
                    raise Uncacheable("observer target is not source-bound")
            self._config_types = (configs.GLiNERConfig, configs.UniEncoderSpanConfig)
            self._encoder_config_type = configs.DebertaV2Config
            self._model_type = gliner.UniEncoderSpanGLiNER
            self._tokenizer_type = fast_deberta.DebertaV2TokenizerFast

            def entry(cls, *args, **kwargs):
                if self._active:
                    self.violations.append("recursive entry")
                self._entries.append(None)
                try:
                    bound = inspect.signature(original_entry).bind(cls, *args, **kwargs)
                    values = dict(bound.arguments)
                    values.pop(next(iter(values)))
                    model_id = values.pop("model_id")
                    extras = values.pop("model_kwargs", {})
                    values.update(extras)
                    wanted = self.plan.descriptor["request"]
                    if model_id != wanted["model_id"] or values != wanted["kwargs"]:
                        self.violations.append("entry request mismatch")
                except Exception:
                    self.violations.append("unreadable entry request")
                self._active = True
                try:
                    result = original_entry(cls, *args, **kwargs)
                    self._entries[-1] = result
                    return result
                except BaseException:
                    self.violations.append("entry failed")
                    raise
                finally:
                    self._active = False

            def state(module, *args, **kwargs):
                caller = sys._getframe(1)
                relevant = self._active and caller.f_code is load_code
                if relevant:
                    instance = caller.f_locals.get("instance")
                    try:
                        bound = inspect.signature(original_state).bind(module, *args, **kwargs)
                        bound.apply_defaults()
                        if (instance is None or instance.model is not module
                                or bound.arguments.get("strict") is not False
                                or bound.arguments.get("assign", False) is not False):
                            self.violations.append("unexpected top state load")
                    except Exception:
                        self.violations.append("unreadable state load")
                try:
                    result = original_state(module, *args, **kwargs)
                except BaseException:
                    if relevant:
                        self.violations.append("state load failed")
                    raise
                if relevant:
                    try:
                        if list(result.missing_keys):
                            self.violations.append("missing parameter keys")
                        self._states.append(module)
                    except Exception:
                        self.violations.append("missing state incompatibility result")
                return result

            def convert(*args, **kwargs):
                relevant = self._active
                if relevant:
                    try:
                        bound = inspect.signature(original_convert).bind(*args, **kwargs)
                        bound.apply_defaults()
                        if (bound.arguments.get("from_tiktoken") is not False
                                or type(bound.arguments.get("transformer_tokenizer")) is not slow.DebertaV2Tokenizer
                                or sys._getframe(1).f_code is not conversion_code):
                            self.violations.append("unsupported tokenizer conversion")
                    except Exception:
                        self.violations.append("unreadable conversion")
                try:
                    result = original_convert(*args, **kwargs)
                except BaseException:
                    if relevant:
                        self.violations.append("conversion failed")
                    raise
                if relevant:
                    self._conversions.append(result)
                return result

            self._replace(entry_class, "from_pretrained", classmethod(entry))
            self._replace(torch.Module, "load_state_dict", state)
            self._replace(fast, "convert_slow_tokenizer", convert)
            self._targets = ((entry_class, "from_pretrained", inspect.getattr_static(entry_class, "from_pretrained")),
                             (torch.Module, "load_state_dict", state), (fast, "convert_slow_tokenizer", convert))
            return self
        except BaseException:
            self._restore()
            raise

    def _restore(self):
        for owner, name, value in reversed(self._restores):
            setattr(owner, name, value)
        self._restores.clear()
        self._closed = True

    def __exit__(self, kind, value, traceback):
        try:
            for owner, name, wrapper in self._targets:
                if inspect.getattr_static(owner, name, None) is not wrapper:
                    self.violations.append("observer replaced during capture")
            if kind is not None:
                self.violations.append("capture failed")
        finally:
            self._restore()
        return False

    def receipt(self, retained):
        if not self._closed or self.violations:
            raise Uncacheable("incomplete capture: " + ", ".join(self.violations))
        if (len(self._entries) != 1 or self._entries[0] is not retained
                or type(retained) is not self._model_type
                or len(self._states) != 1 or self._states[0] is not getattr(retained, "model", None)
                or len(self._conversions) != 1):
            raise Uncacheable("unobserved or different retained GLiNER object")
        tokenizer = getattr(getattr(retained, "data_processor", None), "transformer_tokenizer", None)
        if type(tokenizer) is not self._tokenizer_type or getattr(tokenizer, "_tokenizer", None) is not self._conversions[0]:
            raise Uncacheable("conversion backend differs from retained tokenizer")
        config = getattr(retained, "config", None)
        if (type(config) not in self._config_types or type(getattr(config, "encoder_config", None)) is not self._encoder_config_type
                or config.model_name != self.plan.descriptor["encoder_repo"]
                or config.model_type != "gliner_uni_encoder_span" or config.words_splitter_type != "whitespace"
                or config.encoder_config.model_type != "deberta-v2"):
            raise Uncacheable("returned configuration differs from supported family")
        receipt = {"schema": SCHEMA, "plan_sha256": self.plan.sha256,
                   "entry": {"calls": 1, "returns": 1, "request_sha256": digest(self.plan.descriptor["request"])},
                   "state": {"calls": 1, "returns": 1, "strict": False, "missing_keys": []},
                   "conversion": {"calls": 1, "returns": 1, "from_tiktoken": False,
                                  "slow_class": "DebertaV2Tokenizer", "retained_backend": True},
                   "retained": {"model_class": "UniEncoderSpanGLiNER", "tokenizer_class": "DebertaV2TokenizerFast",
                                "same_returned_object": True}, "violations": []}
        verify_receipt(self.plan, receipt)
        return receipt


def observe(expected: Plan) -> Capture:
    return Capture(expected)
