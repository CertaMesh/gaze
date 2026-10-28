#!/usr/bin/env python3
"""Build current-main and candidate release CLIs without collecting timings."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

from invisible_separator_release import resources

ROOT = Path(__file__).resolve().parents[2]
BASE_SHA = "3b3808c66a528a308b293b5d6bfd1e4924a637c1"


def main():
    base = Path(os.environ["GAZE_COLD_BASE_WORKTREE"]).resolve(strict=True)
    context = json.loads(subprocess.check_output(["anvil", "context", str(base), "--json"], cwd=base, text=True))["data"]
    if not context["ready"] or not context["worktree"]["detached"] or context["worktree"]["head"] != BASE_SHA:
        raise RuntimeError("cold resource baseline must be the prepared pinned-main review")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=base).strip():
        raise RuntimeError("cold resource baseline must be clean")
    resources(base)
    previous = ROOT / "target/separator-checks/release-build.json"
    binary = ROOT / "target/release/gaze"
    if previous.exists() and binary.exists():
        measured = json.loads(previous.read_text())
        if hashlib.sha256(binary.read_bytes()).hexdigest() == measured["binary_sha256"]["candidate"]:
            shutil.copy2(binary, previous.parent / "measured-clean-equality-gaze")
    print("COLD BUILDS 0/2: exact current main", flush=True)
    subprocess.run(["anvil", "run", str(base), "--", "env", "CARGO_BUILD_JOBS=2", "GAZE_DISABLE_OS_KEYSTORE=1",
                    "nice", "-n", "10", "cargo", "build", "--locked", "--release", "-p", "gaze-cli"], cwd=base, check=True)
    resources(base)
    print("COLD BUILDS 1/2: candidate with identical default features", flush=True)
    subprocess.run(["env", "CARGO_BUILD_JOBS=2", "GAZE_DISABLE_OS_KEYSTORE=1", "nice", "-n", "10",
                    "cargo", "build", "--locked", "--release", "-p", "gaze-cli"], cwd=ROOT, check=True)
    resources(base)
    report = {"schema": "gaze-cold-build-v1", "base_sha": BASE_SHA,
              "candidate_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "features": "gaze-cli default", "timing_capture_started": False,
              "binary_sha256": {side: hashlib.sha256((root / "target/release/gaze").read_bytes()).hexdigest()
                                for side, root in (("base", base), ("candidate", ROOT))}}
    (ROOT / "target/separator-checks/cold-build.json").write_text(json.dumps(report, indent=2) + "\n")
    print("COLD BUILDS 2/2: complete; no benchmark timing started", flush=True)


if __name__ == "__main__":
    main()
