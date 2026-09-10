#!/usr/bin/env python3
"""Explicit-deadline custody for the private fourth-arm experiment. No grant implied."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import redact_repaired_stage as supervisor

SUPERVISOR_SHA = "755a053768028c72db2312542056e087e69d7bbcf768bc1e7acda5f808b2f564"
ROOT = Path("target/quality-7414")
PAIRED = ROOT / "paired"
REFERENCE = ROOT / "reference-build/debug/examples/clean_for_bench"
PRODUCER = Path("target/debug/examples/clean_for_bench")
VALIDATOR = Path("target/validator-recall-probe/debug/validator-recall-probe")
PREREQUISITES = ("workspace-bootstrap", "reference-build", "producer-build",
                 "validator-build", "python-tests", "runner-tests", "observer-tests",
                 "legacy-driver-tests", "supervisor-tests")


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_new(path, value):
    with path.open("x") as handle:
        json.dump(value, handle, indent=2)
        handle.write("\n")


def deadline_value(value):
    instant = datetime.datetime.fromisoformat(value)
    if instant.tzinfo is None or instant.utcoffset() != datetime.timedelta(0):
        raise ValueError("UTC deadline required")
    return instant.timestamp()


def commands(deadline):
    base = ["cargo", "build", "--locked", "--offline", "-j", "2"]
    example = ["-p", "gaze-recognizers", "--example", "clean_for_bench"]
    result = {
        "workspace-bootstrap": base + ["--workspace", "--all-features"],
        "reference-build": base + example + ["--features", "safety-net-kiji,redact-live",
                                             "--target-dir", str(ROOT / "reference-build")],
        "producer-build": base + example + ["--features", "safety-net-kiji,redact-live,benchmark-baseline-lock"],
        "validator-build": base + ["--manifest-path", "scripts/bench/validator_recall_probe/Cargo.toml",
                                   "--features", "gaze-recognizers/benchmark-baseline-lock",
                                   "--target-dir", "target/validator-recall-probe"],
    }
    for name, module in (("python-tests", "test_baseline_lock_dev.py"),
                         ("runner-tests", "test_run_no_opf_benchmark.py"),
                         ("observer-tests", "test_output_proof.py"),
                         ("legacy-driver-tests", "test_redact_repaired_dev.py"),
                         ("supervisor-tests", "test_redact_repaired_stage.py")):
        result[name] = [supervisor.PYTHON, "-m", "unittest", "discover", "-s", "scripts/bench", "-p", module]
    for phase in ("freeze", "smoke", "dev"):
        result[phase] = [supervisor.PYTHON, "scripts/bench/baseline_lock_dev.py", phase,
                         "--deadline-utc", deadline]
    return result


def source_state(repo):
    state = supervisor.source_state(repo)
    paths = subprocess.check_output(["git", "ls-files", "-z"], cwd=repo).split(b"\0")
    source = hashlib.sha256()
    for raw in sorted(path for path in paths if path):
        path = repo / os.fsdecode(raw)
        source.update(raw + b"\0")
        source.update(bytes.fromhex(digest(path)))
    return dict(state, tracked_source_sha256=source.hexdigest())


def environment():
    values = supervisor.selected_environment()
    toolchain = Path.home() / ".rustup/toolchains/1.96.0-aarch64-apple-darwin/bin"
    values["PATH"] = str(toolchain) + ":/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
    # No inherited audit destinations, locale override, or Python assertion bypass.
    values.update(GAZE_REDACT_ADMISSION_AUDIT_FILE=None, PYTHONOPTIMIZE=None)
    return values


def toolchain_hashes():
    toolchain = Path.home() / ".rustup/toolchains/1.96.0-aarch64-apple-darwin/bin"
    return {str(path): digest(path) for path in
            [*(toolchain / name for name in ("cargo", "rustc", "rustdoc")), Path(supervisor.PYTHON)]}


def output_hashes(repo, name):
    outputs = {"reference-build": [REFERENCE], "producer-build": [PRODUCER],
               "validator-build": [VALIDATOR], "freeze": [PAIRED / "freeze.json"],
               "smoke": [PAIRED / "smoke.json", PAIRED / "smoke-joined-lock.json"]}.get(name, [])
    # Smoke binds every separate create-new audit, including reference equivalence.
    if name == "smoke":
        outputs += sorted(path.relative_to(repo) for path in
                          (repo / PAIRED).glob("smoke-*.jsonl"))
    if name == "dev":
        outputs = sorted(path.relative_to(repo) for path in (repo / PAIRED).rglob("*")
                         if path.is_file() and path.suffix in {".json", ".jsonl"})
    return {str(path): digest(repo / path) for path in outputs}


def validate_receipt(repo, name, state, deadline):
    path = repo / ROOT / (name + ".json")
    record = json.loads(path.read_text())
    assert record["step"] == name
    assert record["status"] == "exited" and record["exit_code"] == 0
    assert record["source_before"] == record["source_after"] == state and state["clean"]
    assert record["command"] == commands(deadline)[name]
    assert record["effective_environment"] == environment()
    assert record["toolchain_sha256"] == toolchain_hashes()
    assert record["supervisor_sha256"] == digest(supervisor.__file__) == SUPERVISOR_SHA
    assert record["stage_sha256"] == digest(__file__)
    assert deadline_value(record["deadline_utc"]) == deadline_value(deadline)
    assert record["cleanup_reserve_seconds"] == supervisor.CLEANUP_RESERVE
    assert record["owned_remaining"] == 0
    assert record["output_sha256"] == output_hashes(repo, name)
    assert record["log_sha256"] == digest(repo / ROOT / (name + ".log"))
    return digest(path)


def main():
    if not __debug__:
        raise RuntimeError("assertion guards required")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("step", choices=(*PREREQUISITES, "freeze", "smoke", "dev"))
    parser.add_argument("--deadline-utc", required=True)
    args = parser.parse_args()
    deadline = deadline_value(args.deadline_utc)
    assert supervisor.time.time() < deadline - supervisor.CLEANUP_RESERVE
    assert digest(supervisor.__file__) == SUPERVISOR_SHA
    repo = Path(__file__).resolve().parents[2]
    out = repo / ROOT
    out.mkdir(parents=True, exist_ok=True)
    before = source_state(repo)
    assert before["clean"]
    selected = environment()
    env = os.environ.copy()
    for key, value in selected.items():
        if value is None:
            env.pop(key, None)
        else:
            env[key] = value
    command = commands(args.deadline_utc)[args.step]
    record = {"step": args.step, "command": command, "source_before": before,
              "effective_environment": selected, "toolchain_sha256": toolchain_hashes(),
              "stage_sha256": digest(__file__), "supervisor_sha256": SUPERVISOR_SHA,
              "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    write_new(out / (args.step + ".started.json"), record)
    try:
        with (out / (args.step + ".log")).open("xb") as log:
            record.update(supervisor.supervise(command, cwd=repo, env=env, log=log, deadline=deadline))
    finally:
        try:
            record["source_after"] = source_state(repo)
            record["log_sha256"] = digest(out / (args.step + ".log"))
            if record["source_after"] != before:
                record.update(status="source_changed", exit_code=2)
            if record.get("exit_code") == 0 or args.step == "dev":
                record["output_sha256"] = output_hashes(repo, args.step)
        except BaseException as error:
            record.update(status="binding_error", exit_code=2, error_type=type(error).__name__)
        write_new(out / (args.step + ".json"), record)
        print(json.dumps({key: record.get(key) for key in
                          ("step", "status", "exit_code", "seconds", "owned_remaining")}))
    return record.get("exit_code", 2)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(json.dumps({"error_type": type(error).__name__}), file=sys.stderr)
        sys.exit(2)
