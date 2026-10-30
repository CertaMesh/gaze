#!/usr/bin/env python3
"""Build the pinned separator equality base and candidate under one Anvil check lock."""
import argparse
import card_code_matrix as matrix
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
BASE_SHA = "3fb02d70e756102c871c51471cfd76d7ce6de4bc"


def resources(base):
    free = shutil.disk_usage(ROOT).free
    if free < 100 * 1024**3:
        raise RuntimeError("disk free space is below the 100 GiB floor")
    for root in (base, ROOT):
        target = root / "target"
        size = int(subprocess.check_output(["du", "-sk", str(target)], text=True).split()[0]) * 1024 if target.exists() else 0
        if size > 15 * 1024**3:
            raise RuntimeError("a build target exceeds the 15 GiB cap")
        print(f"RESOURCE target={size / 1024**3:.2f}/15 GiB free={free / 1024**3:.2f}/100 GiB", flush=True)



def valid_card_code_report(data, build):
    if (data.get("schema") != "gaze-card-code-boundary-matrix-v1"
            or data.get("base_sha") != BASE_SHA
            or data.get("binary_sha256") != build["binary_sha256"]
            or data.get("script_sha256") != hashlib.sha256(Path(matrix.__file__).read_bytes()).hexdigest()
            or data.get("visible_identity_exceptions") != matrix.VISIBLE_IDENTITY_EXCEPTIONS):
        return False
    expected = [(family, pan, code, kind, name, sep)
                for family, pan in matrix.pans() for code in ("123", "1234")
                for kind, name, sep in matrix.separators()]
    rows = data.get("observations", [])
    if data.get("cells") != len(expected) or len(rows) != len(expected):
        return False
    reference = None
    approved = []
    for row, (family, pan, code, kind, name, sep) in zip(rows, expected):
        case_id = f"{family}/pan-{len(pan)}/code-{len(code)}/{name}"
        before, after = row.get("base_digit_coverage", ""), row.get("candidate_digit_coverage", "")
        if (row.get("case_id") != case_id
                or row.get("input_sha256") != hashlib.sha256(("Card: " + pan + sep + code + " done").encode()).hexdigest()
                or len(before) != len(pan + code) or len(after) != len(before)
                or set(before + after) - {"0", "1"}
                or not matrix.contains(after, before)
                or row.get("manifest_substitution_round_trip") is not True):
            return False
        if name == "space-u0020":
            reference = before
        if row.get("base_space_digit_coverage") != reference:
            return False
        if kind == "removed" and not matrix.contains(after, reference):
            return False
        if row.get("canonical_output_equal") is not True:
            if kind != "removed" and name not in matrix.VISIBLE_IDENTITY_EXCEPTIONS:
                return False
            if kind != "removed":
                approved.append(case_id)
    return (data.get("coverage_regressions") == [] and data.get("removed_space_losses") == []
            and data.get("visible_output_differences") == []
            and data.get("approved_visible_output_differences") == approved)


def reuse_card_codes(build):
    path = ROOT / "target/separator-checks/card-code-matrix.json"
    if not path.exists():
        return False
    data = json.loads(path.read_text())
    if not valid_card_code_report(data, build):
        return False
    proof = {"schema": "gaze-card-code-reuse-v1", "candidate_sha": build["candidate_sha"],
             "measured_source_sha": data["candidate_source_sha"],
             "report_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
             "binary_sha256": build["binary_sha256"], "cells_revalidated": data["cells"]}
    (path.parent / "card-code-reuse.json").write_text(json.dumps(proof, indent=2) + "\n")
    print(f"CARD CODE REUSE {data['cells']}/{data['cells']}: exact binary and script hashes; all cells revalidated", flush=True)
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--card-codes-only", action="store_true")
    args = parser.parse_args()
    base = Path(os.environ["GAZE_CLEAN_BASE_WORKTREE"]).resolve(strict=True)
    context = json.loads(subprocess.check_output(["anvil", "context", str(base), "--json"], text=True, cwd=base))["data"]
    if not context["ready"] or not context["worktree"]["detached"]:
        raise RuntimeError("base must be a prepared detached Anvil review checkout")
    if context["worktree"]["head"] != BASE_SHA or context["metadata"]["pinned_commit"] != BASE_SHA:
        raise RuntimeError("base must be pinned to the integrated main commit")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=base, text=True).strip():
        raise RuntimeError("base checkout must be clean")
    resources(base)
    print("RELEASE 0/6: building pinned main base", flush=True)
    subprocess.run(["anvil", "run", str(base), "--", "env", "CARGO_BUILD_JOBS=2", "GAZE_DISABLE_OS_KEYSTORE=1",
                    "nice", "-n", "10", "cargo", "build", "--locked", "--release", "-p", "gaze-cli"], cwd=base, check=True)
    resources(base)
    print("RELEASE 1/6: building candidate with identical default features", flush=True)
    subprocess.run(["nice", "-n", "10", "cargo", "build", "--locked", "--release", "-p", "gaze-cli"], cwd=ROOT, check=True)
    resources(base)
    print("RELEASE 2/6: recording candidate source-regex compilation work", flush=True)
    subprocess.run(["nice", "-n", "10", "cargo", "test", "--release", "-p", "gaze-recognizers", "--features", "test-support",
                    "--test", "source_regex_cold_work", "--", "--nocapture"], cwd=ROOT, check=True)
    resources(base)
    report = {"base_sha": BASE_SHA,
              "candidate_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "features": "gaze-cli default",
              "binary_sha256": {side: hashlib.sha256((root / "target/release/gaze").read_bytes()).hexdigest()
                                for side, root in (("base", base), ("candidate", ROOT))}}
    output = ROOT / "target/separator-checks/release-build.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + "\n")
    print("RELEASE 3/6: probing the shared card-window gap on main and candidate", flush=True)
    subprocess.run(["nice", "-n", "10", sys.executable, "scripts/ci/probe_card_window_gaps.py",
                    "--base-bin", str(base / "target/release/gaze"),
                    "--candidate-bin", str(ROOT / "target/release/gaze"),
                    "--output", "target/separator-checks/card-window-main-probe.json"], cwd=ROOT, check=True)
    resources(base)
    print("RELEASE 4/6: card/code coverage and visible-layout identity against fresh main", flush=True)
    if args.card_codes_only or not reuse_card_codes(report):
        subprocess.run(["nice", "-n", "10", sys.executable, "scripts/ci/card_code_matrix.py",
                        "--base-bin", str(base / "target/release/gaze"),
                        "--candidate-bin", str(ROOT / "target/release/gaze"),
                        "--output", "target/separator-checks/card-code-matrix.json"], cwd=ROOT, check=True)
    resources(base)
    if args.card_codes_only:
        print("RELEASE 6/6: card/code gate complete; clean equality queued separately", flush=True)
        return
    print("RELEASE 5/6: comparing all pinned clean inputs under both policy modes", flush=True)
    subprocess.run(["nice", "-n", "10", sys.executable, "scripts/verify_clean_equality.py",
                    "--base-bin", str(base / "target/release/gaze"),
                    "--candidate-bin", str(ROOT / "target/release/gaze"),
                    "--base-sha", BASE_SHA, "--candidate-sha", report["candidate_sha"],
                    "--output", "target/separator-checks/clean-equality-round2.json"], cwd=ROOT, check=True)
    resources(base)
    print("RELEASE 6/6: release, main-gap, card/code and clean-equality evidence complete", flush=True)


if __name__ == "__main__":
    main()
