#!/usr/bin/env python3
"""Capture the term-normalization cells once; replay every scored-label contract.

This auxiliary known-record arm supplements the full C/A/D/R gain gate.
Its observation record uses the standard single-population replay format;
`arm.layer` identifies the generated K population, not holdout C.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/"scripts/bench"))
import gaze_bench_score as score
import known_record_cells as cells
import scorecard_record as records
import subject_build


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--subject", type=Path, required=True)
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--model-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    subject, policy, output = args.subject.resolve(), args.policy.resolve(), args.output_dir.resolve()
    if score.git_metadata(subject)["dirty"]:
        raise SystemExit("subject must be clean before capture")
    pairs = [p for p in cells.generate() if p.positive.variant.startswith("term_")]
    documents, contexts = cells.documents(ROOT, "v1", pairs)
    if not pairs or any(score.VALIDATOR_CLASSES_BY_LABEL.get(span.label) for doc in documents for span in doc.spans):
        raise SystemExit("normalization cells require an updated validator measurement path")
    corpus_sha = hashlib.sha256(cells.corpus_bytes(pairs)).hexdigest()
    # These synthetic passport values have no checksum validator. Keep the
    # complete explicit inventory required by the standard observation replay.
    measurements = {
        "validator_kinds_by_class": {},
        "documents": {doc.uid: {
            "gold_validation": [{"start": s.start, "end": s.end, "label": s.label,
                                 "applicable": False, "validator_passed": None} for s in doc.spans],
            "predictions": None,
        } for doc in documents},
    }
    binary, receipt = subject_build.build(subject, ["policy-file"], release=True,
                                         builder=subject_build.cargo_build_binary)
    output.mkdir(parents=True, exist_ok=True)
    writer = records.RecordWriter(documents, measurements, corpus_sha256=corpus_sha, binary=binary)
    transport = score.BenchSubprocess

    class ContextTransport(transport):
        def exchange(self, request):
            return super().exchange({**request, "context_json": contexts[request["fixture_id"]]})

    environment = dict(os.environ, GAZE_BENCH_KNOWN_RECORD_ARM="1")
    with patch.object(score, "BenchSubprocess", ContextTransport):
        run = score.run_config(subject, binary, "policy-file", documents, args.model_dir.resolve(),
                               None, None, None, 0.3, output/"logs", base_environment=environment,
                               policy_path=policy, validator_measurements=measurements,
                               record_document=lambda config, doc, response, measured: writer.add(
                                   "C", config, doc, response, measured))
    # Both subjects use this exact harness and cell inventory. The provenance
    # hash is supplemental; the full release runner supplies main certification.
    inputs = {str(path.relative_to(ROOT)): score.sha256_file(path) for path in (
        Path(__file__), ROOT/"scripts/bench/known_record_cells.py",
        ROOT/"scripts/bench/known_record_attribution.py", ROOT/"scripts/bench/gaze_bench_score.py",
        ROOT/"scripts/bench/scorecard_record.py",
    )}
    arm = {"layer": "K", "generator": cells.manifest(pairs), "harness": inputs,
           "policy_sha256": score.sha256_file(policy), "capture_once": True}
    arm["specification_sha256"] = hashlib.sha256(json.dumps(arm, sort_keys=True).encode()).hexdigest()
    card = {
        "schema_version": 4, "generated_at": "record-once",
        "gaze": score.git_metadata(subject), "subject_build": receipt, "arm": arm,
        "dataset": {"integrity": {"sha256": corpus_sha},
                    "validator_gold_census": score.validator_gold_census(documents, measurements)},
        "scoring": {"scored_label_contract": score.scored_label_contract_report(score.SCORED_LABEL_CONTRACT_V1, documents)},
        "parameters": {"ner_threshold": 0.3, "configs": ["policy-file"], "binary_profile": "release"},
        "runs": [run],
    }
    record_path = output/"observations-v1.jsonl.gz"
    card["observation_record"] = writer.write(record_path, card, add_reference=True)
    (output/"capture.json").write_text(json.dumps(card, indent=2) + "\n")
    for version in (1, 2):
        contract = score.SCORED_LABEL_CONTRACT_V1 if version == 1 else score.load_scored_label_contract(ROOT/"docs/reference/benchmarks/scored-labels-v2.json")
        replayed = records.rescore(record_path, contract, max_workers=1)
        (output/f"scorecard-v{version}.json").write_text(json.dumps(replayed, indent=2) + "\n")
    print(json.dumps({"observations": len(writer.rows), "pairs": len(pairs), "record_sha256": card["observation_record"]["sha256"]}))


if __name__ == "__main__":
    main()
