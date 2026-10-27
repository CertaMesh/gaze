#!/usr/bin/env python3
"""Prove each committed separator mapping is required by its regression test."""
import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/gaze/src/normalize.rs"
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
    finally:
        SOURCE.write_bytes(original)
    print(f"MUTATION DONE: all {len(MUTATIONS)} disabled mappings rejected; original source restored")


if __name__ == "__main__":
    main()
