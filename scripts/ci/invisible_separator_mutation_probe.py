#!/usr/bin/env python3
"""Prove each committed separator mapping is required by its regression test."""
import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/gaze/src/normalize.rs"
REGEX_SOURCE = ROOT / "crates/gaze-recognizers/src/regex.rs"
MUTATIONS = (
    ("zwsp", "'\\u{200B}' | ", "zwsp_identifiers_and_email_keep_exact_source_spans"),
    ("word_joiner", " | '\\u{2060}'", "word_joiner_identifiers_keep_exact_source_spans"),
    ("bom", " | '\\u{FEFF}'", "joiners_and_bom_identifiers_keep_exact_source_spans"),
    ("soft_hyphen", "'\\u{00AD}' | ", "soft_hyphen_identifiers_keep_exact_source_spans"),
    ("nb_hyphen", "        '\\u{2011}' => '-',\n", "nonbreaking_hyphen_identifiers_keep_exact_source_spans"),
)


def main():
    committed = subprocess.check_output(["git", "show", "HEAD:crates/gaze/src/normalize.rs"], cwd=ROOT)
    original = SOURCE.read_bytes()
    if original != committed:
        raise SystemExit("Commit the normalizer before probing; source must match HEAD")
    original_regex = REGEX_SOURCE.read_bytes()
    if original_regex != subprocess.check_output(
            ["git", "show", "HEAD:crates/gaze-recognizers/src/regex.rs"], cwd=ROOT):
        raise SystemExit("Commit regex recovery before probing; source must match HEAD")
    environment = dict(os.environ, RUSTUP_TOOLCHAIN="1.96.0", CARGO_BUILD_JOBS="2")
    for name in ("rustc", "rustdoc"):
        environment[name.upper()] = subprocess.check_output(
            ["rustup", "which", "--toolchain", "1.96.0", name], text=True).strip()
    output = ROOT / "target/separator-checks/mutations"
    output.mkdir(parents=True, exist_ok=True)
    try:
        for name, mapping, test in MUTATIONS:
            source = original.decode()
            if source.count(mapping) != 1:
                raise SystemExit(f"{name}: expected exactly one mapping")
            SOURCE.write_text(source.replace(mapping, "", 1))
            result = subprocess.run(
                ["nice", "-n", "10", "cargo", "test", "-p", "gaze-recognizers", "--test",
                 "invisible_separators", test, "--", "--exact"],
                cwd=ROOT, env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            (output / f"{name}.log").write_bytes(result.stdout)
            if result.returncode != 101 or b"test result: FAILED" not in result.stdout:
                raise SystemExit(f"{name}: mutation did not fail its runtime regression; inspect {output}")
            print(f"{name}: regression rejected disabled mapping", flush=True)
            SOURCE.write_bytes(original)
        source = original_regex.decode()
        recovery_guard = "if !self.complete_labelled_value {"
        if source.count(recovery_guard) != 1:
            raise SystemExit("joined_run: expected exactly one recovery guard")
        REGEX_SOURCE.write_text(source.replace(recovery_guard, "if true {", 1))
        result = subprocess.run(
            ["nice", "-n", "10", "cargo", "test", "-p", "gaze-recognizers", "--test",
             "government_id", "labelled_identifier_boundaries_keep_value_bytes_protected", "--", "--exact"],
            cwd=ROOT, env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (output / "joined_run.log").write_bytes(result.stdout)
        if result.returncode != 101 or b"value group leaked" not in result.stdout:
            raise SystemExit(f"joined_run: recovery mutation did not fail the original regression; inspect {output}")
        print("joined_run: original regression rejected disabled recovery", flush=True)
    finally:
        SOURCE.write_bytes(original)
        REGEX_SOURCE.write_bytes(original_regex)
    print(f"MUTATION DONE: all {len(MUTATIONS)} disabled mappings and joined-run recovery rejected; source restored")


if __name__ == "__main__":
    main()
