#!/usr/bin/env python3
"""Build the detached probe through the canonical harness, without scoring data."""

from pathlib import Path
import sys
import time
import tomllib

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts/bench"))
from gaze_bench_score import build_validator_probe  # noqa: E402


def check_versions(root: Path) -> None:
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    versions = {}
    for member in workspace["workspace"]["members"]:
        package = tomllib.loads((root / member / "Cargo.toml").read_text())["package"]
        version = package["version"]
        if isinstance(version, dict):
            version = workspace["workspace"]["package"]["version"]
        versions[package["name"]] = version
    lock = tomllib.loads(
        (root / "scripts/bench/validator_recall_probe/Cargo.lock").read_text()
    )
    for package in lock["package"]:
        name = package["name"]
        if name in versions and package["version"] != versions[name]:
            raise ValueError(
                f"validator probe lock version drift: {name}: "
                f"probe Cargo.lock={package['version']}, workspace={versions[name]}"
            )


def main() -> None:
    started = time.monotonic()
    check_versions(ROOT)
    print("Validator probe workspace versions match", flush=True)
    binary = build_validator_probe(ROOT)
    print(f"Canonical locked validator probe build passed: {binary}", flush=True)
    print(f"Validator probe gate wall time: {time.monotonic() - started:.2f}s")


if __name__ == "__main__":
    main()
