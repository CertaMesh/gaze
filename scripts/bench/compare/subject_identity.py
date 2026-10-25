"""Private competitor identities, independent of the current Gaze implementation."""
from __future__ import annotations

import copy
import importlib.metadata
import json
import os
import sys
import sysconfig
from pathlib import Path

import benchmark_spec
import capture_identity as content
import gliner_identity
import runtime_closure

GLINER = frozenset(("gliner", "gliner-high-recall", "datafog-gliner", "tuned-presidio-gliner"))


class HashSeedError(ValueError):
    """A value-free refusal when deterministic hashing is not effective."""

    code = "ineffective_python_hash_seed"

    def __init__(self):
        super().__init__(self.code)


def require_hash_seed() -> None:
    if (os.environ.get("PYTHONHASHSEED") != "0"
            or sys.flags.hash_randomization != 0 or sys.flags.ignore_environment):
        raise HashSeedError()


def interpreter_state() -> dict:
    """Bind startup switches and mutable interpreter semantics at both endpoints."""
    process_cpu_count = getattr(os, "process_cpu_count", None)
    return {
        "flags": {name: getattr(sys.flags, name) for name in dir(sys.flags)
                  if not name.startswith("_") and isinstance(getattr(sys.flags, name), int)},
        "xoptions": dict(sys._xoptions), "warnoptions": list(sys.warnoptions),
        "int_max_str_digits": sys.get_int_max_str_digits(),
        "recursion_limit": sys.getrecursionlimit(), "switch_interval": sys.getswitchinterval(),
        "default_encoding": sys.getdefaultencoding(),
        "filesystem_encoding": sys.getfilesystemencoding(),
        "filesystem_errors": sys.getfilesystemencodeerrors(),
        "pycache_prefix": sys.pycache_prefix, "dont_write_bytecode": sys.dont_write_bytecode,
        "dlopen_flags": sys.getdlopenflags() if hasattr(sys, "getdlopenflags") else None,
        "cpu_count": os.cpu_count(), "process_cpu_count": process_cpu_count() if process_cpu_count is not None else os.cpu_count(),
        "process_cpu_count_source": "os.process_cpu_count" if process_cpu_count is not None else "os.cpu_count",
        "cpu_affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
    }


def runtime() -> dict:
    """Inventory installed bytes and lazy native dependencies without importing ML."""
    require_hash_seed()
    installed = []
    import_files = {}
    content_files = {}
    for distribution in importlib.metadata.distributions():
        if distribution.files is None:
            raise ValueError("missing installed content inventory")
        files = []
        for item in sorted(distribution.files):
            if str(item).endswith((".pyc", ".pyo")):
                continue
            path = Path(distribution.locate_file(item))
            if not path.is_file():
                raise ValueError("missing installed content")
            sha = content.file_digest(path)
            content_files[str(path.absolute())] = sha
            files.append((str(item), sha))
            if str(item).startswith(("tokenizers/", "sentencepiece/", "google/protobuf/", "google/_upb/")):
                import_files[str(path.absolute())] = sha
        direct = distribution.read_text("direct_url.json")
        if direct and json.loads(direct).get("dir_info", {}).get("editable"):
            raise ValueError("editable runtime is unsupported")
        installed.append((distribution.metadata["Name"], distribution.version, content.digest(files)))
    imports = runtime_closure.import_inventory(content_files, {
        Path(sysconfig.get_path("purelib")), Path(sysconfig.get_path("platlib"))})
    return {"interpreter_sha256": content.file_digest(Path(sys.executable)),
            "packages_sha256": content.digest(sorted(installed)), "import_files": import_files,
            "packages": [list(item) for item in sorted(installed)], "cwd": str(Path.cwd()),
            "imports_sha256": imports,
            "native": content.native_runtime({}), "sys_path": list(sys.path),
            "environment_sha256": content.digest(dict(os.environ)),
            "interpreter_state": interpreter_state()}


def plan_gliner(request, runtime_identity):
    return gliner_identity.plan(request, source_root=Path(sysconfig.get_path("purelib")),
                                runtime_identity=runtime_identity)


def model_digest(path: Path) -> str:
    """HF snapshot files may link to blobs; linked directories stay unsupported."""
    if not path.is_dir():
        raise ValueError("missing selected model")
    files = {}
    for child in sorted(path.rglob("*")):
        if child.is_symlink() and (child.is_dir() or not child.is_file()):
            raise ValueError("unsupported model directory link")
        if child.is_file():
            runtime_closure.model_references(path, child)
            files[child.relative_to(path).as_posix()] = content.file_digest(child)
    if not files:
        raise ValueError("empty selected model")
    return content.digest(files)


def subject(*, name, config, models, runtime_identity, adapter: Path, gliner_plan=None) -> dict:
    value = {"schema_version": 1, "name": name, "config": copy.deepcopy(config),
             "models": {key: model_digest(path) for key, path in sorted(models.items())},
             "runtime": copy.deepcopy(runtime_identity), "adapter_sha256": content.file_digest(adapter)}
    value["config"]["model_selectors"] = {name: str(path) for name, path in models.items()}
    if name in GLINER:
        if gliner_plan is None:
            raise ValueError("missing independent GLiNER plan")
        gliner_plan.recheck()
        if gliner_plan.descriptor["runtime"] != runtime_identity:
            raise ValueError("GLiNER runtime differs from subject")
        value["gliner"] = gliner_plan.descriptor
    elif gliner_plan is not None:
        raise ValueError("unexpected GLiNER plan")
    content.digest(value)
    return value


def verify_gliner(subject, receipt, expected):
    if subject["name"] in GLINER:
        if expected is None or subject.get("gliner") != expected.descriptor:
            raise ValueError("missing independent GLiNER computation binding")
        gliner_identity.verify_receipt(expected, receipt)
    elif receipt is not None or "gliner" in subject:
        raise ValueError("unexpected GLiNER computation binding")


def document_key(spec, subject, document) -> dict:
    """All predictor inputs are private; gold and ordering are scorer inputs."""
    return {"schema_version": 1, "benchmark_spec_id": benchmark_spec.validate(spec)["id"],
            "subject": copy.deepcopy(subject),
            "input": {"text_sha256": content.digest(document.text),
                      "utf8_bytes": len(document.text.encode("utf-8")),
                      "uid": document.uid, "language": document.language, "region": document.region}}


def public_binding(keys) -> dict:
    return {"schema_version": 1, "binding_sha256": content.digest(keys), "documents": len(keys)}


def require_population(spec, layers):
    for layer in ("C", "A", "D", "R"):
        if benchmark_spec.population(layers[layer], layer) != spec["inputs"]["population"].get(layer):
            raise ValueError("comparison population differs from benchmark specification")


def freeze_spec(path, root, dataset, read):
    """Re-read the independent measuring closure at both capture endpoints."""
    expected = json.loads(path.read_text())
    def checked():
        current = json.loads(path.read_text())
        if content.digest(current) != content.digest(expected):
            raise ValueError("benchmark specification changed during capture")
        benchmark_spec.require_current(current, root, dataset=dataset)
        return read()
    return checked
