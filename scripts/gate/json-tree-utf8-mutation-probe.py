#!/usr/bin/env python3
"""Prove raw UTF-8 suffix revalidation cannot bypass the parser input boundary.

Run in a clean, committed worktree. Executes real parser tests before and after
mutations; requires compiler diagnostics at each injected raw conversion. No
source-pattern acceptance checks or timing assertions. Sources are always restored.
"""

import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path("crates/gaze-types/src/json_tree.rs")
ANCHOR = """let character = self
                        .input
                        .scalar_at(start)
                        .ok_or(JsonTreeError::InvalidUtf8)?;"""
PROBES = [
    ("raw-suffix", "E0608", "&self.input[start..]"),
    ("private-text-bypass", "E0616", "&self.input.text.as_bytes()[start..]"),
]


def run_test(env, name, output_dir):
    result = subprocess.run(
        ["nice", "-n", "10", "cargo", "test", "--locked", "-p", "gaze-types",
         "--lib", "--features", "perf-timers", "--message-format=json",
         "json_tree::tests::"],
        cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, check=False,
    )
    (output_dir / f"{name}.log").write_text(result.stdout)
    return result


def require_pass(result):
    summaries = re.findall(r"test result: ok\. (\d+) passed; 0 failed;", result.stdout)
    if result.returncode or not summaries or not all(int(count) > 0 for count in summaries):
        raise RuntimeError("real parser tests must execute and pass; inspect probe logs")
    return sum(map(int, summaries))


def rejected_at_conversion(result, code, line):
    for raw in result.stdout.splitlines():
        if not raw.startswith("{"):
            continue
        event = json.loads(raw)
        if event.get("reason") != "compiler-message":
            continue
        message = event["message"]
        if (message.get("code") or {}).get("code") != code:
            continue
        if any(span["is_primary"] and span["file_name"] == SOURCE.as_posix()
               and span["line_start"] <= line <= span["line_end"]
               for span in message["spans"]):
            return result.returncode != 0
    return False


def main():
    dirty = subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT,
    )
    if dirty:
        raise RuntimeError("commit tracked changes before running mutations")
    path = ROOT / SOURCE
    saved = path.read_bytes()
    original = saved.decode()
    if original.count(ANCHOR) != 1:
        raise RuntimeError("mutation anchor must be unique")
    env = os.environ.copy()
    env["CARGO_BUILD_JOBS"] = "4"
    output_dir = ROOT / "target/json-tree-utf8-mutations"
    output_dir.mkdir(parents=True, exist_ok=True)
    baseline = require_pass(run_test(env, "baseline", output_dir))
    probes = []
    for name, code, raw_bytes in PROBES:
        mutation = (
            f"let character = std::str::from_utf8({raw_bytes})\n"
            "                        .map_err(|_| JsonTreeError::InvalidUtf8)?\n"
            "                        .chars().next().ok_or(JsonTreeError::InvalidUtf8)?;"
        )
        line = original[:original.index(ANCHOR)].count("\n") + 1
        try:
            path.write_text(original.replace(ANCHOR, mutation))
            result = run_test(env, name, output_dir)
            if not rejected_at_conversion(result, code, line):
                raise RuntimeError(f"{name}: raw conversion escaped the type guard; inspect probe logs")
        finally:
            path.write_bytes(saved)
        probes.append({"name": name, "mutation": "rejected", "diagnostic": code})
        print(json.dumps(probes[-1]), flush=True)
    restored = require_pass(run_test(env, "restored", output_dir))
    report = {
        "schema": "json-tree-utf8-mutations-v1",
        "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "baseline_tests": baseline, "probes": probes, "restored_tests": restored,
    }
    (output_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report), flush=True)


if __name__ == "__main__":
    main()
