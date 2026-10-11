#!/usr/bin/env python3
"""Reproduce the ordinary gate and the unresolved extension attachment binding."""
import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(ROOT / "scripts/bench"))
import agentic_layers as agentic
import dictionary_sequences as dictionary
import gaze_bench_score as score
import scorecard_record as records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    evidence = Path(__file__).resolve().parent
    args.output.mkdir(parents=True, exist_ok=True)
    for version in (1, 2, 4):
        contract = (score.SCORED_LABEL_CONTRACT_V1 if version == 1 else
                    score.load_scored_label_contract(ROOT / f"docs/reference/benchmarks/scored-labels-v{version}.json"))
        cards = {}
        extension_counts = {}
        for side in ("base", "candidate"):
            card = records.rescore(evidence / f"{side}-observations.jsonl.gz", contract, max_workers=4)
            extension = dictionary.replay(evidence / f"{side}-dictionary-sequence", contract)
            try:
                dictionary.attach(card, extension)
            except ValueError as error:
                if str(error) != "extension contract differs from ordinary layers":
                    raise
            else:
                raise AssertionError("The recorded attachment blocker no longer reproduces")
            cards[side] = card
            extension_counts[side] = {
                arm: {"bytes": run["metrics"]["utf8_bytes"],
                      "availability": run["pipeline_availability"],
                      "contract": run["pipeline_contract"]}
                for arm, run in extension["runs"].items()
            }
        result = {"workflow_status": "blocked",
                  "attachment_error": "extension contract differs from ordinary layers",
                  "ordinary_gate": agentic.gate(cards["base"], cards["candidate"]),
                  "dictionary_counts": extension_counts}
        (args.output / f"v{version}.json").write_text(json.dumps(result, indent=2) + "\n")
        print(f"v{version}: combined gate blocked; ordinary gate {result['ordinary_gate']['verdict']}")


if __name__ == "__main__":
    main()
