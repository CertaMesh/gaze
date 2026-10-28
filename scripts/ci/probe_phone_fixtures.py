#!/usr/bin/env python3
"""Measure the same phone fixtures on pinned pre-merge and main worktrees."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
HEADS = {
    "pre_merge": "9e87f4df7bef7b23db1824c8224d2fe78dc37bb3",
    "main": "3b3808c66a528a308b293b5d6bfd1e4924a637c1",
    "integrated": "c09fbf4345b6a34029d63ba425d2aa62879e382c",
}
PROBE = r'''
use gaze_types::ValidatorKind;
#[test]
fn pinned_phone_fixture_results() {
    let result = serde_json::json!({
        "uk_e164": ValidatorKind::E164Phone.validates("+44 7700 900123"),
        "uk_compact_e164": ValidatorKind::E164Phone.validates("+447700900123"),
        "de_e164": ValidatorKind::E164Phone.validates("+49 171 3920000"),
        "us_e164": ValidatorKind::E164Phone.validates("+1 202 555 0100"),
        "uk_phone_number": ValidatorKind::PhoneNumber.validates("+44 7700 900123"),
    });
    println!("PHONE_FIXTURE_RESULT={result}");
}
'''


def identity(sha):
    lock = subprocess.check_output(["git", "show", sha + ":Cargo.lock"], cwd=ROOT)
    packages = tomllib.loads(lock.decode())["package"]
    phones = [item for item in packages if "phone" in item["name"]]
    source = subprocess.check_output(["git", "show", sha + ":crates/gaze-types/src/lib.rs"], cwd=ROOT, text=True)
    helper = re.search(r"fn e164_phone_check\(input: &str\) -> bool \{\n.*?\n\}", source, re.S).group()
    return {"sha": sha, "phone_packages": phones,
            "version_build_metadata": {item["name"]: item["version"].partition("+")[2] for item in phones},
            "separate_phone_metadata_packages": [item for item in phones if "metadata" in item["name"]],
            "e164_helper_sha256": hashlib.sha256(helper.encode()).hexdigest()}


def main():
    output = ROOT / "target/separator-checks/phone-fixtures"
    output.mkdir(parents=True, exist_ok=True)
    results = {}
    for index, side in enumerate(("pre_merge", "main"), 1):
        worktree = Path(os.environ["GAZE_PHONE_" + side.upper() + "_WORKTREE"]).resolve(strict=True)
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=worktree, text=True).strip()
        if head != HEADS[side] or subprocess.check_output(["git", "status", "--porcelain"], cwd=worktree).strip():
            raise RuntimeError("probe checkout must be clean and pinned to the requested head")
        if shutil.disk_usage(ROOT).free < 100 * 1024**3:
            raise RuntimeError("disk free space is below 100 GiB")
        target = worktree / "crates/gaze-types/tests/phone_fixture_probe.rs"
        if target.exists():
            raise RuntimeError("refusing to overwrite an existing test")
        print(f"PHONE FIXTURES {index}/2: {side} {head}", flush=True)
        try:
            target.write_text(PROBE)
            result = subprocess.run(["anvil", "run", str(worktree), "--", "env", "CARGO_BUILD_JOBS=2",
                                     "GAZE_DISABLE_OS_KEYSTORE=1", "nice", "-n", "10", "cargo", "test",
                                     "--locked", "-p", "gaze-types", "--features", "phone-parser",
                                     "--test", "phone_fixture_probe", "--", "--nocapture"], cwd=worktree,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            (output / (side + ".log")).write_bytes(result.stdout)
            print(result.stdout.decode(), flush=True)
            if result.returncode:
                raise RuntimeError("phone fixture probe failed to execute")
            line = next(line for line in result.stdout.decode().splitlines() if line.startswith("PHONE_FIXTURE_RESULT="))
            results[side] = json.loads(line.partition("=")[2])
        finally:
            target.unlink(missing_ok=True)
        size = int(subprocess.check_output(["du", "-sk", str(worktree / "target")], text=True).split()[0]) * 1024
        if size > 15 * 1024**3:
            raise RuntimeError("probe target exceeds 15 GiB")
    report = {"schema": "gaze-phone-fixture-precondition-v1", "heads": {side: identity(sha) for side, sha in HEADS.items()},
              "probe_sha256": hashlib.sha256(PROBE.encode()).hexdigest(), "results": results,
              "same_results": results["pre_merge"] == results["main"]}
    (output.parent / "phone-fixtures.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report), flush=True)
    if not report["same_results"]:
        raise RuntimeError("phone validation differs between pinned heads; do not change the fixture")


if __name__ == "__main__":
    main()
