#!/usr/bin/env python3
"""Attach chart selection and conservative timing validity to a complete run."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from compare import TOOLS
import row_sources


CHART_CONFIGS = Path(__file__).with_name("chart-configs.json")


def finalize(report: dict) -> dict:
    row_sources.complete(report)
    declared = json.loads(CHART_CONFIGS.read_text(encoding="utf-8"))
    if len(set(declared.values())) != len(declared) or not set(declared.values()) <= set(TOOLS):
        raise ValueError("README chart configurations must be distinct measured tools")
    samples = report["contention_samples"]
    report["chart_config_sha256"] = hashlib.sha256(CHART_CONFIGS.read_bytes()).hexdigest()
    report["finalizer_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    report["latency_validity"] = {
        "publishable": False,
        "reason": "Comparison timing is withheld pending a dedicated quiet-machine measurement.",
        "required_rule": "Mark contended if sampled CPU outside the benchmark process tree exceeds 100%.",
        "contended_configurations": sorted(name for name, data in samples.items() if data["contended"]),
    }
    for name, tool in report["tools"].items():
        for contract in tool["contracts"].values():
            for row in contract.values():
                row["latency"]["contended"] = samples[name]["contended"]
                row["latency"]["status"] = "withheld"
    for contract in report["gaze"].values():
        for row in contract["layers"].values():
            row["latency"]["contended"] = samples["gaze-full"]["contended"]
            row["latency"]["status"] = "withheld"
    for name, ablation in report["gaze_ablations"].items():
        for contract in ablation.values():
            for row in contract.values():
                row["latency"]["contended"] = samples[f"gaze-{name}"]["contended"]
                row["latency"]["status"] = "withheld"
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--opf-unavailable-receipt", type=Path)
    parser.add_argument("--compose-base", type=Path)
    parser.add_argument("--base-sha256")
    parser.add_argument("--supplement", type=Path)
    parser.add_argument("--supplement-sha256")
    parser.add_argument("--base-predictions-dir", type=Path)
    parser.add_argument("--supplement-predictions-dir", type=Path)
    args = parser.parse_args()
    composition = (args.compose_base, args.base_sha256, args.supplement,
                   args.supplement_sha256, args.base_predictions_dir, args.supplement_predictions_dir)
    if any(value is not None for value in composition):
        if not all(value is not None for value in composition) or args.opf_unavailable_receipt:
            parser.error("composition requires both pinned snapshots and private prediction directories; OPF stays inherited")
        report = row_sources.compose(args.compose_base, args.base_sha256, args.supplement,
                                     args.supplement_sha256, args.report,
                                     args.base_predictions_dir, args.supplement_predictions_dir)
    else:
        report = json.loads(args.report.read_text(encoding="utf-8"))
        if "row_sources" in report:
            if args.opf_unavailable_receipt:
                parser.error("composed OPF receipt must remain inherited")
            row_sources.validate(report, args.report)
        else:
            if args.opf_unavailable_receipt is not None:
                report["unavailable"] = {"opf": json.loads(args.opf_unavailable_receipt.read_text(encoding="utf-8"))}
            report = finalize(report)
    if "row_sources" in report:
        from render import validate_current
        validate_current(report, args.report)
    temporary = args.report.with_suffix(args.report.suffix + ".tmp")
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(args.report)


if __name__ == "__main__":
    main()
