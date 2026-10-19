"""Compare external aggregate measurements without publishing native reports.

External scorers count all gold. The sealed OpenPII language subset is checked
for regressions but excluded from net-byte totals because it overlaps all.
The quality wrapper retains its narrower counter-only read contract.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from external_quality_receipt import COUNTERS, SETS, count, validate_counts, validate_receipt

SUMMED_SETS = {
    "sealed": ("openpii/all", "gretel/en", "gretel/de", "gretel/fr"),
    "piibench-commercial": ("test",),
    "pii-trace": ("test",),
}
BYTE_FIELDS = frozenset({"gold_bytes", "leaked_bytes", "false_positive_bytes"})
ROW_FIELDS = COUNTERS | BYTE_FIELDS


def aggregate_rows(scores, quality):
    """Read only fixed aggregate keys; never traverse native report extras."""
    validate_receipt(quality)
    kind = quality["benchmark"]
    result = {}
    for key in SETS[kind]:
        if kind == "sealed":
            native = scores["results"][key]
            byte_counts = {name: count(native[name]) for name in BYTE_FIELDS}
            documents = count(native["documents"])
            if count(native["failed_closed"]) != quality["quality"][key]["failed_closed_documents"]:
                raise ValueError("aggregate refusal mismatch")
        else:
            native = scores["rows"]["gaze-full"][key]["product_coverage"]
            leaked = count(native["leaked_bytes"])
            byte_counts = {
                "gold_bytes": count(native["true_positive_bytes"]) + leaked,
                "leaked_bytes": leaked,
                "false_positive_bytes": count(native["false_positive_bytes"]),
            }
            documents = count(native["documents"])
        counters = quality["quality"][key]
        if documents != counters["completed_documents"] or byte_counts["leaked_bytes"] > byte_counts["gold_bytes"]:
            raise ValueError("aggregate population mismatch")
        result[key] = {**counters, **byte_counts}
    return result


def decide(kind, base, candidate):
    """Apply per-set reliability guards, then the net-byte improvement rule."""
    if kind not in SETS or set(base) != set(SETS[kind]) or set(candidate) != set(SETS[kind]):
        raise ValueError("aggregate set mismatch")
    regressions = set()
    for key in SETS[kind]:
        before, after = base[key], candidate[key]
        for row in (before, after):
            if set(row) != ROW_FIELDS:
                raise ValueError("aggregate row schema mismatch")
            for value in row.values():
                count(value)
            validate_counts({name: row[name] for name in COUNTERS})
            if row["leaked_bytes"] > row["gold_bytes"]:
                raise ValueError("aggregate byte mismatch")
        for field in ("attempted_documents", "gold_bytes"):
            if before[field] != after[field]:
                regressions.add("population_changed")
        for field in ("leaked_bytes", "failed_closed_documents", "restore_failures",
                      "restore_decision_failures", "manifest_invalid_documents"):
            if after[field] > before[field]:
                regressions.add(field + "_rose")
        for field in ("restore_exact_documents", "restore_success_decisions", "manifest_valid_documents"):
            if after[field] < before[field]:
                regressions.add(field + "_fell")
    summed = SUMMED_SETS[kind]
    leaked_drop = sum(base[key]["leaked_bytes"] - candidate[key]["leaked_bytes"] for key in summed)
    fp_rise = sum(candidate[key]["false_positive_bytes"] - base[key]["false_positive_bytes"] for key in summed)
    quality_gain = any(candidate[key][field] > base[key][field]
                       for key in summed for field in ("restore_exact_documents", "manifest_valid_documents"))
    if regressions:
        verdict, reason = "fail", "per_set_regression"
    elif leaked_drop > 0 and fp_rise < leaked_drop:
        verdict, reason = "pass", "net_bytes_improved"
    elif leaked_drop == 0 and fp_rise < 0:
        verdict, reason = "pass", "false_positives_improved"
    elif leaked_drop == 0 and fp_rise == 0 and quality_gain:
        verdict, reason = "pass", "reversibility_improved"
    else:
        verdict, reason = "fail", "no_eligible_gain"
    return {
        "verdict": verdict, "reason": reason, "regressions": sorted(regressions),
        "summed_sets": list(summed),
        "leaked_bytes_decrease": leaked_drop, "false_positive_bytes_increase": fp_rise,
    }


def build_report(base_scores, base_quality, candidate_scores, candidate_quality):
    for quality in (base_quality, candidate_quality):
        validate_receipt(quality)
    kind = base_quality["benchmark"]
    if candidate_quality["benchmark"] != kind:
        raise ValueError("benchmark mismatch")
    for field in ("driver_sha256", "wrapper_sha256", "harness_revision", "ner_manifest_sha256",
                  "nym_manifest_sha256", "inputs_sha256"):
        if base_quality["receipts"][field] != candidate_quality["receipts"][field]:
            raise ValueError("measurement receipt mismatch")
    base = aggregate_rows(base_scores, base_quality)
    candidate = aggregate_rows(candidate_scores, candidate_quality)
    return {
        "schema_version": 1, "benchmark": kind, "contract": "all-gold",
        "gate_script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "base": {"receipts": base_quality["receipts"], "sets": base},
        "candidate": {"receipts": candidate_quality["receipts"], "sets": candidate},
        "gate": decide(kind, base, candidate),
    }


def load_pair(scores_path, quality_path):
    payload = scores_path.read_bytes()
    quality = json.loads(quality_path.read_text())
    validate_receipt(quality)
    if hashlib.sha256(payload).hexdigest() != quality["receipts"]["driver_output_sha256"]:
        raise ValueError("driver output receipt mismatch")
    return json.loads(payload), quality


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    for name in ("base-scores", "base-quality", "candidate-scores", "candidate-quality", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.output.exists():
            raise ValueError("fresh output required")
        result = build_report(*load_pair(args.base_scores, args.base_quality),
                              *load_pair(args.candidate_scores, args.candidate_quality))
        args.output.write_text(json.dumps(result, indent=2) + "\n")
    except Exception:
        print("EXTERNAL_AGGREGATE_GATE_FAILED")
        return 1
    print("EXTERNAL_AGGREGATE_GATE_" + result["gate"]["verdict"].upper())
    return 0 if result["gate"]["verdict"] == "pass" else 2


if __name__ == "__main__":
    raise SystemExit(main())
