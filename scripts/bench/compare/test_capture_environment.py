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
parser.add_argument("--tuning-guard", action="store_true")
args = scope["fixed_arguments"](parser.parse_args())
import subject_identity as identity
import document_capture as captures
import benchmark_spec
if args.tuning_guard:
    import tune
    tune.require_hash_seed()
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

    def run(shell, *, flags=(), tuning_guard=False, refusal=False, **extra):
        env = {**environment, "TERM_SESSION_ID": f"shell-{shell}",
               "SSH_AUTH_SOCK": f"/synthetic/socket-{shell}", **extra}
        command = [sys.executable, "-B", "-O", "-X", f"pycache_prefix={tmp_path / 'pycache'}",
                   *flags, str(script), str(here), "../captures"]
        if tuning_guard:
            command.append("--tuning-guard")
        result = subprocess.run(command, cwd=callers[shell], env=env, capture_output=True, text=True, timeout=30)
        if refusal:
            return result
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


@pytest.mark.parametrize("flag", ["-R", "-E", "-I"])
@pytest.mark.parametrize("tuning_guard", [False, True])
def test_randomized_or_environment_ignoring_interpreters_refuse(isolated_capture, flag, tuning_guard):
    run, cache = isolated_capture
    result = run(0, flags=[flag], tuning_guard=tuning_guard, refusal=True)
    assert result.returncode != 0
    assert "HashSeedError: ineffective_python_hash_seed" in result.stderr
    assert not list(cache.rglob("*.json"))


@pytest.mark.parametrize("flags", [["-X", "utf8"], ["-X", "int_max_str_digits=1000"],
                                  ["-X", "synthetic_option=enabled"], ["-W", "ignore"], ["-b"], ["-q"], ["-OO"]])
def test_interpreter_options_change_capture_key(isolated_capture, flags):
    run, _ = isolated_capture
    first, second = run(0), run(1, flags=flags)
    assert first["environment_sha256"] == second["environment_sha256"]
    assert first["key"] != second["key"]
    assert second["misses"] == 1 and second["hits"] == 0
    assert run(1, flags=flags)["hits"] == 1


@pytest.mark.parametrize("source", ["cpu_count", "process_cpu_count", "sched_getaffinity"])
def test_native_default_thread_inputs_change_runtime_identity(monkeypatch, source):
    import subject_identity as identity
    from capture_identity import digest
    before = digest(identity.interpreter_state())
    value = {123456} if source == "sched_getaffinity" else 123456
    monkeypatch.setattr(identity.os, source, lambda *args: value, raising=False)
    assert digest(identity.interpreter_state()) != before


@pytest.mark.parametrize("source,value", [("get_int_max_str_digits", 1000),
                                          ("getrecursionlimit", 123456),
                                          ("getswitchinterval", 0.123456)])
def test_mutable_interpreter_semantics_change_identity(monkeypatch, source, value):
    import subject_identity as identity
    from capture_identity import digest
    before = digest(identity.interpreter_state())
    monkeypatch.setattr(identity.sys, source, lambda: value)
    assert digest(identity.interpreter_state()) != before


@pytest.mark.parametrize("seed,randomization,ignore_environment", [
    ("1", 0, 0), ("0", 1, 0), ("0", 0, 1)])
@pytest.mark.parametrize("tuning_guard", [False, True])
def test_hash_seed_guard_checks_effective_flags(monkeypatch, seed, randomization,
                                               ignore_environment, tuning_guard):
    from types import SimpleNamespace
    import subject_identity as identity
    monkeypatch.setenv("PYTHONHASHSEED", seed)
    monkeypatch.setattr(identity, "sys", SimpleNamespace(flags=SimpleNamespace(
        hash_randomization=randomization, ignore_environment=ignore_environment)))
    guard = identity.require_hash_seed
    if tuning_guard:
        monkeypatch.syspath_prepend(str(Path(__file__).parent / "tuned_presidio"))
        import tune
        guard = tune.require_hash_seed
    with pytest.raises(identity.HashSeedError, match="^ineffective_python_hash_seed$") as caught:
        guard()
    assert caught.value.code == "ineffective_python_hash_seed"
