#!/usr/bin/env python3
"""Recheck every committed mutation whose edit site is regex.rs."""
import ast
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "scripts/ci/invisible_separator_mutation_probe.py"


def main():
    probes = [ast.literal_eval(node.args[0]) for node in ast.walk(ast.parse(RUNNER.read_text()))
              if isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
              and node.func.id == "reject" and len(node.args) >= 2
              and isinstance(node.args[1], ast.Name) and node.args[1].id == "REGEX_SOURCE"]
    if not probes or len(set(probes)) != len(probes):
        raise RuntimeError("regex mutation site selection must be nonempty and unique")
    print(f"REGEX SITE GATE 0/{len(probes)}: {', '.join(probes)}", flush=True)
    arguments = [argument for probe in probes for argument in ("--probe", probe)]
    return subprocess.run([sys.executable, str(RUNNER), *arguments], cwd=ROOT).returncode


if __name__ == "__main__":
    raise SystemExit(main())
