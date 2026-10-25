#!/usr/bin/env python3
"""Kill the JSON-label ownership and shared-quote-index mutants, restoring sources."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "target/scanner-mutations")
    args = parser.parse_args()
    if git("status", "--porcelain"):
        raise SystemExit("Commit all changes before mutation probes; a clean checkout is required.")
    head = git("rev-parse", "HEAD")
    if "\ngpgsig " not in git("cat-file", "-p", head):
        raise SystemExit("Mutation probes require a signed HEAD.")
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    probes = [
        (
            "drop-json-label-ownership",
            "crates/gaze-recognizers/src/regex.rs",
            "    if let Some(scan) = index.scan_json_label_cell(input, capture.clone()) {",
            "    if let Some(scan) = None::<LabelledValueScan> {",
            "json_label_close_owns_following_value",
        ),
        (
            "rebuild-quote-events-per-query",
            "crates/gaze-recognizers/src/regex/scalar_index.rs",
            "        let index = self\n            .quotes",
            "        self.quotes.clear();\n        let index = self\n            .quotes",
            "doubled_apostrophe_prefix_work_is_linear",
        ),
    ]
    receipts = []
    for name, relative, old, new, test in probes:
        path = ROOT / relative
        original = path.read_bytes()
        source = original.decode()
        if source.count(old) != 1:
            raise SystemExit(f"Mutation anchor changed: {name}; update this probe.")
        log = out / f"{name}.log"
        try:
            path.write_text(source.replace(old, new))
            command = ["cargo", "test", "-p", "gaze-recognizers", "--lib", test, "--", "--nocapture"]
            with log.open("w") as stream:
                result = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, check=False)
            output = log.read_text()
            compiled = "Finished `test` profile" in output and "Running unittests" in output
            killed = result.returncode == 101 and compiled and f"{test} ... FAILED" in output and "test result: FAILED. 0 passed; 1 failed" in output
            receipt = dict(mutation=name, head=head, command=command, test=test, compiled=compiled, exit=result.returncode, killed=killed, original_sha256=hashlib.sha256(original).hexdigest(), log=str(log))
        finally:
            path.write_bytes(original)
            if path.read_bytes() != original:
                raise SystemExit(f"Source restoration failed: {relative}")
        receipt["restored_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        receipts.append(receipt)
        (out / "receipts.json").write_text(json.dumps(receipts, indent=2) + "\n")
        print(json.dumps(receipt), flush=True)
        if not killed:
            raise SystemExit(f"Mutant survived or did not compile: {name}")
    if git("status", "--porcelain") or git("rev-parse", "HEAD") != head:
        raise SystemExit("Checkout changed during probes.")
    print("Both mutants compiled, failed their named test, and were restored.")


if __name__ == "__main__":
    main()
