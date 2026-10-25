#!/usr/bin/env python3
"""Prove the keystore guard using a fake backend, never the native OS store."""
import argparse
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/gaze-token-bridge/src/persistent.rs"
CONFIG = ROOT / ".cargo/config.toml"
TEST = "cargo_tests_and_children_never_enter_the_os_keystore"
COMMAND = ["cargo", "test", "-p", "gaze-token-bridge", "--features", "os-keychain",
           "--test", "os_keystore_isolation", "--", "--exact", TEST, "--nocapture"]


def run(tag, expected):
    output = ROOT / "target/gate/os-keystore"
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    env.pop("GAZE_DISABLE_OS_KEYSTORE", None)
    with (output / f"{tag}.log").open("w") as log:
        result = subprocess.run(COMMAND, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    text = (output / f"{tag}.log").read_text()
    outcome = "ok" if expected == 0 else "FAILED"
    assert result.returncode == expected, f"{tag}: unexpected exit; inspect log"
    assert f"test {TEST} ... {outcome}" in text, f"{tag}: named test did not execute"
    if expected:
        assert "panicked at" in text, f"{tag}: no executed assertion failure"
    print(f"{tag}: compiled and executed named test: {outcome}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", action="store_true", help="also run focused checks and the full feature matrix")
    args = parser.parse_args()
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT), "commit before probing; no stash"
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    source = SOURCE.read_text()
    config = CONFIG.read_text()
    assert "Box::new(ForbiddenBackend)" in (ROOT / "crates/gaze-token-bridge/tests/os_keystore_isolation.rs").read_text()
    run("baseline", 0)
    try:
        before = 'if cfg!(test) || env::var_os("GAZE_DISABLE_OS_KEYSTORE").is_some() {'
        assert source.count(before) == 1
        SOURCE.write_text(source.replace(before, "if cfg!(test) {"))
        run("remove-runtime-guard", 101)
    finally:
        SOURCE.write_text(source)
    try:
        before = 'GAZE_DISABLE_OS_KEYSTORE = { value = "1", force = true }'
        assert config.count(before) == 1
        CONFIG.write_text(config.replace(before, ""))
        run("remove-cargo-marker", 101)
    finally:
        CONFIG.write_text(config)
    run("restored", 0)
    (ROOT / "target/gate/os-keystore/results.json").write_text(json.dumps({
        "head": head, "baseline": "pass", "restored": "pass",
        "mutations": {"remove-runtime-guard": "compiled_executed_and_killed", "remove-cargo-marker": "compiled_executed_and_killed"},
        "native_backend_calls": 0,
    }, indent=2) + "\n")
    if args.workspace:
        commands = [
            ["cargo", "test", "-p", "gaze-token-bridge", "--features", "os-keychain"],
            ["cargo", "test", "-p", "gaze-cli", "--all-features", "--features", "gaze-token-bridge/os-keychain", "--test", "index_cli", "--test", "audit_cross_version", "--test", "cli_pipe"],
            ["cargo", "clippy", "-p", "gaze-token-bridge", "-p", "gaze-cli", "--all-features", "--all-targets", "--", "-D", "warnings"],
            ["cargo", "run", "-p", "xtask", "--", "ci-feature-matrix"],
        ]
        for command in commands:
            print("keystore verification: " + " ".join(command), flush=True)
            subprocess.run(command, cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
