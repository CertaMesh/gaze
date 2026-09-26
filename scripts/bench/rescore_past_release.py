#!/usr/bin/env python3
"""Measure a past release's own detection code with today's harness and scorer.

A benchmark docs change compares releases, so every release it shows has to be
scored the same way. A release tagged before a scorer feature existed (v0.14.0
predates `--scored-labels`) cannot be re-run with its own harness under the new
contract. This driver keeps the release's detection code and swaps only the
measuring side:

* the benchmark binary is the release's own `clean_for_bench`, built from its
  own commit with its own feature set, and runs from its own checkout;
* the corpus, sampling, scored-label contract, response validation and scoring
  are today's (`gaze_bench_score`, `run_no_opf_benchmark` helpers), with one
  recorded exception: `--manifest-actions tokenize` checks trace/manifest
  agreement under the rule a pre-v0.15 binary was built against (redactions
  were not manifest entries yet). The leak math is unchanged by it.

The scorecard records the release checkout as `gaze` (it must be clean, so the
row stays reproducible) and the harness commit, driver and binary digest under
`runner_provenance`. Record it with
`render_benchmark_doc.py --append-contract-result`.

    uv run --project scripts/bench python scripts/bench/rescore_past_release.py \\
      --release-root <checkout of the release's measured commit> \\
      --binary <that checkout>/target/debug/examples/clean_for_bench \\
      --binary-profile debug \\
      --configs rule-floor-extended,pass2-ner,full-stack-kiji-resolve \\
      --model-env GAZE_KIJI_DISTILBERT_MODEL_DIR=<model-cache>/kiji-distilbert \\
      --model-bundle kiji-distilbert=<model-cache>/kiji-distilbert=<SHA256SUMS digest> \\
      --manifest-actions tokenize \\
      --scored-labels docs/reference/benchmarks/scored-labels-v2.json \\
      --output target/bench-data/past-release/scorecard-v4.json
"""

from __future__ import annotations

import argparse
import hashlib
import os
import platform
import subprocess
import sys
from pathlib import Path
from typing import Sequence

sys.path.insert(0, str(Path(__file__).resolve().parent))

import dataiku_en_de_gaze_bench as dataiku  # noqa: E402
import gaze_bench_score as score  # noqa: E402
import run_no_opf_benchmark as runner  # noqa: E402

HARNESS_ROOT = Path(__file__).resolve().parents[2]


class PastReleaseError(Exception):
    """Raised for an input that would make the measurement unreproducible."""


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _key_value(value: str, flag: str) -> tuple[str, str]:
    key, sep, rest = value.partition("=")
    if not sep or not key or not rest:
        raise PastReleaseError(f"{flag} expects KEY=VALUE, got {value!r}")
    return key, rest


def model_bundle(value: str) -> dict[str, object]:
    """`model_id=<dir>=<expected SHA256SUMS digest>`, verified on disk."""
    parts = value.split("=")
    if len(parts) != 3:
        raise PastReleaseError(f"--model-bundle expects ID=DIR=SHA256, got {value!r}")
    model_id, directory, expected = parts
    sums = Path(directory).expanduser() / "SHA256SUMS"
    if not sums.is_file():
        raise PastReleaseError(f"{model_id}: {sums} is missing")
    observed = _sha256(sums)
    if observed != expected:
        raise PastReleaseError(
            f"{model_id}: SHA256SUMS digest {observed} does not match the pinned {expected}"
        )
    return {
        "model_id": model_id,
        "digest_kind": "SHA256SUMS",
        "expected_sha256": expected,
        "observed_sha256": observed,
    }


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--release-root", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--binary-profile", choices=("debug", "release"), required=True)
    parser.add_argument("--configs", required=True)
    parser.add_argument("--model-env", action="append", default=[])
    parser.add_argument("--model-bundle", action="append", default=[])
    parser.add_argument(
        "--scored-labels",
        type=Path,
        help="scored-label contract file; omitted means contract v1 (every label)",
    )
    parser.add_argument("--seed", type=int, default=20260710)
    parser.add_argument("--threshold", type=float, default=0.3)
    parser.add_argument("--warmups", type=int, default=runner.DEFAULT_WARMUPS)
    parser.add_argument(
        "--dataset", type=Path, default=Path("target/bench-data/dataiku-en-de/test.parquet")
    )
    parser.add_argument("--model-dir", type=Path, default=runner.DEFAULT_DAVLAN_MODEL)
    parser.add_argument(
        "--manifest-actions",
        choices=("tokenize,redact", "tokenize"),
        default="tokenize,redact",
        help=(
            "trace actions the release recorded as manifest entries; `tokenize` "
            "for releases built before redactions joined the manifest (v0.14.0)"
        ),
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--max-documents",
        type=int,
        help="smoke check only: a sampled run is never recorded as a release row",
    )
    return parser.parse_args(argv)


def run(args: argparse.Namespace) -> Path:
    configs = tuple(c for c in args.configs.split(",") if c)
    if not configs or any("opf" in c.lower() for c in configs):
        raise PastReleaseError("configs must be non-empty and OPF-free")
    # Read before anything runs: the scorecard names the harness that scored it,
    # not whatever the checkout holds when the run ends.
    harness = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=HARNESS_ROOT, check=True, text=True,
        capture_output=True,
    ).stdout.strip()
    release_root = args.release_root.resolve()
    status = subprocess.run(
        ["git", "status", "--porcelain"], cwd=release_root, check=True, text=True,
        capture_output=True,
    ).stdout
    if status.strip():
        raise PastReleaseError(f"release checkout is dirty: {release_root}")
    # The scorecard stamps the harness commit; an uncommitted scorer edit would
    # be credited to a commit that does not contain it.
    harness_status = subprocess.run(
        ["git", "status", "--porcelain"], cwd=HARNESS_ROOT, check=True, text=True,
        capture_output=True,
    ).stdout
    if harness_status.strip():
        raise PastReleaseError(f"harness checkout is dirty: {HARNESS_ROOT}")
    binary = args.binary.resolve()
    if not binary.is_file():
        raise PastReleaseError(f"release benchmark binary is missing: {binary}")

    davlan = args.model_dir.expanduser().resolve()
    bundles = runner.validate_required_models(HARNESS_ROOT, davlan)
    bundles += [model_bundle(value) for value in args.model_bundle]

    contract = runner.load_scored_label_contract(HARNESS_ROOT, args.scored_labels)
    dataset_path = runner.repo_path(HARNESS_ROOT, args.dataset)
    dataiku.verify_dataset(dataset_path)
    positives, dataiku_report = dataiku.load_documents(dataset_path)
    negatives, negative_report = runner.load_negative_documents(
        HARNESS_ROOT / runner.NEGATIVE_CORPUS
    )
    available = positives + negatives
    documents, sampling_report = score.stratified_sample(
        available, args.max_documents, seed=args.seed
    )
    available = score.apply_scored_label_contract(available, contract)
    documents = score.apply_scored_label_contract(documents, contract)

    probe = score.build_validator_probe(HARNESS_ROOT)
    measurements = score.collect_validator_measurements(
        probe, available, (document.uid for document in documents)
    )
    environment = runner.build_no_opf_environment(os.environ)
    for value in args.model_env:
        key, path = _key_value(value, "--model-env")
        environment[key] = str(Path(path).expanduser())
    output = args.output.resolve()
    runs = [
        score.run_config(
            release_root,
            binary,
            config,
            documents,
            davlan,
            None,
            None,
            None,
            args.threshold,
            output.parent / "logs",
            base_environment=environment,
            warmup_count=args.warmups,
            validator_measurements=measurements,
            replacing_actions=frozenset(args.manifest_actions.split(",")),
        )
        for config in configs
    ]
    metadata, dataset_report = runner.composite_dataset_report(dataiku_report, negative_report)
    dataset_report["validator_gold_census"] = score.validator_gold_census(
        available, measurements
    )
    card = score.assemble_scorecard(
        repo_root=release_root,
        dataset_metadata=metadata,
        dataset_report=dataset_report,
        sampling_report=sampling_report,
        parameters={
            "profile": "full" if args.max_documents is None else "sampled",
            "configs": list(configs),
            "binary_profile": args.binary_profile,
            "max_documents": args.max_documents,
            "sampling_seed": args.seed,
            "ner_threshold": args.threshold,
            "warmup_count": args.warmups,
            "measured_repetitions": 1,
            "opf": False,
        },
        runs=runs,
        scored_label_contract=score.scored_label_contract_report(contract, documents),
    )
    card["runner_provenance"] = {
        "entry_point": "scripts/bench/rescore_past_release.py",
        "method": (
            "release detection code, today's harness: the release's own "
            "clean_for_bench (built from the measured commit) scored by the "
            "harness commit below"
        ),
        "harness_revision": harness,
        "manifest_replacing_actions": sorted(args.manifest_actions.split(",")),
        "binary_sha256": _sha256(binary),
        "profile": card["parameters"]["profile"],
        "model_bundles": bundles,
        "hardware": platform.platform() + "; " + platform.processor(),
        "warmup_count": args.warmups,
        "measured_repetitions": 1,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    runner.write_json(output, card)
    return output


def main(argv: Sequence[str] | None = None) -> int:
    try:
        print(f"rescore_past_release: wrote {run(parse_args(argv))}")
        return 0
    except (PastReleaseError, runner.CandidateError, score.ScoredLabelContractError) as error:
        sys.stderr.write(f"rescore_past_release: {error}\n")
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
