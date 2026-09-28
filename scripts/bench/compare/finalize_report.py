#!/usr/bin/env python3
"""Attach chart selection and conservative timing validity to a complete run."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from compare import TOOLS


CHART_CONFIGS = Path(__file__).with_name("chart-configs.json")


def finalize(report: dict) -> dict:
    if report["schema_version"] != 2 or set(report["tools"]) != set(TOOLS) or report["skipped"]:
        raise ValueError("only a complete, unskipped schema-v2 comparison can be finalized")
    declared = json.loads(CHART_CONFIGS.read_text(encoding="utf-8"))
    if len(set(declared.values())) != len(declared) or not set(declared.values()) <= set(TOOLS):
        raise ValueError("README chart configurations must be distinct measured tools")
    samples = report.get("contention_samples", {})
    if set(samples) != set(TOOLS) | {"gaze-full", "gaze-rules-only", "gaze-rules-ner"}:
        raise ValueError("foreign CPU samples are missing for a measured configuration")
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
    args = parser.parse_args()
    report = finalize(json.loads(args.report.read_text(encoding="utf-8")))
    temporary = args.report.with_suffix(args.report.suffix + ".tmp")
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(args.report)


if __name__ == "__main__":
    main()
