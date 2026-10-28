#!/usr/bin/env python3
"""Prove each structured producer's observer test rejects dropped field evidence.

Run from a clean, committed prepared Anvil worktree. Mutations are temporary and
restored even on failure. Output reports test status, never fixture or field data.
"""
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
PROBES = [
    ("legacy", "crates/gaze-proxy/src/server.rs", "gaze-proxy",
     "field_provenance_legacy_adapters_reaches_detect_context",
     "Some(field) => pseudonymizer.protect_field(surface.text, field)",
     "Some(_field) => pseudonymizer.protect(surface.text)"),
    ("codec", "crates/gaze-proxy/src/codecs/anthropic.rs", "gaze-proxy",
     "field_provenance_anthropic_codec_reaches_detect_context",
     "value.value = ctx.protect_field(&value.value, &field)?;",
     "value.value = ctx.protect(&value.value)?;"),
    ("mcp", "crates/gaze-mcp-core/src/dispatch.rs", "gaze-mcp-core",
     "field_provenance_mcp_reaches_detect_context",
     "context.with_field(&field)", "context"),
]


def run_test(package, test, env):
    result = subprocess.run(
        ["nice", "-n", "10", "cargo", "test", "-p", package, "--lib", test],
        cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        check=False,
    )
    return result.returncode, result.stdout.decode(errors="replace")


def main():
    dirty = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT)
    if dirty:
        raise RuntimeError("commit tracked changes before running mutations")
    env = os.environ.copy()
    env["CARGO_BUILD_JOBS"] = "2"
    env["GAZE_DISABLE_OS_KEYSTORE"] = "1"
    reports = []
    for name, source, package, test, original, mutation in PROBES:
        path = ROOT / source
        saved = path.read_bytes()
        text = saved.decode()
        if text.count(original) != 1:
            raise RuntimeError(f"{name}: mutation anchor must be unique")
        code, output = run_test(package, test, env)
        if code or "1 passed" not in output:
            raise RuntimeError(f"{name}: baseline test did not pass")
        try:
            path.write_text(text.replace(original, mutation))
            code, output = run_test(package, test, env)
            killed = code != 0 and "1 failed" in output and "field evidence must reach both recognizer passes" in output
            if not killed:
                raise RuntimeError(f"{name}: mutation was not rejected by the observer assertion")
        finally:
            path.write_bytes(saved)
        code, output = run_test(package, test, env)
        if code or "1 passed" not in output:
            raise RuntimeError(f"{name}: restored producer did not pass")
        reports.append({"producer": name, "baseline": "pass", "mutation": "killed", "restored": "pass"})
        print(json.dumps(reports[-1]), flush=True)
    report = {"schema": "field-provenance-mutations-v1", "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(), "probes": reports}
    output = ROOT / "target/field-provenance-mutations.json"
    output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
