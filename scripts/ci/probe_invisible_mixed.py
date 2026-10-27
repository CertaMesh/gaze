#!/usr/bin/env python3
"""Probe mixed invisible boundaries without printing values or session secrets."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source-sha", required=True)
    args = parser.parse_args()
    values = (("card", "4111111111111111", 4), ("iban", "GB82WEST12345698765432", 8),
              ("eth", "0x0000000000000000000000000000000000000000", 12),
              ("email", "cedar@example.invalid", 3), ("phone", "+12025550100", 5))
    rows = []
    binary = args.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="gaze-invisible-mixed-") as cwd:
        for separator in (0x200B, 0x34F, 0x2067):
            sep = chr(separator)
            for family, value, at in values:
                value = value[:at] + sep + value[at:]
                text = "word" + sep + value + sep + "word; done"
                result = subprocess.run([str(binary), "clean", "--locale", "en-US"],
                                        input=text, text=True, capture_output=True, cwd=cwd,
                                        env=dict(os.environ, GAZE_DISABLE_OS_KEYSTORE="1"), timeout=60)
                if result.returncode:
                    raise RuntimeError(f"probe failed: {family}/{separator:x}, exit {result.returncode}")
                data = json.loads(result.stdout)
                rows.append({"family": family, "separator": f"{separator:x}",
                             "unchanged": data["clean_text"] == text,
                             "whole_value_raw": value in data["clean_text"],
                             "tokens": len(data["entries"])})
    report = {"source_sha": args.source_sha, "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "cases": rows}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
