#!/usr/bin/env python3
"""Prove short-record refusal and value-free stderr against two single-site mutations.

Run through anvil run with no concurrent builds in this worktree. Source is
restored in finally, including when a mutant survives. Logs stay under target/.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/gaze-cli/src/pipeline/run.rs"
OUTPUT = ROOT / "target/caller-context-mutations"
SITE = "ContextEnforcement::Enforce => enforce_context_transformable(context)?,"
TEST = "caller_value_too_short_refuses_without_output_or_value"


def run(name):
    while True:
        cargo = subprocess.run(["pgrep", "-x", "cargo"], capture_output=True, text=True)
        memory = subprocess.check_output(["memory_pressure"], text=True).splitlines()[-1]
        free = int(re.search(r"(\d+)%", memory)[1])
        if len(cargo.stdout.splitlines()) < 2 and free >= 30:
            break
        time.sleep(5)
    env = dict(os.environ, CARGO_BUILD_JOBS="2", CARGO_PROFILE_DEV_DEBUG="0",
               CARGO_PROFILE_TEST_DEBUG="0", CARGO_INCREMENTAL="0", GAZE_DISABLE_OS_KEYSTORE="1")
    result = subprocess.run([
        "nice", "-n", "10", "cargo", "test", "--locked", "-p", "gaze-cli",
        "--test", "cli_pipe", TEST, "--", "--exact",
    ], cwd=ROOT, env=env, capture_output=True)
    log = result.stdout + result.stderr
    (OUTPUT / f"{name}.log").write_bytes(log)
    return result.returncode, log


def main():
    OUTPUT.mkdir(parents=True, exist_ok=True)
    original = SOURCE.read_text()
    assert original.count(SITE) == 1
    mutations = {
        "skip-refusal": "ContextEnforcement::Enforce => (),",
        "echo-context": """ContextEnforcement::Enforce => {
            eprintln!("{}", std::fs::read_to_string(options.context_json.unwrap()).unwrap());
            enforce_context_transformable(context)?;
        }""",
    }
    receipt = {"source_sha256": hashlib.sha256(original.encode()).hexdigest(), "mutations": {}}
    try:
        for name, replacement in mutations.items():
            SOURCE.write_text(original.replace(SITE, replacement))
            code, log = run(name)
            killed = code == 101 and f"test {TEST} ... FAILED".encode() in log
            receipt["mutations"][name] = {"exit": code, "killed_by_regression": killed}
            assert killed, f"{name} must fail the regression test, not compilation"
    finally:
        SOURCE.write_text(original)
    code, _ = run("restored")
    receipt["restored_exit"] = code
    assert code == 0
    (OUTPUT / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
