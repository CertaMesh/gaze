#!/usr/bin/env python3
"""Measure what one mechanism costs in time: base policy versus base plus its delta.

Same `clean_for_bench` binary, same stratified documents (seed 20260710), arms
alternated for `--rounds` rounds so drift hits both. Reports warm per-document
`clean_ms`, the cold first document and peak RSS per arm, using the timer and
quiet-host gate of `cli-latency.py`: a run that starts on a busy host measures
nothing, and one that sees a foreign process appear says `timing invalid`.

Usage (repository root; release `clean_for_bench` already built, pinned
dataset under target/bench-data/dataiku-en-de/):

    uv run --project scripts/bench python scripts/bench/mechanism_latency.py \\
      --base-policy gaze.toml --policy-delta scripts/bench/mechanisms/gliner-dob-judge.toml \\
      --out target/bench-data/mechanism-latency.json
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

import dataiku_en_de_gaze_bench as dataiku
import gaze_bench_score as score

HERE = Path(__file__).resolve().parent
_SPEC = importlib.util.spec_from_file_location("cli_latency", HERE / "cli-latency.py")
assert _SPEC and _SPEC.loader
cli_latency = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(cli_latency)


def candidate_policy(base: Path, delta: Path, out: Path) -> Path:
    """Base plus delta, refused if the delta touches a section the base has."""
    base_data = tomllib.loads(base.read_text(encoding="utf-8"))
    delta_data = tomllib.loads(delta.read_text(encoding="utf-8"))
    overlap = sorted(base_data.keys() & delta_data.keys())
    if not delta_data or overlap:
        raise SystemExit(f"policy delta must add new sections only; overlapping: {overlap}")
    out.write_text(base.read_text(encoding="utf-8") + "\n" + delta.read_text(encoding="utf-8"),
                   encoding="utf-8")
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base-policy", type=Path, required=True)
    parser.add_argument("--policy-delta", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--documents", type=int, default=200)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--dataset", type=Path,
                        default=Path("target/bench-data/dataiku-en-de/test.parquet"))
    args = parser.parse_args(argv)
    root = Path.cwd()

    before = cli_latency.host_probe("before")
    if not before["quiet"]:
        args.out.write_text(json.dumps({"verdict": "timing invalid: host not quiet at start; "
                                        "nothing measured", "host_before": before}, indent=2) + "\n")
        print("timing invalid: host not quiet at start", file=sys.stderr)
        return 1
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", str(root / "target")))
    binary = target_dir / "release/examples/clean_for_bench"
    positive, _ = dataiku.load_documents(args.dataset)
    documents, _ = score.stratified_sample(positive, args.documents + 1, cli_latency.SEED)

    info = cli_latency.nym_latency.host_info(root)
    result: dict[str, object] = {
        "hardware": cli_latency.nym_latency.hardware_line(info),
        "git_head": subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True,
                                   text=True, check=True).stdout.strip(),
        "seed": cli_latency.SEED,
        "documents": len(documents) - 1,
        "rounds": args.rounds,
        "clean_for_bench_sha256": cli_latency.sha256_file(binary),
        "policy_delta": args.policy_delta.as_posix(),
        "host_before": before,
    }
    with tempfile.TemporaryDirectory(prefix="gaze-mechanism-latency-") as scratch:
        policies = {
            "base": args.base_policy,
            "candidate": candidate_policy(args.base_policy, args.policy_delta,
                                          Path(scratch) / "candidate.toml"),
        }
        result["policy_sha256"] = {arm: cli_latency.sha256_file(path) for arm, path in policies.items()}
        rounds: dict[str, list[dict[str, object]]] = {arm: [] for arm in policies}
        for _ in range(args.rounds):
            for arm, path in policies.items():
                rounds[arm].append(cli_latency.pipeline_arm(
                    binary, "policy-file", documents, {**os.environ, "GAZE_BENCH_POLICY": str(path)}
                ))
        result["rounds_by_arm"] = rounds
        result["median_of_rounds"] = {
            arm: {
                key: sorted(run["warm_clean"][key] for run in runs)[len(runs) // 2]
                for key in ("p50_ms", "p95_ms", "mean_ms")
            } | {
                "cold_first_document_ms": sorted(run["cold_first_document_ms"] for run in runs)[len(runs) // 2],
                "peak_rss_mib": sorted(run["peak_rss_mib"] for run in runs)[len(runs) // 2],
            }
            for arm, runs in rounds.items()
        }
    after = cli_latency.host_probe("after")
    result["host_after"] = after
    result["verdict"] = cli_latency.verdict(before, after, False)
    args.out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"verdict": result["verdict"], "median_of_rounds": result["median_of_rounds"]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
