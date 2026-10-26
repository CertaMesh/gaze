#!/usr/bin/env python3
"""Build the clean-equality baseline in its own prepared Anvil worktree.

Run only inside the named mcp-host-release check, which owns workspace-build.
GAZE_CLEAN_BASE_WORKTREE selects an existing clean, detached review worktree.
The Anvil context pins its source commit; the resulting binary hash is recorded
by verify_clean_equality.py. No target directory or artifacts are shared.
"""
import json
import os
from pathlib import Path
import subprocess


def main():
    base = Path(os.environ["GAZE_CLEAN_BASE_WORKTREE"]).resolve(strict=True)
    context = json.loads(subprocess.check_output(["anvil", "context", str(base), "--json"], text=True))
    data = context["data"]
    if not data["ready"] or not data["worktree"]["detached"]:
        raise RuntimeError("base must be a prepared detached review worktree")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=base, text=True).strip():
        raise RuntimeError("base worktree must be clean")
    if data["worktree"]["head"] != data["metadata"]["pinned_commit"]:
        raise RuntimeError("base commit differs from review pin")
    subprocess.run(["anvil", "run", str(base), "--", "env", "CARGO_BUILD_JOBS=2",
                    "GAZE_DISABLE_OS_KEYSTORE=1", "nice", "-n", "10", "cargo", "build",
                    "--locked", "--release", "-p", "gaze-cli", "--features", "mcp"], check=True)
    print(json.dumps({"base_sha": data["worktree"]["head"], "built": True}))


if __name__ == "__main__":
    main()
