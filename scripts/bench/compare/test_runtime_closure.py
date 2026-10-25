"""Installed membership and model file selectors come from independent inputs."""
import importlib.machinery
import json
from pathlib import Path
import sys
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
