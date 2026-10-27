#!/usr/bin/env python3
"""Mutate issuer boundaries and private-key framing; require behavioral failures.

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
BOUNDARY_TEST = "issuer_boundaries_fail_closed_but_generic_prefixes_need_attribution"
PEM_TEST = "truncated_mismatched_and_framed_private_keys_are_whole_and_restorable"
PROBES = [
    ("bare-disabled", "security_token.bare", "enabled = true", "enabled = false", "bare_vendor_prefixes_are_whole_restorable_credentials"),
    ("pem-disabled", "security_token.pem", "enabled = true", "enabled = false", "private_pem_blocks_include_headers_body_and_footer"),
    ("start-boundary", "security_token.bare", r"\A|", "", BOUNDARY_TEST),
    ("end-boundary", "security_token.bare", r"\z|", "", BOUNDARY_TEST),
    ("ascii-boundary", "security_token.bare", r"[^A-Za-z0-9_\\-]|", "", BOUNDARY_TEST),
    ("json-boundary", "security_token.bare", r'\\(?:[ntrbf"\\]|u[0-9A-Fa-f]{4})', r'\\q', BOUNDARY_TEST),
    ("unicode-boundary", "security_token.bare", r"|u[0-9A-Fa-f]{4}", "", BOUNDARY_TEST),
    ("url-boundary", "security_token.bare", r"|%[0-9A-Fa-f]{2}", "", BOUNDARY_TEST),
    ("pem-literal-newline", "security_token.pem", r"\r?\n|", "", PEM_TEST),
    ("pem-json-newline", "security_token.pem", r"\\(?:r\\)?n|", "", PEM_TEST),
    ("pem-nested-json-newline", "security_token.pem", r"\\\\(?:r\\\\)?n|", "", PEM_TEST),
    ("pem-unicode-newline", "security_token.pem", r"\\u000[aAdD]|", "", PEM_TEST),
    ("pem-html-newline", "security_token.pem", r"(?i:<br\x20*/?>)|", "", PEM_TEST),
    ("pem-shell-newline", "security_token.pem", r"|\\\r?\n", "", PEM_TEST),
    ("pem-string-join", "security_token.pem", r"""(?:["'][\x20\t]*\+?[\x20\t]*(?:\r?\n[\x20\t]*)?["'])?""", "", PEM_TEST),
    ("pem-comment", "security_token.pem", r"\#|", "", PEM_TEST),
    ("pem-slash-comment", "security_token.pem", "[A-Za-z0-9+/=]", "[A-Za-z0-9+=]", PEM_TEST),
    ("pem-quote", "security_token.pem", ">|", "", PEM_TEST),
    ("pem-star", "security_token.pem", r"|\*", "", PEM_TEST),
    ("pem-minimum", "security_token.pem", "{15}", "{0}", PEM_TEST),
    ("pem-truncation", "security_token.pem", "-----)?)", "-----))", PEM_TEST),
]


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
        for name, rule, before, after, test in PROBES:
            sections = original.decode().split("[[recognizers]]")
            selected = [i for i, section in enumerate(sections) if f'id = "{rule}"\n' in section]
            if len(selected) != 1:
                raise AssertionError(f"expected one rule {rule}")
            index = selected[0]
            if before not in sections[index]:
                raise AssertionError(f"{name}: mutation no longer identifies its target")
            sections[index] = sections[index].replace(before, after)
            RULEPACK.write_text("[[recognizers]]".join(sections))
            log = logs / (name + ".log")
            with log.open("w") as output:
                result = subprocess.run(["nice", "-n", "10", "cargo", "test", "-p", "gaze-recognizers", "--test", "bare_credentials", test, "--", "--exact"], cwd=ROOT, stdout=output, stderr=subprocess.STDOUT)
            text = log.read_text()
            if result.returncode != 101 or "whole credential ownership" not in text or "1 failed" not in text:
                raise AssertionError(f"{name}: expected behavioral failure, inspect {log}")
            print(f"{name}: mutation makes {test} fail; {log}", flush=True)
            RULEPACK.write_bytes(original)
    finally:
        RULEPACK.write_bytes(original)
    print(f"MUTATION DONE: {len(PROBES)} mutations caught; source restored")


if __name__ == "__main__":
    main()
