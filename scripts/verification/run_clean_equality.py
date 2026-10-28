#!/usr/bin/env python3
"""Build a candidate and compare it with a verified, frozen main binary.

Run through the named clean-equality Anvil check for the workspace-build lock.
GAZE_CLEAN_BASE_WORKTREE selects a prepared, detached main checkout.
GAZE_CLEAN_BASE_BINARY_SHA256 binds a previously verified independent build.
The comparison covers gaze clean stdin, not proxy or MCP producer paths.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]


def git(*args, cwd=ROOT):
    return subprocess.check_output(["git", *args], cwd=cwd, text=True).strip()


def main():
    base = Path(os.environ["GAZE_CLEAN_BASE_WORKTREE"]).resolve(strict=True)
    context = json.loads(subprocess.check_output(
        ["anvil", "context", str(base), "--json"], text=True))["data"]
    main_sha = git("rev-parse", "origin/main")
    if (not context["ready"] or not context["worktree"]["detached"]
            or context["worktree"]["head"] != main_sha
            or context["metadata"]["pinned_commit"] != main_sha):
        raise RuntimeError("base must be a prepared review copy pinned to origin/main")
    if git("status", "--porcelain", cwd=base) or git("status", "--porcelain"):
        raise RuntimeError("both checkouts must be clean")
    subprocess.run(["git", "merge-base", "--is-ancestor", main_sha, "HEAD"], check=True)
    binary = base / "target/release/gaze"
    expected_hash = os.environ["GAZE_CLEAN_BASE_BINARY_SHA256"]
    if hashlib.sha256(binary.read_bytes()).hexdigest() != expected_hash:
        raise RuntimeError("frozen base binary differs from its verified build hash")
    candidate_sha = git("rev-parse", "HEAD")
    env = dict(os.environ, CARGO_BUILD_JOBS="2", GAZE_DISABLE_OS_KEYSTORE="1")
    subprocess.run(["cargo", "build", "--locked", "--release", "-p", "gaze-cli",
                    "--features", "mcp"], cwd=ROOT, env=env, check=True)
    subprocess.run([
        sys.executable, "scripts/verify_clean_equality.py",
        "--base-bin", str(binary), "--candidate-bin", "target/release/gaze",
        "--base-sha", main_sha, "--candidate-sha", candidate_sha,
        "--output", "target/field-clean-equality-final.json",
    ], cwd=ROOT, env=env, check=True)
    if (git("rev-parse", "HEAD") != candidate_sha
            or git("rev-parse", "origin/main") != main_sha
            or hashlib.sha256(binary.read_bytes()).hexdigest() != expected_hash):
        raise RuntimeError("comparison inputs changed during the check")


if __name__ == "__main__":
    main()
