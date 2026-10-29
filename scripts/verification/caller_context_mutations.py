#!/usr/bin/env python3
"""Prove library refusal, explicit eval observation, and value-free stderr.

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
CLI_SOURCE = ROOT / "crates/gaze-cli/src/pipeline/run.rs"
ASSEMBLY_SOURCE = ROOT / "crates/gaze-assembly/src/lib.rs"
DICTIONARY_SOURCE = ROOT / "crates/gaze/src/dictionaries.rs"
OUTPUT = ROOT / "target/caller-context-mutations"
SITE = "ContextEnforcement::Enforce => enforce_context_transformable(context)?,"
TEST = "caller_value_too_short_refuses_without_output_or_value"
LIBRARY_TEST = "caller_value_too_short_refuses_all_library_assembly_paths_without_value"
EVAL_TEST = "caller_value_too_short_eval_warns_and_measures_without_value"
DICTIONARY_TEST = "dictionaries::tests::caller_value_too_short_refuses_public_dictionary_conversions_without_value"
ASSEMBLY_SITE = """    if let Some(reason) = context.untransformable_reason() {
        return Err(BuildError::Untransformable(reason));
    }
"""
DICTIONARY_SITE = """    if let Some(reason) = ctx.untransformable_reason() {
        return Err(reason);
    }
"""


def run(name, package, target, test):
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
        "nice", "-n", "10", "cargo", "test", "--locked", "-p", package,
        *(["--lib"] if target == "lib" else ["--test", target]), test, "--", "--exact",
    ], cwd=ROOT, env=env, capture_output=True)
    log = result.stdout + result.stderr
    (OUTPUT / f"{name}.log").write_bytes(log)
    return result.returncode, log


def main():
    OUTPUT.mkdir(parents=True, exist_ok=True)
    originals = {source: source.read_text() for source in (
        CLI_SOURCE, ASSEMBLY_SOURCE, DICTIONARY_SOURCE,
    )}
    cli_test = ("gaze-cli", "cli_pipe", TEST)
    library_test = ("gaze-assembly", "record_context", LIBRARY_TEST)
    eval_test = ("gaze-cli", "eval_cli", EVAL_TEST)
    dictionary_test = ("gaze-pii", "lib", DICTIONARY_TEST)
    mutations = {
        "dictionary-refusal": (DICTIONARY_SOURCE, DICTIONARY_SITE, "", dictionary_test),
        "library-refusal": (ASSEMBLY_SOURCE, ASSEMBLY_SITE, "", library_test),
        "eval-opt-out": (CLI_SOURCE, "context.record_value_rejections.clear();", "", eval_test),
        "echo-context": (CLI_SOURCE, SITE, """ContextEnforcement::Enforce => {
            eprintln!("{}", std::fs::read_to_string(options.context_json.unwrap()).unwrap());
            enforce_context_transformable(context)?;
        }""", cli_test),
    }
    receipt = {"source_sha256": {
        str(source.relative_to(ROOT)): hashlib.sha256(original.encode()).hexdigest()
        for source, original in originals.items()
    }, "mutations": {}}
    try:
        for name, (source, site, replacement, test) in mutations.items():
            original = originals[source]
            assert original.count(site) == 1
            source.write_text(original.replace(site, replacement))
            code, log = run(name, *test)
            killed = code == 101 and f"test {test[2]} ... FAILED".encode() in log
            receipt["mutations"][name] = {"exit": code, "killed_by_regression": killed}
            assert killed, f"{name} must fail the regression test, not compilation"
            source.write_text(original)
    finally:
        for source, original in originals.items():
            source.write_text(original)
    for test in (dictionary_test, library_test, eval_test, cli_test):
        code, _ = run("restored-" + test[1], *test)
        receipt["restored-" + test[1]] = code
        assert code == 0
    (OUTPUT / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
