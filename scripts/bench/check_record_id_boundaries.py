#!/usr/bin/env python3
"""Reproduce boundary-cell sensitivity with a pinned pre-repair binary.

This model-free probe is not benchmark gain evidence. The canonical byte
scorer and response validator determine all reported metrics.
"""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

import agentic_layers as api
import gaze_bench_score as score
import record_id_boundary_cells as cells


SHAPES = ("punctuation_period", "punctuation_colon", "punctuation_dotted",
          "punctuation_short_dotted", "connectors_is_same", "connectors_ist_same",
          "connectors_lautet_same", "connectors_space_same", "serialized_depth2_unicode",
          "serialized_depth2_unicode_escape", "serialized_depth3_quote",
          "serialized_depth8_tab", "serialized_depth9_unicode",
          "opening_lf_lf_malformed", "opening_lf_crlf_malformed",
          "tab_group_unicode", "tab_group_combining", "tab_group_ambiguous_column")


def selected_rows(partition):
    rows = cells.records(api, partition)
    selected = []
    for family in cells.LABELS:
        for shape in SHAPES:
            choices = [r for r in rows if r.family == family and r.layer == "R"
                       and r.surface == "recordids_boundary_" + shape]
            selected.append(choices[1] if len(choices) > 1 else choices[0])
    return selected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--detector-root", required=True, type=Path)
    parser.add_argument("--policy", required=True, type=Path)
    parser.add_argument("--binary-receipt", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    binding = json.loads(args.binary_receipt.read_text())
    if (binding["binary_sha256"] != score.sha256_file(args.binary)
            or binding["policy_sha256"] != score.sha256_file(args.policy)):
        raise ValueError("binary/policy receipt does not match actual probe inputs")
    revision = binding["revision"]
    if subprocess.check_output(["git", "-C", str(args.detector_root), "diff", revision, "HEAD", "--",
                                "crates", "Cargo.toml", "Cargo.lock"]):
        raise ValueError("vocabulary checkout runtime differs from the receipt-bound binary source")
    rows = selected_rows("test")
    documents = [r.to_document() for r in rows]
    score.COMMITTED_SOURCE_ID_VOCABULARY = score.load_committed_source_id_vocabulary(args.detector_root)
    cases = []

    def observe(config, document, response, _):
        if "pipeline_error_code" in response:
            raise ValueError("a refused probe cannot demonstrate boundary sensitivity")
        predictions = score.final_trace_predictions(document, response)
        accumulator = score.MetricAccumulator()
        accumulator.add(document, predictions)
        cases.append({"id": document.uid, "cell": document.cell,
                      "utf8_bytes": accumulator.result()["utf8_bytes"],
                      "restore": response["restore"],
                      "final_protection_trace": response["final_protection_trace"]})

    result = score.run_config(root, args.binary.resolve(), "policy-file", documents,
                              Path("~/.local/share/gaze/models/davlan-mbert-ner-hrl").expanduser(),
                              None, None, None, 0.35, args.output.parent / "probe-logs",
                              policy_path=args.policy.resolve(), record_document=observe)
    for family in cells.LABELS:
        for shape in ("punctuation_period", "connectors_is_same", "serialized_depth2_unicode",
                      "opening_lf_lf_malformed", "tab_group_unicode"):
            row = next(r for r in rows if r.family == family and r.surface == "recordids_boundary_" + shape)
            case = next(c for c in cases if c["id"] == row.uid)
            if case["utf8_bytes"]["leaked"] == 0:
                raise ValueError(f"pre-repair failure is not visible: {family}/{shape}")
    payload = {"schema_version": 1, "purpose": "Independent model-free pre-repair sensitivity; no gain claim",
               "generator_version": api.GENERATOR_VERSION,
               "generator_revision": score.git_metadata(root),
               "source_binding": binding,
               "vocabulary_revision": score.git_metadata(args.detector_root),
               "binary_sha256": score.sha256_file(args.binary),
               "policy_sha256": score.sha256_file(args.policy),
               "selected_documents_sha256": hashlib.sha256(api.corpus_bytes(rows)).hexdigest(),
               "pipeline_availability": result["pipeline_availability"],
               "pipeline_contract": result["pipeline_contract"], "cases": cases}
    text = json.dumps(payload, indent=2) + "\n"
    if "/Users/" in text or "/tmp/" in text:
        raise ValueError("probe artifacts must be portable")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(text)
    print(f"Boundary sensitivity: {len(cases)} canonical probes, all five failures visible in all families")


if __name__ == "__main__":
    main()
