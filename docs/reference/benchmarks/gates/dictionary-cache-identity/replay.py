#!/usr/bin/env python3
"""Replay retained ordinary and dictionary observations through the combined gate."""
import argparse
import gzip
import json
import subprocess
import sys
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repo", type=Path)
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    root = args.repo or Path(__file__).resolve().parents[5]
    evidence = args.evidence or Path(__file__).resolve().parent
    sys.path.insert(0, str(root / "scripts/bench"))
    import dictionary_sequences as dictionary
    import gaze_bench_score as score
    import scorecard_record as records
    args.output.mkdir(parents=True, exist_ok=True)
    reports = {}
    for version in (1, 2, 4):
        contract = (score.SCORED_LABEL_CONTRACT_V1 if version == 1 else
                    score.load_scored_label_contract(
                        root / f"docs/reference/benchmarks/scored-labels-v{version}.json",
                        display_path=f"docs/reference/benchmarks/scored-labels-v{version}.json"))
        paths = {}
        for side in ("base", "candidate"):
            record = (f"{side}-observations-v1.jsonl.gz" if version == 1 else f"{side}-observations.jsonl.gz")
            card = records.rescore(evidence / record, contract, max_workers=4)
            extension = dictionary.replay(evidence / f"{side}-dictionary-sequence", contract)
            combined = dictionary.attach(card, extension)
            paths[side] = args.output / f"{side}-v{version}.json"
            paths[side].write_text(json.dumps(combined, indent=2) + "\n")
            (args.output / f"{side}-v{version}.json.gz").write_bytes(
                gzip.compress(paths[side].read_bytes(), mtime=0))
            (args.output / f"{side}-dictionary-v{version}.json.gz").write_bytes(
                gzip.compress((json.dumps(extension, indent=2) + "\n").encode(), mtime=0))
        report = args.output / f"gate-v{version}.json"
        result = subprocess.run([sys.executable, str(root / "scripts/bench/agentic_layers.py"),
                                 "gate", "--base", str(paths["base"]), "--candidate", str(paths["candidate"]),
                                 "--output", str(report)], cwd=root, check=False)
        reports[f"v{version}"] = json.loads(report.read_text())
        print(f"v{version}: combined gate {reports[f'v{version}']['verdict']} (exit {result.returncode})")
        if result.returncode != 0 or reports[f"v{version}"]["verdict"] != "pass":
            raise SystemExit(result.returncode or 1)
    (args.output / "gate.json").write_text(json.dumps(reports, indent=2) + "\n")


if __name__ == "__main__":
    main()
