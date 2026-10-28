#!/usr/bin/env python3
"""Run the unchanged field-geometry contract on pinned main."""
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SHA = "3b3808c66a528a308b293b5d6bfd1e4924a637c1"
TEST = "actual_assembly_normalization_exposes_source_boundaries_and_raw_size_limitations"


def main():
    worktree = Path(os.environ["GAZE_FIELD_MAIN_WORKTREE"]).resolve(strict=True)
    if subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=worktree, text=True).strip() != SHA:
        raise RuntimeError("main probe must use the pinned commit")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=worktree).strip():
        raise RuntimeError("main probe must use a clean checkout")
    result = subprocess.run(["anvil", "run", str(worktree), "--", "env", "CARGO_BUILD_JOBS=2",
                             "GAZE_DISABLE_OS_KEYSTORE=1", "nice", "-n", "10", "cargo", "test", "--locked",
                             "-p", "gaze-recognizers", "--all-features", "--test", "explicit_fields", TEST,
                             "--", "--exact", "--nocapture"], cwd=worktree)
    output = ROOT / "target/separator-checks/field-geometry-main.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({"schema": "gaze-field-geometry-main-v1", "source_sha": SHA,
                                  "test": TEST, "exit_code": result.returncode}, indent=2) + "\n")
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
