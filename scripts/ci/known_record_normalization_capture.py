#!/usr/bin/env python3
"""Capture all known-record cells once per profile; replay every scored-label contract.

The `default` profile uses the gate policy and writes to the output directory.
The `core` profile (`known_record_cells.CORE_PROFILE_POLICY`: bundled `core`,
no NER or safety net) writes to `<output>/core`. Each profile is gated on its own.

This auxiliary known-record arm supplements the full C/A/D/R gain gate.
Its observation record uses the standard single-population replay format;
`arm.layer` identifies the generated K population, not holdout C.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
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
import run_no_opf_benchmark as benchmark


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
    pairs = cells.generate()
    documents, contexts = cells.documents(ROOT, "v1", pairs)
    if not pairs:
        raise SystemExit("empty known-record layer")
    corpus_sha = hashlib.sha256(cells.corpus_bytes(pairs)).hexdigest()
    probe = benchmark.build_validator_probe(ROOT, release=True)
    measurements = score.collect_validator_measurements(probe, documents, (doc.uid for doc in documents))
    binary, receipt = subject_build.build(subject, ["policy-file"], release=True,
                                         builder=subject_build.cargo_build_binary)
    output.mkdir(parents=True, exist_ok=True)
    core_policy = output/"core-profile-policy.toml"
    core_policy.write_text(cells.CORE_PROFILE_POLICY, encoding="utf-8")
    profiles = {"default": (policy, output), "core": (core_policy, output/"core")}
    assert tuple(profiles) == cells.CAPTURE_PROFILES
    for profile, (profile_policy, profile_output) in profiles.items():
        capture(profile, subject, profile_policy, args.model_dir.resolve(), profile_output,
                pairs=pairs, documents=documents, contexts=contexts, corpus_sha=corpus_sha,
                probe=probe, measurements=measurements, binary=binary, receipt=receipt)


def capture(profile, subject, policy, model_dir, output, *, pairs, documents, contexts, corpus_sha,
            probe, measurements, binary, receipt):
    output.mkdir(parents=True, exist_ok=True)
    writer = records.RecordWriter(documents, measurements, corpus_sha256=corpus_sha, binary=binary)
    transport = score.BenchSubprocess

    class ContextTransport(transport):
        def exchange(self, request):
            return super().exchange({**request, "context_json": contexts[request["fixture_id"]]})

    environment = dict(os.environ, GAZE_BENCH_KNOWN_RECORD_ARM="1")
    with patch.object(score, "BenchSubprocess", ContextTransport):
        run = score.run_config(subject, binary, "policy-file", documents, model_dir,
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
    arm = {"layer": "K", "profile": profile, "generator": cells.manifest(pairs), "harness": inputs,
           "policy_sha256": score.sha256_file(policy), "validator_binary_sha256": score.sha256_file(probe), "capture_once": True}
    arm["specification_sha256"] = hashlib.sha256(json.dumps(arm, sort_keys=True).encode()).hexdigest()
    card = {
        "schema_version": 4, "generated_at": datetime.now(timezone.utc).isoformat(),
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
    for version in (1, 2, 4):
        contract = score.SCORED_LABEL_CONTRACT_V1 if version == 1 else score.load_scored_label_contract(ROOT/f"docs/reference/benchmarks/scored-labels-v{version}.json")
        replayed = records.rescore(record_path, contract, max_workers=1)
        (output/f"scorecard-v{version}.json").write_text(json.dumps(replayed, indent=2) + "\n")
    print(json.dumps({"profile": profile, "observations": len(writer.rows), "pairs": len(pairs),
                      "record_sha256": card["observation_record"]["sha256"]}))


if __name__ == "__main__":
    main()
