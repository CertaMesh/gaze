"""Separate shells share captures only through the actual isolated run inputs."""
import json
import os
from pathlib import Path
import subprocess
import sys

import pytest
import capture_launch


@pytest.fixture
def isolated_capture(tmp_path):
    here = Path(capture_launch.__file__).resolve().parent
    script = tmp_path / "synthetic_capture.py"
    script.write_text('''
import argparse, json, os, pathlib, sys
from types import SimpleNamespace
launch = pathlib.Path(sys.argv[1]) / "capture_launch.py"
scope = {"__file__": str(launch)}
exec(compile(launch.read_bytes(), str(launch), "exec"), scope)
scope["initialize"](script=True)
parser = argparse.ArgumentParser()
parser.add_argument("here", type=pathlib.Path)
parser.add_argument("cache", type=pathlib.Path)
args = scope["fixed_arguments"](parser.parse_args())
import subject_identity as identity
import document_capture as captures
import benchmark_spec
from capture_identity import digest
# Keep the installed-content inventory synthetic; exercise the real runtime
# environment/cwd fields, subject construction, document keys and disk reuse.
identity.importlib.metadata.distributions = lambda: []
identity.runtime_closure.import_inventory = lambda *a, **kw: "a" * 64
identity.content.native_runtime = lambda *a: {}
spec = benchmark_spec.manifest({"corpus_sha256": "a" * 64, "generator": None, "seed": 1,
    "contracts": {"v1": "all"}, "harness": {"driver": "b" * 64}, "record_schema": [1],
    "protocol": {"mode": "synthetic"}, "population": {"C": {"documents": 1}}})
read = lambda: identity.subject(name="synthetic", config={}, models={},
    runtime_identity=identity.runtime(), adapter=pathlib.Path(__file__))
session = captures.Session(args.cache, spec, read, {})
document = SimpleNamespace(uid="synthetic", text="synthetic", language="en", region="")
def predictor(document):
    assert "GAZE_SYNTHETIC_OUTSIDE" not in os.environ
    return []
session.capture_payload(document, predictor, lambda *a: None)
session.finish()
print(json.dumps({"key": digest(session.keys), "hits": session.hits, "misses": session.misses,
    "cwd": str(pathlib.Path.cwd()), "cache": str(args.cache),
    "outside_present": "GAZE_SYNTHETIC_OUTSIDE" in os.environ,
    "environment_sha256": session.subject["runtime"]["environment_sha256"],
    "optimization": sys.flags.optimize, "prefix": sys.pycache_prefix}))
''')
    cache = tmp_path / "captures"
    cache.mkdir()
    callers = [tmp_path / "shell-a", tmp_path / "shell-b"]
    for caller in callers:
        caller.mkdir()
    environment = {"PATH": os.defpath, "HOME": str(tmp_path), "LANG": "C",
                   "PYTHONHASHSEED": "0", "PYTHONDONTWRITEBYTECODE": "1"}

    def run(shell, **extra):
        env = {**environment, "TERM_SESSION_ID": f"shell-{shell}",
               "SSH_AUTH_SOCK": f"/synthetic/socket-{shell}", **extra}
        command = [sys.executable, "-B", "-O", "-X", f"pycache_prefix={tmp_path / 'pycache'}",
                   str(script), str(here), "../captures"]
        result = subprocess.run(command, cwd=callers[shell], env=env, capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)
    return run, cache


def test_capture_reuses_across_shell_metadata_and_caller_directories(isolated_capture):
    run, cache = isolated_capture
    first, second = run(0), run(1)
    assert first["misses"] == 1 and first["hits"] == 0
    assert second["hits"] == 1 and second["misses"] == 0
    assert first["key"] == second["key"]
    assert first["environment_sha256"] == second["environment_sha256"]
    assert first["cwd"] == second["cwd"] == str(Path(capture_launch.__file__).resolve().parents[3])
    assert first["cache"] == second["cache"] == str(cache)
    assert second["optimization"] == 1
    assert second["prefix"].endswith("/pycache")


@pytest.mark.parametrize("name,value", [("LANG", "POSIX"), ("OMP_NUM_THREADS", "2"),
                                        ("PYTHONUTF8", "1"), ("HF_HUB_OFFLINE", "1")])
def test_allowlisted_environment_change_invalidates_capture(isolated_capture, name, value):
    run, _ = isolated_capture
    first, second = run(0), run(1, **{name: value})
    assert first["key"] != second["key"]
    assert first["environment_sha256"] != second["environment_sha256"]
    assert second["misses"] == 1 and second["hits"] == 0


def test_non_allowlisted_environment_cannot_reach_capture_predictor(isolated_capture):
    run, _ = isolated_capture
    first = run(0, GAZE_SYNTHETIC_OUTSIDE="synthetic")
    second = run(1, GAZE_SYNTHETIC_OUTSIDE="changed")
    assert not first["outside_present"] and not second["outside_present"]
    assert first["key"] == second["key"] and second["hits"] == 1


@pytest.mark.parametrize("entrypoint", ["capture_compare.py", "tuned_presidio/capture_produce.py",
                                        "tuned_presidio/capture_tune.py"])
def test_real_entrypoints_discard_shell_columns_before_argument_parsing(tmp_path, entrypoint):
    here = Path(capture_launch.__file__).resolve().parent
    command = [sys.executable, "-B", "-X", f"pycache_prefix={tmp_path / 'pycache'}",
               str(here / entrypoint), "--help"]
    outputs = []
    for width in (40, 200):
        result = subprocess.run(command, env={"PATH": os.defpath, "HOME": str(tmp_path),
            "LANG": "C", "COLUMNS": str(width)}, capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, result.stderr
        outputs.append(result.stdout)
    assert outputs[0] == outputs[1]
