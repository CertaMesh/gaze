#!/usr/bin/env python3
"""Drop one normalization path at a time and require a behavioral test failure."""
from __future__ import annotations

import os
from pathlib import Path
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
PROBES = (
    ("dictionary_terms", "crates/gaze-types/src/lib.rs",
     ".map(|term| normalize::normalize(&term).text)", ".map(|term| term)",
     "gaze-assembly", "core_pipeline_dictionaries", "normalized_rulepack_inline_and_file_terms_round_trip_source_bytes"),
    ("record_terms", "crates/gaze/src/context.rs",
     "gaze_types::normalize::normalize(value)\n        .text\n        .split_whitespace()", "value.split_whitespace()",
     "gaze-assembly", "record_context", "normalized_short_records_still_refuse_before_assembly"),
    ("private_guesses", "crates/gaze-recognizers/src/record_guess.rs",
     "RecordGuessText::new(term.value)", "RecordGuessText::from_normalized(term.value.to_owned())",
     "gaze-mcp-core", "strict_contract", "host_context::normalized_host_dictionary_terms_protect_response_and_preserve_guess_exclusion"),
    ("dictionary_overlaps", "crates/gaze-recognizers/src/dictionary.rs",
     ".find_overlapping_iter(input)", ".find_iter(input)",
     "gaze-assembly", "record_context", "normalized_prefix_terms_cannot_hide_existing_longer_dictionary_values"),
    ("folded_overlaps", "crates/gaze-recognizers/src/dictionary.rs",
     ".find_overlapping_iter(&folded)", ".find_iter(&folded)",
     "gaze-recognizers", None, "dictionary::normalization_tests::normalized_prefix_cannot_hide_a_folded_longer_value"),
    ("session_sweep", "crates/gaze/src/sweep.rs",
     "let normalized = normalize(&source.raw).text;", "let normalized = source.raw.clone();",
     "gaze-assembly", "record_context", "normalized_known_terms_protect_original_bytes_and_session_repeats"),
)


def resource_preflight():
    while True:
        result = subprocess.check_output(["memory_pressure"], text=True)
        free = re.search(r"System-wide memory free percentage: (\d+)%", result)
        if not free:
            raise RuntimeError("could not read free memory percentage")
        if int(free[1]) >= 30 and not (Path.home()/".cache/root-coordinator/teyos-turn.active").exists():
            return
        print("Waiting for shared build resources", flush=True)
        time.sleep(15)


def main():
    output = ROOT/"target/known-record-checks/mutations"
    output.mkdir(parents=True, exist_ok=True)
    originals = {}
    for _, relative, *_ in PROBES:
        source = ROOT/relative
        original = source.read_bytes()
        committed = subprocess.check_output(["git", "show", f"HEAD:{relative}"], cwd=ROOT)
        if original != committed:
            raise SystemExit(f"Commit {relative} before mutation probing")
        originals[source] = original
    environment = dict(os.environ, CARGO_BUILD_JOBS="4", RUSTUP_TOOLCHAIN="1.96.0", GAZE_DISABLE_OS_KEYSTORE="1")
    for name, relative, before, after, package, binary, test in PROBES:
        source = ROOT/relative
        original = originals[source]
        text = original.decode()
        if text.count(before) != 1:
            raise SystemExit(f"{name}: mutation site is not unique")
        resource_preflight()
        target = ["--test", binary] if binary else ["--lib"]
        print(f"MUTATION {name}", flush=True)
        try:
            source.write_text(text.replace(before, after, 1))
            result = subprocess.run(
                ["nice", "-n", "10", "cargo", "test", "-p", package, *target, test, "--", "--exact"],
                cwd=ROOT, env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            )
            (output/f"{name}.log").write_bytes(result.stdout)
            expected = f"test {test} ... FAILED".encode()
            if result.returncode != 101 or expected not in result.stdout or b"test result: FAILED" not in result.stdout:
                raise SystemExit(f"{name}: behavioral assertion did not reject mutation; inspect {output/name}.log")
            print(f"REJECTED {name}", flush=True)
        finally:
            source.write_bytes(original)
    print(f"MUTATIONS PASSED: {len(PROBES)}")


if __name__ == "__main__":
    main()
