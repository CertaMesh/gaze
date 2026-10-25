"""Installed membership and model file selectors come from independent inputs."""
import importlib.machinery
import importlib.util
import json
import marshal
from pathlib import Path
import re
import sys
import struct
from types import SimpleNamespace

import pytest
import runtime_closure
import subject_identity
from capture_identity import file_digest


@pytest.fixture
def installation(tmp_path, monkeypatch):
    root = tmp_path / "site-packages"
    root.mkdir()
    module = root / "synthetic_module.py"
    module.write_text("synthetic = True\n")
    monkeypatch.delitem(sys.modules, "sitecustomize", raising=False)
    monkeypatch.delitem(sys.modules, "usercustomize", raising=False)
    monkeypatch.setattr(sys, "path_importer_cache", {})
    return root, module, {str(module): file_digest(module)}


def test_installed_sources_and_their_origins_are_bound(installation):
    root, _, files = installation
    first = runtime_closure.import_inventory(files, [root], path=[str(root)], hooks=[])
    assert len(first) == 64
    assert runtime_closure.import_inventory(files, [root], path=[str(root)], hooks=[]) == first


def test_installed_source_inventory_checks_effective_bytecode(installation):
    root, module, files = installation
    source = module.read_bytes()
    cached = Path(importlib.util.cache_from_source(str(module)))
    cached.parent.mkdir()
    different = compile("synthetic = False\n", str(module), "exec", dont_inherit=True)
    cached.write_bytes(importlib.util.MAGIC_NUMBER + struct.pack("<I", 3)
                       + importlib.util.source_hash(source) + marshal.dumps(different))
    with pytest.raises(ValueError, match="unverified import bytecode"):
        runtime_closure.import_inventory(files, [root], path=[str(root)], hooks=[])


@pytest.mark.parametrize("damage", ["source", "sourceless", "startup", "linked_directory", "external_root", "sitecustomize"])
def test_unclosed_import_inputs_are_refused(installation, tmp_path, monkeypatch, damage):
    root, module, files = installation
    path = [str(root)]
    if damage == "source":
        (root / "unrecorded.py").write_text("synthetic = True")
    elif damage == "sourceless":
        (root / "unrecorded.pyc").write_bytes(b"synthetic bytecode")
    elif damage == "startup":
        (root / "unreviewed.pth").write_text("import synthetic_startup\n")
    elif damage == "linked_directory":
        (root / "unrecorded").symlink_to(tmp_path)
    elif damage == "external_root":
        path.append(str(tmp_path))
    else:
        monkeypatch.setitem(sys.modules, "sitecustomize", SimpleNamespace())
    with pytest.raises(ValueError):
        runtime_closure.import_inventory(files, [root], path=path, hooks=[])


@pytest.mark.parametrize("damage", ["missing", "loader", "origin", "namespace"])
def test_selected_origin_is_checked_against_recorded_membership(installation, monkeypatch, damage):
    root, module, files = installation
    spec = (None if damage == "missing" else
            SimpleNamespace(origin=None, submodule_search_locations=[]) if damage == "namespace" else
            SimpleNamespace(origin=str(module if damage == "loader" else root / "foreign.py"),
                loader=object() if damage == "loader" else importlib.machinery.SourceFileLoader("synthetic_module", str(module))))
    monkeypatch.setattr(importlib.machinery.PathFinder, "_get_spec", lambda *args: spec)
    with pytest.raises(ValueError):
        runtime_closure.import_inventory(files, [root], path=[str(root)], hooks=[])


def test_unknown_meta_hook_is_not_a_runtime_receipt(installation):
    root, _, files = installation
    with pytest.raises(ValueError):
        runtime_closure.import_inventory(files, [root], path=[str(root)], hooks=[object()])


@pytest.mark.parametrize("damage", ["path_hook", "cached_importer"])
def test_path_import_machinery_must_be_the_standard_loader(installation, monkeypatch, damage):
    root, module, files = installation
    if damage == "path_hook":
        monkeypatch.setattr(sys, "path_hooks", [lambda path: None])
    else:
        sys.path_importer_cache["synthetic"] = object()
    spec = SimpleNamespace(origin=str(module), loader=importlib.machinery.SourceFileLoader("synthetic_module", str(module)))
    monkeypatch.setattr(importlib.machinery.PathFinder, "_get_spec", lambda *args: spec)
    with pytest.raises(ValueError):
        runtime_closure.import_inventory(files, [root], path=[str(root)], hooks=[])


@pytest.mark.parametrize("selector", ["/synthetic/external.json", "../external.json", "~/external.json", "https://example.invalid/model", "missing.json", 123])
def test_model_file_references_cannot_escape_selected_bytes(tmp_path, selector):
    (tmp_path / "weights").write_text("synthetic weights")
    (tmp_path / "tokenizer_config.json").write_text(json.dumps({"tokenizer_file": selector}))
    with pytest.raises(ValueError):
        subject_identity.model_digest(tmp_path)


def test_local_model_references_recheck_target_bytes(tmp_path):
    target = tmp_path / "vocab.json"
    target.write_text("{}")
    (tmp_path / "tokenizer_config.json").write_text(json.dumps({"tokenizer_file": "vocab.json"}))
    first = subject_identity.model_digest(tmp_path)
    target.write_text('{"synthetic":1}')
    assert subject_identity.model_digest(tmp_path) != first


@pytest.mark.parametrize("selector", ['"/synthetic/model"', '"../model"', '"\\u002fsynthetic\\u002fmodel"', '"~/model"'])
def test_saved_model_config_rejects_external_and_escaped_paths(tmp_path, selector):
    (tmp_path / "config.cfg").write_text("source = " + selector + "\n")
    with pytest.raises(ValueError):
        subject_identity.model_digest(tmp_path)


# The cache prefix keeps attack bytecode out of the checkout, while CPython still
# selects it for the real source paths (including with bytecode writes disabled).
def harness_child(cache, script, *arguments):
    import subprocess
    return subprocess.run([sys.executable, "-I", "-B", "-X", f"pycache_prefix={cache}",
                           "-c", script, *map(str, arguments)], capture_output=True, text=True)


def substitute_harness_bytecode(cache, module, marker):
    result = harness_child(cache, '''
import importlib.util, marshal, pathlib, struct, sys
source = pathlib.Path(sys.argv[1])
original = source.read_bytes()
replacement = original + ("\\nfrom pathlib import Path as _AttackPath\\n"
    + "_AttackPath(" + repr(sys.argv[2]) + ").write_text('executed')\\n"
    + ("Presidio.predict = lambda self, document: []\\n" if source.name == "compare.py" else "")).encode()
cached = pathlib.Path(importlib.util.cache_from_source(str(source)))
cached.parent.mkdir(parents=True, exist_ok=True)
cached.write_bytes(importlib.util.MAGIC_NUMBER + struct.pack('<I', 3)
                   + importlib.util.source_hash(original)
                   + marshal.dumps(compile(replacement, str(source), 'exec', dont_inherit=True)))
''', module, marker)
    assert result.returncode == 0, result.stderr


@pytest.mark.parametrize("module", ["compare.py", "tuned_presidio/pool.py",
                                    "theirbench/presidio_research_repro.py", "../scorecard_record.py"])
def test_inventory_refuses_substituted_harness_bytecode_before_import(tmp_path, module):
    here = Path(runtime_closure.__file__).resolve().parent
    selected = (here / module).resolve()
    original = selected.read_bytes()
    cache, marker = tmp_path / "cache", tmp_path / "executed"
    substitute_harness_bytecode(cache, selected, marker)
    result = harness_child(cache, '''
import pathlib, sys
here = pathlib.Path(sys.argv[1])
sys.path[:0] = [str(here.parent), str(here)]
import runtime_closure
try:
    runtime_closure.import_inventory({}, set(), path=[str(here.parent), str(here)], hooks=[])
except ValueError as error:
    print(type(error).__name__ + ': ' + str(error))
    raise SystemExit(2)
''', here)
    assert selected.read_bytes() == original
    assert result.returncode == 2, result.stderr
    assert result.stdout == "HarnessImportError: unverified import bytecode\n"
    assert not marker.exists()


@pytest.mark.parametrize("entrypoint", ["capture_compare.py", "tuned_presidio/capture_produce.py",
                                        "tuned_presidio/capture_tune.py"])
@pytest.mark.parametrize("module", ["compare.py", "capture_launch.py", "gliner_identity.py"])
def test_capture_entrypoint_refuses_real_predictor_or_guard_substitution(tmp_path, entrypoint, module):
    import subprocess
    here = Path(runtime_closure.__file__).resolve().parent
    selected = here / module
    original = selected.read_bytes()
    cache, marker = tmp_path / "cache", tmp_path / "executed"
    command = [sys.executable, "-I", "-B", "-X", f"pycache_prefix={cache}",
               str(here / entrypoint), "--help"]
    clean = subprocess.run(command, capture_output=True, text=True)
    assert clean.returncode == 0, clean.stderr
    substitute_harness_bytecode(cache, selected, marker)
    refused = subprocess.run(command, capture_output=True, text=True)
    assert selected.read_bytes() == original
    assert refused.returncode != 0
    assert "HarnessImportError: unverified import bytecode" in re.sub(r"\x1b\[[0-9;]*m", "", refused.stderr)
    assert not marker.exists(), "substituted harness code executed before refusal"


@pytest.mark.parametrize("damage", ["nested_bytecode", "sourceless", "linked_directory"])
def test_harness_walk_covers_nested_imports_and_refuses_unclosed_roots(tmp_path, monkeypatch, damage):
    import capture_launch
    bench = tmp_path / "bench"
    here = bench / "compare"
    here.mkdir(parents=True)
    monkeypatch.setattr(capture_launch, "__file__", str(here / "capture_launch.py"))
    if damage == "linked_directory":
        (bench / "nested").symlink_to(here, target_is_directory=True)
    elif damage == "sourceless":
        (here / "orphan.pyc").write_bytes(b"synthetic")
    else:
        nested = here / "nested_package"
        nested.mkdir()
        module = nested / "predictor.py"
        module.write_text("synthetic = True\n")
        cached = Path(importlib.util.cache_from_source(str(module)))
        cached.parent.mkdir()
        cached.write_bytes(importlib.util.MAGIC_NUMBER + struct.pack("<I", 3)
                           + importlib.util.source_hash(module.read_bytes())
                           + marshal.dumps(compile("synthetic = False\n", str(module), "exec")))
    with pytest.raises(capture_launch.HarnessImportError):
        capture_launch.check_harness()


@pytest.mark.parametrize("admission", ["initialize", "inventory"])
@pytest.mark.parametrize("prefix_source", ["flag", "environment"])
def test_stdlib_substitution_under_effective_prefix_is_refused(tmp_path, admission, prefix_source):
    import os
    import subprocess
    here = Path(runtime_closure.__file__).resolve().parent
    selected = Path(runtime_closure.sysconfig.get_path("stdlib")) / "colorsys.py"
    original = selected.read_bytes()
    cache, marker = tmp_path / "cache", tmp_path / "executed"
    substitute_harness_bytecode(cache, selected, marker)
    script = '''
import pathlib, sys
here = pathlib.Path(sys.argv[1])
sys.path[:0] = [str(here.parent), str(here)]
import capture_launch
try:
    if sys.argv[2] == 'initialize':
        capture_launch.initialize()
    else:
        import runtime_closure
        runtime_closure.import_inventory({}, set(), path=[str(here)], hooks=[])
except capture_launch.HarnessImportError as error:
    print(type(error).__name__ + ': ' + str(error))
    raise SystemExit(2)
import colorsys
'''
    command = [sys.executable, "-B"]
    environment = dict(os.environ)
    environment.pop("PYTHONPATH", None)
    if prefix_source == "flag":
        command += ["-I", "-X", f"pycache_prefix={cache}"]
    else:
        environment["PYTHONPYCACHEPREFIX"] = str(cache)
    result = subprocess.run([*command, "-c", script, str(here), admission],
                            env=environment, capture_output=True, text=True)
    assert selected.read_bytes() == original
    assert result.returncode == 2, result.stderr
    assert result.stdout == "HarnessImportError: unverified import bytecode\n"
    assert not marker.exists()


def test_import_identity_binds_effective_pycache_prefix(installation, tmp_path, monkeypatch):
    root, _, files = installation
    first = runtime_closure.import_inventory(files, [root], path=[str(root)], hooks=[])
    monkeypatch.setattr(sys, "pycache_prefix", str(tmp_path / "cache"))
    assert runtime_closure.import_inventory(files, [root], path=[str(root)], hooks=[]) != first


def test_uv_minor_version_stdlib_alias_is_admitted_without_extra_roots(installation, tmp_path, monkeypatch):
    root, _, files = installation
    standard = tmp_path / "cpython-3.13.13" / "lib" / "python3.13"
    dynamic = standard / "lib-dynload"
    dynamic.mkdir(parents=True)
    alias = tmp_path / "cpython-3.13"
    alias.symlink_to(standard.parents[1], target_is_directory=True)
    paths = {"stdlib": str(standard), "platstdlib": str(standard)}
    monkeypatch.setattr(runtime_closure.sysconfig, "get_path", paths.__getitem__)
    direct = runtime_closure.import_inventory(files, [root], path=[str(root), str(dynamic)], hooks=[])
    linked = alias / "lib" / "python3.13" / "lib-dynload"
    assert runtime_closure.import_inventory(files, [root], path=[str(root), str(linked)], hooks=[]) == direct
    unrelated = tmp_path / "unrelated"
    unrelated.mkdir()
    with pytest.raises(ValueError, match="external Python import root"):
        runtime_closure.import_inventory(files, [root], path=[str(root), str(unrelated)], hooks=[])


@pytest.mark.parametrize("entrypoint", ["capture_compare.py", "tuned_presidio/capture_produce.py",
                                        "tuned_presidio/capture_tune.py"])
def test_capture_entrypoints_require_script_path_launch(tmp_path, entrypoint):
    import subprocess
    here = Path(runtime_closure.__file__).resolve().parent
    cache = tmp_path / "cache"
    options = [sys.executable, "-B", "-X", f"pycache_prefix={cache}"]
    script = subprocess.run([*options, str(here / entrypoint), "--help"], capture_output=True, text=True)
    assert script.returncode == 0, script.stderr
    module = entrypoint.removesuffix(".py").replace("/", ".")
    launched = subprocess.run([*options, "-m", module, "--help"], cwd=here, capture_output=True, text=True)
    assert launched.returncode != 0
    assert "HarnessImportError: script-path launch only" in re.sub(r"\x1b\[[0-9;]*m", "", launched.stderr)


@pytest.mark.parametrize("damage", ["nested_bytecode", "sourceless", "linked_directory"])
@pytest.mark.parametrize("parent", ["synthetic_package", "__pycache__"])
def test_stdlib_walk_checks_lazy_nested_imports(tmp_path, monkeypatch, damage, parent):
    import capture_launch
    standard = tmp_path / "stdlib"
    nested = standard / parent
    nested.mkdir(parents=True)
    monkeypatch.setattr(capture_launch.sysconfig, "get_path", lambda name: str(standard))
    if damage == "linked_directory":
        (standard / "linked").symlink_to(nested, target_is_directory=True)
    elif damage == "sourceless":
        (nested / "orphan.pyc").write_bytes(b"synthetic")
    else:
        source = nested / "lazy_module.py"
        source.write_text("synthetic = True\n")
        cached = Path(importlib.util.cache_from_source(str(source)))
        cached.parent.mkdir()
        cached.write_bytes(importlib.util.MAGIC_NUMBER + struct.pack("<I", 3)
                           + importlib.util.source_hash(source.read_bytes())
                           + marshal.dumps(compile("synthetic = False\n", str(source), "exec")))
    with pytest.raises(capture_launch.HarnessImportError):
        capture_launch.check_stdlib()


def test_stdlib_frozen_and_builtin_modules_do_not_select_source_caches(tmp_path, monkeypatch):
    import capture_launch
    monkeypatch.setattr(capture_launch.sysconfig, "get_path", lambda name: str(tmp_path))
    monkeypatch.setattr(capture_launch._imp, "is_frozen", lambda name: name == "synthetic_frozen")
    monkeypatch.setattr(sys, "builtin_module_names", ("synthetic_builtin",))
    for name in ("synthetic_frozen", "synthetic_builtin"):
        source = tmp_path / (name + ".py")
        source.write_text("synthetic = True\n")
        cached = Path(importlib.util.cache_from_source(str(source)))
        cached.parent.mkdir(exist_ok=True)
        cached.write_bytes(b"unused cache")
    capture_launch.check_stdlib()


@pytest.mark.parametrize("import_root", ["stdlib", "lib-dynload"])
def test_stdlib_prefix_validation_uses_each_admitted_alias(tmp_path, monkeypatch, import_root):
    import capture_launch
    standard = tmp_path / "stdlib"
    standard.mkdir()
    selected = standard if import_root == "stdlib" else standard / "lib-dynload"
    selected.mkdir(exist_ok=True)
    (selected / "lazy_module.py").write_text("synthetic = True\n")
    alias = tmp_path / "stdlib_alias"
    alias.symlink_to(selected, target_is_directory=True)
    monkeypatch.setattr(capture_launch.sysconfig, "get_path", lambda name: str(standard))
    monkeypatch.setattr(sys, "path", [str(alias)])
    monkeypatch.setattr(sys, "pycache_prefix", str(tmp_path / "cache"))
    source = alias / "lazy_module.py"
    cached = Path(importlib.util.cache_from_source(str(source)))
    cached.parent.mkdir(parents=True)
    cached.write_bytes(importlib.util.MAGIC_NUMBER + struct.pack("<I", 3)
                       + importlib.util.source_hash(source.read_bytes())
                       + marshal.dumps(compile("synthetic = False\n", str(source), "exec")))
    with pytest.raises(capture_launch.HarnessImportError, match="unverified import bytecode"):
        capture_launch.check_stdlib()


@pytest.mark.parametrize("root_name", ["purelib", "platlib"])
def test_installed_prefix_validation_uses_each_admitted_alias(installation, tmp_path, monkeypatch, root_name):
    root, module, files = installation
    alias = tmp_path / root_name
    alias.symlink_to(root, target_is_directory=True)
    source = alias / module.name
    files[str(source)] = file_digest(source)
    monkeypatch.setattr(sys, "pycache_prefix", str(tmp_path / "cache"))
    cached = Path(importlib.util.cache_from_source(str(source)))
    cached.parent.mkdir(parents=True)
    cached.write_bytes(importlib.util.MAGIC_NUMBER + struct.pack("<I", 3)
                       + importlib.util.source_hash(source.read_bytes())
                       + marshal.dumps(compile("synthetic = False\n", str(source), "exec")))
    with pytest.raises(ValueError, match="unverified import bytecode"):
        runtime_closure.import_inventory(files, [root], path=[str(alias), str(root)], hooks=[])
