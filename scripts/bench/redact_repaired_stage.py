#!/usr/bin/env python3
"""Owned stage supervision under the original root-granted absolute deadline."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

DEADLINE_UTC = "2026-09-10T11:14:11+00:00"
DEADLINE = datetime.datetime.fromisoformat(DEADLINE_UTC).timestamp()
CLEANUP_RESERVE = 5.0
PYTHON = "/Users/krishankoenig/Workspace/EmpireTwo/gaze-quality-7377/scripts/bench/.venv/bin/python"


def intended_commands():
    commands = {
        "workspace-bootstrap": ["cargo", "build", "--workspace", "--all-features", "--locked", "--offline", "-j", "2"],
        "producer-build": ["cargo", "build", "--locked", "--offline", "-j", "2", "-p", "gaze-recognizers", "--example", "clean_for_bench", "--features", "safety-net-kiji,redact-live"],
        "validator-build": ["cargo", "build", "--locked", "--offline", "-j", "2", "--manifest-path", "scripts/bench/validator_recall_probe/Cargo.toml", "--target-dir", "target/validator-recall-probe"],
    }
    for name, module in (("python-tests", "test_redact_repaired_dev.py"),
                         ("runner-tests", "test_run_no_opf_benchmark.py"),
                         ("observer-tests", "test_output_proof.py"),
                         ("supervisor-tests", "test_redact_repaired_stage.py")):
        commands[name] = [PYTHON, "-m", "unittest", "discover", "-s", "scripts/bench", "-p", module]
    for phase in ("freeze", "smoke", "dev"):
        commands[phase] = [PYTHON, "scripts/bench/redact_repaired_dev.py", phase,
                           "--frozen", "target/quality-7390/frozen-dev.json",
                           "--dataset", "/Users/krishankoenig/Workspace/EmpireTwo/gaze/target/bench-data/dataiku-en-de/test.parquet",
                           "--output", "target/quality-7390/paired"]
    return commands


def selected_environment():
    toolchain = Path.home() / ".rustup/toolchains/1.96.0-aarch64-apple-darwin/bin"
    return {
        "GAZE_NER_LOCALE": None, "RUSTC": str(toolchain / "rustc"),
        "RUSTDOC": str(toolchain / "rustdoc"), "CARGO_BUILD_JOBS": "2",
        "CARGO_NET_OFFLINE": "true", "ORT_SKIP_DOWNLOAD": "true",
        "ORT_LIB_PATH": str(Path.home() / "Library/Caches/ort.pyke.io/dfbin/aarch64-apple-darwin/612739f75438dc0a075461e1fb454226b4a1eb175e60a7271ba966bbbb972cd4"),
        "GAZE_REDACT_BRIDGE": str(Path.home() / ".local/share/gaze-private/redact-7383/sdk/.build/debug/gaze-redact-bridge"),
        "GAZE_REDACT_MODEL_DIR": str(Path.home() / ".local/share/gaze-private/redact-7370/model-patched-coreml"),
    }


def source_state(repo):
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
    clean = not subprocess.check_output(["git", "status", "--porcelain"], cwd=repo)
    return {"head": head, "clean": clean}


def output_paths(repo, name):
    relative = {"producer-build": "target/debug/examples/clean_for_bench",
                "validator-build": "target/validator-recall-probe/debug/validator-recall-probe",
                "freeze": "target/quality-7390/paired/freeze.json",
                "smoke": "target/quality-7390/paired/smoke.json"}.get(name)
    return [repo / relative] if relative else []


def output_hashes(repo, name):
    return {str(path.relative_to(repo)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in output_paths(repo, name)}


def process_snapshot():
    result = subprocess.run(["ps", "-axo", "pid=,ppid=,lstart="],
                            capture_output=True, text=True, timeout=1, check=True)
    rows = {}
    for line in result.stdout.splitlines():
        fields = line.split(None, 2)
        if len(fields) == 3:
            rows[int(fields[0])] = (int(fields[1]), fields[2])
    return rows


def owned_snapshot(parent, known):
    snapshot = process_snapshot()
    roots = {pid for pid, birth in known.items()
             if pid in snapshot and snapshot[pid][1] == birth}
    if not known and parent in snapshot:
        roots.add(parent)
    while True:
        children = {pid for pid, (ppid, _) in snapshot.items() if ppid in roots}
        if children <= roots:
            break
        roots.update(children)
    for pid in roots:
        if pid in snapshot:
            known[pid] = snapshot[pid][1]
    return {pid for pid, birth in known.items()
            if pid in snapshot and snapshot[pid][1] == birth}


def signal_owned(parent, known, signum):
    # Individual verified descendants include nested bridge process groups.
    for pid in owned_snapshot(parent, known):
        try:
            os.kill(pid, signum)
        except ProcessLookupError:
            pass


def supervise(command, *, cwd, env, log, deadline=DEADLINE):
    interrupted = []
    old_handlers = {}
    process = None
    known = {}
    began = time.monotonic()
    # A monotonic cap prevents wall-clock rollback extending the grant.
    stop = began + max(0, deadline - time.time() - CLEANUP_RESERVE)
    result = {"status": "not_started", "exit_code": 2,
              "deadline_utc": datetime.datetime.fromtimestamp(
                  deadline, datetime.timezone.utc).isoformat(),
              "cleanup_reserve_seconds": CLEANUP_RESERVE}
    for signum in (signal.SIGINT, signal.SIGTERM):
        old_handlers[signum] = signal.signal(signum, lambda number, frame: interrupted.append(number))
    try:
        if time.monotonic() >= stop:
            result["status"] = "deadline_before_start"
            return result
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=log,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        result["owned_root_pid"] = process.pid
        while process.poll() is None:
            owned_snapshot(process.pid, known)
            if interrupted or time.monotonic() >= stop or time.time() >= deadline - CLEANUP_RESERVE:
                result["status"] = "interrupted" if interrupted else "timeout"
                break
            time.sleep(0.1)
        else:
            result.update(status="exited", exit_code=process.returncode)
    except BaseException as error:
        result.update(status="supervisor_error", error_type=type(error).__name__)
    finally:
        if process is not None:
            # Even a successful parent must leave no owned descendants running.
            try:
                alive = owned_snapshot(process.pid, known)
                if alive:
                    signal_owned(process.pid, known, signal.SIGTERM)
                    until = time.monotonic() + 2
                    while time.monotonic() < until:
                        process.poll()
                        if not owned_snapshot(process.pid, known):
                            break
                        time.sleep(0.1)
                    signal_owned(process.pid, known, signal.SIGKILL)
                process.wait(timeout=1)
                remaining = owned_snapshot(process.pid, known)
                until = time.monotonic() + 0.5
                while remaining and time.monotonic() < until:
                    time.sleep(0.05)
                    remaining = owned_snapshot(process.pid, known)
                result["owned_remaining"] = len(remaining)
                if remaining:
                    result.update(status="cleanup_incomplete", exit_code=2)
            except BaseException as error:
                # This root was created by this supervisor; do not signal outsiders.
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=1)
                result.update(status="cleanup_error", exit_code=2,
                              cleanup_error_type=type(error).__name__)
        for signum, handler in old_handlers.items():
            signal.signal(signum, handler)
        result["seconds"] = time.monotonic() - began
    return result


def main():
    repo = Path(__file__).resolve().parents[2]
    out = repo / "target/quality-7390"
    name, command = sys.argv[1], sys.argv[2:]
    env = os.environ.copy()
    for key, value in selected_environment().items():
        if value is None:
            env.pop(key, None)
        else:
            env[key] = value
    toolchain = Path.home() / ".rustup/toolchains/1.96.0-aarch64-apple-darwin/bin"
    env.update(RUSTC=str(toolchain / "rustc"), RUSTDOC=str(toolchain / "rustdoc"),
               PATH=str(toolchain) + ":" + env["PATH"], CARGO_BUILD_JOBS="2",
               CARGO_NET_OFFLINE="true", ORT_SKIP_DOWNLOAD="true",
               ORT_LIB_PATH=str(Path.home() / "Library/Caches/ort.pyke.io/dfbin/aarch64-apple-darwin/612739f75438dc0a075461e1fb454226b4a1eb175e60a7271ba966bbbb972cd4"),
               GAZE_REDACT_BRIDGE=str(Path.home() / ".local/share/gaze-private/redact-7383/sdk/.build/debug/gaze-redact-bridge"),
               GAZE_REDACT_MODEL_DIR=str(Path.home() / ".local/share/gaze-private/redact-7370/model-patched-coreml"))
    before = source_state(repo)
    assert before["clean"] and command == intended_commands()[name]
    record = {"step": name, "command": command, "source_head": before["head"],
              "source_before": before,
              "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "stage_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "effective_environment": {key: env.get(key) for key in (
                  "GAZE_NER_LOCALE", "RUSTC", "RUSTDOC", "CARGO_BUILD_JOBS",
                  "CARGO_NET_OFFLINE", "ORT_SKIP_DOWNLOAD", "ORT_LIB_PATH",
                  "GAZE_REDACT_BRIDGE", "GAZE_REDACT_MODEL_DIR")}}
    # An append-only start marker survives interruption before a final receipt.
    with (out / (name + ".started.json")).open("x") as handle:
        json.dump(record, handle)
    try:
        with (out / (name + ".log")).open("xb") as log:
            record.update(supervise(command, cwd=repo, env=env, log=log))
    finally:
        try:
            record["source_after"] = source_state(repo)
            if record["source_after"] != before:
                record.update(status="source_changed", exit_code=2)
            if record.get("exit_code") == 0:
                record["output_sha256"] = output_hashes(repo, name)
        except BaseException as error:
            record.update(status="binding_error", exit_code=2, error_type=type(error).__name__)
        with (out / (name + ".json")).open("x") as handle:
            json.dump(record, handle, indent=2)
            handle.write("\n")
        print(json.dumps({key: record.get(key) for key in (
            "step", "status", "exit_code", "seconds", "owned_remaining", "source_head")}))
    return record.get("exit_code", 2)


if __name__ == "__main__":
    sys.exit(main())
