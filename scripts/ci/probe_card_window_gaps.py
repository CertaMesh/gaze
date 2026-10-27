#!/usr/bin/env python3
"""Probe card-window gap coverage using the freshly attested equality binaries."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-bin", type=Path, required=True)
    parser.add_argument("--candidate-bin", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    attestation = json.loads((ROOT / "target/separator-checks/release-build.json").read_text())
    if attestation["base_sha"] != "3fb02d70e756102c871c51471cfd76d7ce6de4bc":
        raise RuntimeError("probe requires the exact integrated main base")
    binaries = {"base": args.base_bin.resolve(strict=True), "candidate": args.candidate_bin.resolve(strict=True)}
    if any(sha256(path.read_bytes()) != attestation["binary_sha256"][side] for side, path in binaries.items()):
        raise RuntimeError("probe binaries differ from the release build attestation")
    cases = [
        ("visible-space-union", "0 4111 1111 1111 1111 003"),
        ("leading-zwj-union", "0\u200d4111 1111 1111 1111"),
        ("two-zwj-gaps", "0\u200d4111 1111 1111 1111\u200d003"),
        ("two-zwnj-gaps", "0\u200c4111 1111 1111 1111\u200c003"),
        ("two-zwsp-gaps", "0\u200b4111 1111 1111 1111\u200b003"),
    ]
    rows = []
    with tempfile.TemporaryDirectory(prefix="gaze-card-window-probe-") as cwd:
        for case_id, value in cases:
            text = "Ref " + value + " ok"
            for side, binary in binaries.items():
                print(f"CARD GAP {len(rows) + 1}/{len(cases) * 2}: {case_id}/{side}", flush=True)
                result = subprocess.run([str(binary), "clean", "--locale", "en-US"], input=text.encode(),
                                        cwd=cwd, env=dict(os.environ, GAZE_DISABLE_OS_KEYSTORE="1"),
                                        capture_output=True, timeout=60)
                if result.returncode:
                    raise RuntimeError("card-gap clean failed")
                response = json.loads(result.stdout)
                covered = any(entry["raw"] == value for entry in response["entries"])
                restored = response["clean_text"]
                for entry in response["entries"]:
                    restored = restored.replace(entry["token"], entry["raw"])
                if restored != text:
                    raise RuntimeError("card-gap manifest substitution failed to round-trip")
                if side == "candidate" and not covered:
                    raise RuntimeError(f"candidate did not protect the complete union: {case_id}")
                if case_id in ("visible-space-union", "leading-zwj-union") and not covered:
                    raise RuntimeError("positive control no longer protects its complete union")
                rows.append({"case_id": case_id, "side": side, "input_sha256": sha256(text.encode()),
                             "full_union_protected": covered, "detections": response["stats"]["detections"],
                             "manifest_substitution_round_trip": True, "exit_code": result.returncode})
    report = {"schema": "gaze-card-window-gap-probe-v1", **attestation,
              "script_sha256": sha256(Path(__file__).read_bytes()), "observations": rows}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
