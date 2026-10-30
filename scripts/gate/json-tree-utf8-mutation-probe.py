#!/usr/bin/env python3
"""Exercise type and counted-work guards against UTF-8 suffix revalidation.

Run in a clean, committed worktree. Executes real parser tests before and after
mutations; requires compiler diagnostics or the scaling assertion to reject them. No
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
RUNTIME_PROBES = [
    ("copy-range-suffix", "",
     "self.input.copy_range(start..self.input.len(), self.input.len())?.as_bytes()"),
    ("byte-at-suffix",
     "let suffix = (start..self.input.len())"
     ".filter_map(|i| self.input.byte_at(i)).collect::<Vec<u8>>();\n",
     "&suffix"),
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


def rejected_at_line(result, code, line):
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


def rejected_by_scaling_test(result):
    return (
        result.returncode == 101
        and "test json_tree::tests::non_ascii_strings_are_validated_once ... FAILED" in result.stdout
        and "doubling scalar count must at most double total input work" in result.stdout
        and re.search(r"test result: FAILED\. \d+ passed; 1 failed;", result.stdout)
        and '"level":"error"' not in result.stdout
    )


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
            if not rejected_at_line(result, code, line):
                raise RuntimeError(f"{name}: raw conversion escaped the type guard; inspect probe logs")
        finally:
            path.write_bytes(saved)
        probes.append({"name": name, "mutation": "rejected", "diagnostic": code})
        print(json.dumps(probes[-1]), flush=True)
    name = "raw-field-capture"
    capture_anchors = [
        ("    parse_validated(ValidatedInput::new(input)?, limits)",
         "    let raw = input;\n    parse_validated(ValidatedInput::new(input)?, limits)"),
        ("struct Parser<'a> {", "struct Parser<'a> {\n    raw: &'a [u8],"),
        ("    let mut parser = Parser {", "    let mut parser = Parser {\n        raw,"),
        (ANCHOR, "let character = std::str::from_utf8(&self.raw[start..])\n"
         "                        .map_err(|_| JsonTreeError::InvalidUtf8)?\n"
         "                        .chars().next().ok_or(JsonTreeError::InvalidUtf8)?;"),
    ]
    mutation = original
    for before, after in capture_anchors:
        if mutation.count(before) != 1:
            raise RuntimeError(f"{name}: mutation anchor must be unique: {before}")
        mutation = mutation.replace(before, after)
    line = mutation[:mutation.index("        raw,")].count("\n") + 1
    try:
        path.write_text(mutation)
        result = run_test(env, name, output_dir)
        if not rejected_at_line(result, "E0425", line):
            raise RuntimeError(f"{name}: raw input reached parser construction; inspect probe logs")
    finally:
        path.write_bytes(saved)
    probes.append({"name": name, "mutation": "rejected", "diagnostic": "E0425"})
    print(json.dumps(probes[-1]), flush=True)
    for name, prefix, raw_bytes in RUNTIME_PROBES:
        mutation = (
            prefix + f"let character = std::str::from_utf8({raw_bytes})\n"
            "                        .map_err(|_| JsonTreeError::InvalidUtf8)?\n"
            "                        .chars().next().ok_or(JsonTreeError::InvalidUtf8)?;"
        )
        try:
            path.write_text(original.replace(ANCHOR, mutation))
            result = run_test(env, name, output_dir)
            if not rejected_by_scaling_test(result):
                raise RuntimeError(f"{name}: expected the scaling assertion to fail; inspect probe logs")
        finally:
            path.write_bytes(saved)
        probes.append({"name": name, "mutation": "rejected", "diagnostic": "scaling assertion"})
        print(json.dumps(probes[-1]), flush=True)
    restored = require_pass(run_test(env, "restored", output_dir))
    report = {
        "schema": "json-tree-utf8-mutations-v2",
        "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "baseline_tests": baseline, "probes": probes, "restored_tests": restored,
    }
    (output_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report), flush=True)


if __name__ == "__main__":
    main()
