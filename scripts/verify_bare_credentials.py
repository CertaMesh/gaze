#!/usr/bin/env python3
"""Disable each bare-credential rule and require its behavioral test to fail.

Run from a committed checkout with RUSTC/RUSTDOC and CARGO_BUILD_JOBS set.
The original rulepack bytes are restored even when Cargo or an assertion fails.
"""
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
RULEPACK = ROOT / "crates/gaze-recognizers/embedded/secrets.toml"
TEST = "crates/gaze-recognizers/tests/bare_credentials.rs"
PROBES = {
    "security_token.bare": "bare_vendor_prefixes_are_whole_restorable_credentials",
    "security_token.pem": "private_pem_blocks_include_headers_body_and_footer",
}


def main():
    if os.environ.get("CARGO_BUILD_JOBS") != "2":
        raise SystemExit("Set CARGO_BUILD_JOBS=2 before running mutation probes")
    subprocess.run(["git", "diff", "--exit-code", "HEAD", "--", str(RULEPACK), TEST], cwd=ROOT, check=True)
    commit = subprocess.check_output(["git", "cat-file", "-p", "HEAD"], cwd=ROOT)
    if b"\ngpgsig " not in commit:
        raise SystemExit("Commit the implementation with a signature before probing")
    original = RULEPACK.read_bytes()
    logs = Path(tempfile.mkdtemp(prefix="gaze-bare-credential-mutations-"))
    try:
        for rule, test in PROBES.items():
            sections = original.decode().split("[[recognizers]]")
            selected = [i for i, section in enumerate(sections) if f'id = "{rule}"\n' in section]
            if len(selected) != 1:
                raise AssertionError(f"expected one rule {rule}")
            index = selected[0]
            if sections[index].count("enabled = true") != 1:
                raise AssertionError(f"expected enabled rule {rule}")
            sections[index] = sections[index].replace("enabled = true", "enabled = false", 1)
            RULEPACK.write_text("[[recognizers]]".join(sections))
            log = logs / (rule + ".log")
            with log.open("w") as output:
                result = subprocess.run(["nice", "-n", "10", "cargo", "test", "-p", "gaze-recognizers", "--test", "bare_credentials", test, "--", "--exact"], cwd=ROOT, stdout=output, stderr=subprocess.STDOUT)
            text = log.read_text()
            if result.returncode != 101 or "whole credential ownership" not in text or "1 failed" not in text:
                raise AssertionError(f"{rule}: expected behavioral failure, inspect {log}")
            print(f"{rule}: disabled rule makes {test} fail; {log}", flush=True)
            RULEPACK.write_bytes(original)
    finally:
        RULEPACK.write_bytes(original)
    print("MUTATION DONE: both rule-disable probes failed as expected; source restored")


if __name__ == "__main__":
    main()
