#!/usr/bin/env python3
"""Prove the CSV and wrapper tests fail when their corresponding fix is removed.

Run only from a clean signed commit, with no other Cargo work in this checkout.
Original bytes are restored even when a mutant fails unexpectedly. No stash or
commit rewrite is used. Logs and hashes remain in the requested output folder.
"""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if git("status", "--porcelain"):
        raise SystemExit("Commit the source and tests before mutation probing")
    head = git("rev-parse", "HEAD")
    if "\ngpgsig " not in git("cat-file", "commit", head):
        raise SystemExit("The source commit must be signed")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    mutants = [
        ("csv", "crates/gaze-recognizers/src/regex.rs",
         "if escapes == depth && run >= 2 {", "if false && escapes == depth && run >= 2 {",
         "complete_csv_and_sql_cells_keep_embedded_quote_suffixes", 1),
        ("wrappers", "crates/gaze-recognizers/src/regex.rs",
         ".filter(|ch| field_wrapper_opens(*ch))",
         ".filter(|_| false)",
         "backtick_and_typographic_wrappers_keep_complete_values", 1),
    ]
    receipt = {"revision": head, "mutants": []}
    for name, source, before, after, test, count in mutants:
        path = ROOT / source
        original = path.read_bytes()
        text = original.decode()
        assert text.count(before) == count, (name, text.count(before))
        mutant = text.replace(before, after)
        try:
            path.write_text(mutant)
            command = ["cargo", "test", "-p", "gaze-recognizers", "--test", "record_id_boundaries",
                       test, "--", "--exact", "--nocapture"]
            with (output / f"{name}.log").open("w") as log:
                result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
            log = (output / f"{name}.log").read_text()
            assert result.returncode != 0 and "running 1 test" in log and "test result: FAILED" in log, name
            assert "whole private scalar" in log or "public bytes changed" in log, name
            receipt["mutants"].append({"name": name, "file": source, "test": test,
                                      "original_sha256": hashlib.sha256(original).hexdigest(),
                                      "mutant_sha256": hashlib.sha256(mutant.encode()).hexdigest(),
                                      "exit_code": result.returncode, "test_failed_behaviorally": True})
        finally:
            path.write_bytes(original)
            assert path.read_bytes() == original
    assert not git("status", "--porcelain")
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"Both mutation probes failed behaviorally; original source restored at {head}")


if __name__ == "__main__":
    main()
