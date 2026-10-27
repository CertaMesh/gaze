#!/usr/bin/env python3
"""Bypass each competitor admission refusal and delete each document-key field."""
from __future__ import annotations

import argparse
import ast
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
COMPARE = "scripts/bench/compare/"
TESTS = [COMPARE + "test_capture_guards.py", COMPARE + "test_runtime_closure.py",
         COMPARE + "test_document_capture.py", COMPARE + "test_gliner_identity.py",
         COMPARE + "tuned_presidio/test_input_binding.py"]
FILES = ["subject_identity.py", "document_capture.py", "gliner_identity.py", "capture_compare.py",
         "runtime_closure.py", "tuned_presidio/input_binding.py", "tuned_presidio/capture_produce.py", "tuned_presidio/capture_tune.py"]


def offset(lines, row, column):
    return sum(len(line) for line in lines[:row - 1]) + column


def mutations():
    for filename in FILES:
        path = ROOT / COMPARE / filename
        original = path.read_bytes()
        lines = original.splitlines(keepends=True)
        tree = ast.parse(original)
        for node in ast.walk(tree):
            if (filename == "subject_identity.py" and isinstance(node, ast.Assign)
                    and any(isinstance(target, ast.Subscript) and isinstance(target.slice, ast.Constant)
                            and target.slice.value == "model_selectors" for target in node.targets)):
                start = offset(lines, node.lineno, node.col_offset)
                end = offset(lines, node.end_lineno, node.end_col_offset)
                yield f"subject-model-selectors-{node.lineno}", path, original[:start] + b"pass" + original[end:]
            if (isinstance(node, ast.Expr) and isinstance(node.value, ast.Call)
                    and isinstance(node.value.func, ast.Attribute)):
                call = node.value
                if (filename == "document_capture.py" and call.func.attr == "append"
                        and isinstance(call.func.value, ast.Attribute) and call.func.value.attr == "observations"):
                    argument = call.args[0]
                    start = offset(lines, argument.lineno, argument.col_offset)
                    end = offset(lines, argument.end_lineno, argument.end_col_offset)
                    yield f"document-observation-content-{node.lineno}", path, original[:start] + b"digest(key)" + original[end:]
                if (filename == "capture_compare.py" and call.func.attr == "validate"
                        and isinstance(call.func.value, ast.Name) and call.func.value.id == "freshness"):
                    start = offset(lines, node.lineno, node.col_offset)
                    end = offset(lines, node.end_lineno, node.end_col_offset)
                    yield f"freshness-admission-{node.lineno}", path, original[:start] + b"pass" + original[end:]
            if isinstance(node, ast.If) and any(
                    isinstance(statement, ast.Raise) or
                    (isinstance(statement, ast.Call) and isinstance(statement.func, ast.Attribute)
                     and statement.func.attr in {"_bootstrap_context", "_check_bytecode"})
                    for child in node.body for statement in ast.walk(child)):
                if ast.unparse(node.test) == "__name__ == '__main__'":
                    continue  # Script startup is not an input-admission guard.
                test = node.test
                start = offset(lines, test.lineno, test.col_offset)
                end = offset(lines, test.end_lineno, test.end_col_offset)
                yield f"{Path(filename).stem}-guard-{node.lineno}", path, original[:start] + b"False" + original[end:]
        if filename == "subject_identity.py":
            for function in tree.body:
                if not isinstance(function, ast.FunctionDef) or function.name not in {"document_key", "subject", "runtime", "public_binding"}:
                    continue
                # Delete every field in the externally consumed private key schema.
                for node in ast.walk(function):
                    if not isinstance(node, ast.Dict):
                        continue
                    for index, key in enumerate(node.keys):
                        if not isinstance(key, ast.Constant) or not isinstance(key.value, str):
                            continue
                        value = node.values[index]
                        start = offset(lines, key.lineno, key.col_offset)
                        end = offset(lines, value.end_lineno, value.end_col_offset)
                        if index + 1 < len(node.keys):
                            following = node.keys[index + 1]
                            end = offset(lines, following.lineno, following.col_offset)
                        elif index:
                            previous = node.values[index - 1]
                            start = offset(lines, previous.end_lineno, previous.end_col_offset)
                        yield f"{function.name}-{key.value}-{key.lineno}", path, original[:start] + original[end:]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", action="append")
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()
    selected = list(mutations())
    if args.only:
        selected = [row for row in selected if row[0] in args.only]
        if {row[0] for row in selected} != set(args.only):
            raise SystemExit("unknown mutation")
    if args.list:
        print("\n".join(row[0] for row in selected))
        return
    if subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT).strip():
        raise SystemExit("commit before running mutation probes")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    output = ROOT / "target/competitor-mutations"
    output.mkdir(parents=True, exist_ok=True)
    baseline = output / "baseline.log"
    command = ["anvil", "run", "--", "uv", "run", "--project", "scripts/bench", "--python", "3.13",
               "--group", "comparison-test", "python", "-m", "pytest", "-q", "--color=no", *TESTS]
    with baseline.open("w") as stream:
        result = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT)
    if result.returncode:
        raise SystemExit("positive baseline failed; no mutation evidence accepted")
    receipts = []
    for name, path, changed in selected:
        original = path.read_bytes()
        log = output / (name + ".log")
        command = ["anvil", "run", "--", "uv", "run", "--project", "scripts/bench", "--python", "3.13",
                   "--group", "comparison-test", "python", "-m", "pytest", "-q", "-x", "--color=no", *TESTS]
        try:
            path.write_bytes(changed)
            with log.open("w") as stream:
                result = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT)
        finally:
            path.write_bytes(original)
        text = log.read_text()
        killed = result.returncode == 1 and " failed" in text and "ERROR collecting" not in text
        receipt = {"mutation": name, "killed": killed, "exit": result.returncode,
                   "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest()}
        receipts.append(receipt)
        print(json.dumps(receipt), flush=True)
    (output / "receipts.json").write_text(json.dumps({"head": head, "receipts": receipts}, indent=2) + "\n")
    if not all(row["killed"] for row in receipts):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
