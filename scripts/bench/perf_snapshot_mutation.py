#!/usr/bin/env python3
"""Disable three real Rust producers; require snapshot failures and exact restoration.

Run only in a clean, leased implementation/review worktree with a granted machine
slot. No timings are captured. Each Cargo command is serial, nice 10, jobs 2.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
TEST = "perf_invocation_snapshot::attribution_invocation_snapshot_covers_model_free_inventory"
MUTATIONS = (
    ("sweep-span", "crates/gaze/src/pipeline.rs",
     'let _perf = gaze_types::perf_span!("sweep_ns");',
     'let _perf = gaze_types::perf::Disabled;'),
    ("input-bytes", "crates/gaze/src/pipeline.rs",
     'gaze_types::perf_count!("input_bytes", text.len());', ''),
    ("recognizer-span", "crates/gaze/src/registry.rs",
     'let mut perf = gaze_types::perf_recognizer!(recognizer.id(), evidence);',
     'let mut perf = gaze_types::perf::Disabled;'),
)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def run_test(out, name):
    probe = subprocess.check_output(["memory_pressure", "-Q"], text=True).splitlines()[-1]
    free = int(re.search(r"(\d+)%", probe)[1])
    splash = subprocess.run(["pgrep", "-i", "splash"], stdout=subprocess.DEVNULL).returncode == 0
    floor = 20 if splash else 30
    if free < floor:
        raise RuntimeError(f"memory gate: {free}% < {floor}%")
    command = ["anvil", "run", "--", "env", "CARGO_BUILD_JOBS=2", "nice", "-n", "10",
               "cargo", "test", "--locked", "-p", "gaze-cli", "--features", "perf-timers",
               "--test", "cli_pipe", TEST, "--", "--exact", "--nocapture"]
    with (out / (name + ".log")).open("w") as log:
        run = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
    text = (out / (name + ".log")).read_text()
    return run.returncode, text, {"command": command, "memory_free_percent": free,
                                 "memory_floor_percent": floor, "exit": run.returncode}


def prove(out):
    if subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT):
        raise RuntimeError("commit all tracked changes before Rust mutations")
    out.mkdir(parents=True, exist_ok=True)
    receipt = {"source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"],
               cwd=ROOT, text=True).strip(), "test": TEST, "timed": False, "mutations": []}
    code, text, baseline = run_test(out, "baseline")
    if code or "1 passed; 0 failed" not in text:
        raise RuntimeError("baseline must execute and pass exactly one snapshot test")
    receipt["baseline"] = baseline
    for name, filename, before, after in MUTATIONS:
        path = ROOT / filename
        original = path.read_bytes()
        if original.count(before.encode()) != 1:
            raise RuntimeError(f"expected exactly one producer: {name}")
        try:
            mutated = original.replace(before.encode(), after.encode())
            path.write_bytes(mutated)
            code, text, failed = run_test(out, name)
            if (code != 101 or "0 passed; 1 failed" not in text
                    or "invocation snapshot changed:" not in text):
                raise RuntimeError(f"{name} must fail the executed snapshot assertion, not compilation")
        finally:
            path.write_bytes(original)
        code, text, restored = run_test(out, name + "-restored")
        if code or "1 passed; 0 failed" not in text or path.read_bytes() != original:
            raise RuntimeError(f"{name} did not restore and pass")
        receipt["mutations"].append({"name": name, "file": filename,
            "original_sha256": sha(original), "mutant_sha256": sha(mutated),
            "restored_sha256": sha(path.read_bytes()), "mutated": failed, "restored": restored,
            "snapshot_assertion_failed": True, "source_restored_exactly": True})
        (out / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    result = prove(args.out.resolve())
    print(f"{len(result['mutations'])} real Rust omissions refused; restored passes each")
