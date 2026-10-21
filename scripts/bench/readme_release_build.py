#!/usr/bin/env python3
"""Build signed-tag checkouts and retain the README measurement build receipts.

Run in a durable terminal before readme_byte_measurements.py. Each runtime
must already be a clean, isolated checkout of its signed release tag.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess

from readme_byte_measurements import digest
from tagged_gaze import tag_commit


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--v017-root", type=Path, required=True)
    parser.add_argument("--v016-root", type=Path, help="also build v0.16.0")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.v017_root = args.v017_root.resolve()
    args.v016_root = args.v016_root.resolve() if args.v016_root else None
    args.output.mkdir(parents=True, exist_ok=True)
    toolbin = Path(subprocess.check_output(["rustup", "which", "--toolchain", "1.96.0", "rustc"], text=True).strip()).parent
    environment = dict(os.environ, PATH=str(toolbin) + ":" + os.environ["PATH"],
                       RUSTC=str(toolbin / "rustc"), RUSTDOC=str(toolbin / "rustdoc"), CARGO_BUILD_JOBS="4")
    receipts = {}
    for key, tag, root in (("v017", "v0.17.0", args.v017_root), ("v016", "v0.16.0", args.v016_root)):
        if root is None:
            continue
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
        if revision != tag_commit(tag) or subprocess.check_output(["git", "status", "--porcelain"], cwd=root):
            raise ValueError("runtime must be a clean checkout of its release tag")
        subprocess.run(["git", "tag", "-v", tag], cwd=root, check=True)
        policy = args.output / (key + ".toml")
        commands = [["cargo", "build", "--release", "--locked", "-p", "gaze-cli"],
                    ["cargo", "build", "--release", "--locked", "-p", "gaze-recognizers", "--example", "clean_for_bench", "--features", "safety-net-nym"],
                    [str(root / "target/release/gaze"), "setup", "--non-interactive", "--force", "--policy-out", str(policy)]]
        for index, command in enumerate(commands):
            print(key, index, "started", flush=True)
            with (args.output / f"{key}-build-{index}.log").open("w") as log:
                subprocess.run(command, cwd=root, env=environment, stdout=log, stderr=subprocess.STDOUT, check=True)
        receipts[key] = {"revision": revision, "commands": commands, "hardware": platform.platform(),
                         "binary_sha256": digest(root / "target/release/examples/clean_for_bench"), "policy_sha256": digest(policy)}
        (args.output / "build-receipts.json").write_text(json.dumps(receipts, indent=2) + "\n")


if __name__ == "__main__":
    main()
