#!/usr/bin/env python3
"""Paired, all-request comparison of the single-pass Nym arms (solo todo 3738, Stage A).

Runs every arm over the same population the release harness scores (the EN/DE Dataiku
holdout plus the A4 negatives, 2,910 documents) on one binary, keeps every validated
response per document, and scores them with the release scorer's own accumulators, so a
row here equals the scorecard row for that arm.

    uv run --project scripts/bench python scripts/bench/single_pass_nym_paired.py run \
        --binary target/release/examples/clean_for_bench --out target/bench-data/single-pass
    uv run --project scripts/bench python scripts/bench/single_pass_nym_paired.py score \
        --out target/bench-data/single-pass

`run` needs GAZE_NYM_MODEL_DIR (and the Davlan bundle); OPF variables are scrubbed. `score`
needs only the captured responses. What `score` reports, English, German and negatives
separately and together:

* completion and refusal of every requested document per arm (all-request accounting), and
  the paired population every arm completed;
* leaked, true-positive and false-positive bytes under contract v2 (the headline), the v3
  gold-gap diagnostic beside it, exact restores;
* bytes each arm buys over `pass2-ner` and the false-positive bytes it adds, on the paired
  population;
* per label, the residual gold bytes `pass2-ner` leaks split into single-pass only,
  resolve only, both and neither;
* Nym flags on the negatives (trace items sourced by a Nym adapter or a safety-net action);
* the Stage A acceptance checks.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import subprocess
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path
from typing import Iterable, Mapping, Sequence

import dataiku_en_de_gaze_bench as dataiku
import gaze_bench_score as score
import run_no_opf_benchmark as release
from bench_subprocess import BenchSubprocess

ARMS = (
    "rule-floor-extended",
    "pass2-ner",
    "full-stack-nym-resolve",
    "single-pass-nym",
    "single-pass-nym-observed",
)
BASELINE = "pass2-ner"
RESOLVE = "full-stack-nym-resolve"
SINGLE_PASS = "single-pass-nym"
OBSERVED = "single-pass-nym-observed"
CONTRACT = Path("docs/reference/benchmarks/scored-labels-v3.json")
POPULATIONS = ("en", "de", "negative")
# Stage A acceptance (brief 10098, todo 3738).
REFERENCE_RESOLVE_BUYS = 6_154
MAX_ADDED_FALSE_POSITIVE_BYTES = 526
MAX_NEGATIVE_FALSE_FLAGS = 1


def population(document: score.Document) -> str:
    return "negative" if document.negative_category is not None else document.language


def load_population(repo_root: Path) -> list[score.Document]:
    positives, _ = dataiku.load_documents(repo_root / "target/bench-data/dataiku-en-de/test.parquet")
    negatives, _ = release.load_negative_documents(repo_root / release.NEGATIVE_CORPUS)
    documents, _ = score.stratified_sample([*positives, *negatives], None, score.DEFAULT_SAMPLE_SEED)
    contract = score.load_scored_label_contract(
        repo_root / CONTRACT, display_path=CONTRACT.as_posix()
    )
    return score.apply_scored_label_contract(documents, contract)


def run_arm(
    repo_root: Path,
    binary: Path,
    arm: str,
    documents: Sequence[score.Document],
    environment: Mapping[str, str],
    output: Path,
) -> dict[str, object]:
    started = time.perf_counter()
    load_before = os.getloadavg()
    with output.open("w", encoding="utf-8") as sink:
        command = [str(binary), "--config", arm]
        if arm in (SINGLE_PASS, OBSERVED):
            command += ["--exclusion-trace", str(output.with_suffix(".exclusion.jsonl"))]
        with BenchSubprocess(command, cwd=repo_root, env=dict(environment)) as process:
            for index, document in enumerate(documents):
                request = {
                    "fixture_id": document.uid,
                    "locale_chain": document.locale_chain,
                    "text": document.text,
                }
                response = score.validate_response(document, process.exchange(request))
                sink.write(json.dumps({"id": document.uid, "response": response}) + "\n")
                if (index + 1) % 500 == 0:
                    print(f"{arm}: {index + 1}/{len(documents)}", file=sys.stderr)
            process.check_deadline()
    return {
        "arm": arm,
        "documents": len(documents),
        "wall_seconds": round(time.perf_counter() - started, 1),
        "load_average_before": load_before,
        "load_average_after": os.getloadavg(),
        "responses": output.name,
    }


def cmd_run(args: argparse.Namespace) -> int:
    repo_root = Path(__file__).resolve().parents[2]
    if not os.environ.get("GAZE_NYM_MODEL_DIR"):
        raise RuntimeError("GAZE_NYM_MODEL_DIR must point at the pinned Nym bundle")
    binary = (args.binary if args.binary.is_absolute() else repo_root / args.binary).resolve()
    out = args.out if args.out.is_absolute() else repo_root / args.out
    out.mkdir(parents=True, exist_ok=True)
    documents = load_population(repo_root)
    environment = release.build_no_opf_environment(os.environ)
    environment["GAZE_NER_MODEL_DIR"] = str(args.model_dir.expanduser().resolve())
    environment["GAZE_NER_THRESHOLD"] = str(args.threshold)
    runs = []
    for arm in args.arm or ARMS:
        print(f"running {arm} on {len(documents)} documents", file=sys.stderr)
        runs.append(run_arm(repo_root, binary, arm, documents, environment, out / f"{arm}.jsonl"))
    git = score.git_metadata(repo_root)
    manifest = {
        "binary": str(binary.relative_to(repo_root)) if binary.is_relative_to(repo_root) else str(binary),
        "binary_sha256": score.sha256_file(binary),
        "git": git,
        "host": {
            "machine": platform.machine(),
            "platform": platform.platform(),
            "processor": subprocess.run(
                ["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True
            ).stdout.strip()
            or platform.processor(),
        },
        "population": {
            "documents": len(documents),
            "ids": score.document_ids_digest([d.uid for d in documents]),
        },
        "ner_threshold": args.threshold,
        "runs": runs,
        "timing_note": "correctness only; no latency claim is made from these runs",
    }
    existing = out / "manifest.json"
    if existing.exists() and args.arm:
        previous = json.loads(existing.read_text(encoding="utf-8"))
        if previous["binary_sha256"] != manifest["binary_sha256"]:
            raise RuntimeError("appending arms from a different binary to one comparison")
        kept = [run for run in previous["runs"] if run["arm"] not in set(args.arm)]
        manifest["runs"] = kept + runs
    existing.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return 0


def load_responses(path: Path) -> dict[str, dict[str, object]]:
    responses = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        row = json.loads(line)
        responses[row["id"]] = row["response"]
    return responses


def is_refusal(response: Mapping[str, object]) -> bool:
    return "pipeline_error_code" in response


def nym_flag(item: Mapping[str, object]) -> bool:
    provenance = item["provenance"]
    return provenance["stage"] == "safety_net" or any(
        source.startswith("nym/") for source in provenance["source_ids"]
    )


def intervals(spans: Iterable[score.Span]) -> list[tuple[int, int]]:
    return score.merge_intervals((span.start, span.end) for span in spans)


def intersect(
    left: Sequence[tuple[int, int]], right: Sequence[tuple[int, int]]
) -> list[tuple[int, int]]:
    return score.merge_intervals(
        (max(a, c), min(b, d))
        for a, b in left
        for c, d in right
        if max(a, c) < min(b, d)
    )


def arm_summary(
    documents: Sequence[score.Document],
    responses: Mapping[str, Mapping[str, object]],
    paired: frozenset[str],
) -> dict[str, object]:
    metrics: dict[str, score.MetricAccumulator] = defaultdict(score.MetricAccumulator)
    paired_metrics: dict[str, score.MetricAccumulator] = defaultdict(score.MetricAccumulator)
    contract: dict[str, score.ContractAccumulator] = defaultdict(score.ContractAccumulator)
    attempted: Counter[str] = Counter()
    refused: dict[str, Counter[str]] = defaultdict(Counter)
    negative_flags = 0
    negative_flag_documents = 0
    for document in documents:
        group = population(document)
        attempted[group] += 1
        response = responses[document.uid]
        if is_refusal(response):
            refused[group][str(response["pipeline_error_code"])] += 1
            continue
        predictions = score.final_trace_predictions(document, response)
        for key in (group, "all"):
            metrics[key].add(document, predictions)
            contract[key].add(response)
            if document.uid in paired:
                paired_metrics[key].add(document, predictions)
        if group == "negative":
            flags = sum(nym_flag(item) for item in response["final_protection_trace"])
            negative_flags += flags
            negative_flag_documents += flags > 0
    attempted["all"] = sum(attempted[group] for group in POPULATIONS)
    summary = {}
    for key in (*POPULATIONS, "all"):
        refusals = (
            sum((refused[group] for group in POPULATIONS), Counter())
            if key == "all"
            else refused[key]
        )
        result = metrics[key].result() if key in metrics else None
        paired_result = paired_metrics[key].result() if key in paired_metrics else None
        contract_result = contract[key].result() if key in contract else None
        summary[key] = {
            "attempted": attempted[key],
            "completed": attempted[key] - sum(refusals.values()),
            "refused": dict(sorted(refusals.items())),
            "exact_restores": contract_result["restore_exact_documents"] if contract_result else 0,
            "all_request": _bytes(result),
            "paired": _bytes(paired_result),
        }
    summary["negative_nym_flags"] = {
        "trace_items": negative_flags,
        "documents": negative_flag_documents,
    }
    return summary


def _bytes(result: Mapping[str, object] | None) -> dict[str, object] | None:
    if result is None:
        return None
    utf8 = result["utf8_bytes"]
    out = {
        "documents": result["documents"],
        "gold": utf8["pii"],
        "leaked": utf8["leaked"],
        "true_positive": utf8["true_positive"],
        "false_positive": utf8["false_positive"],
        "precision": round(utf8["precision"], 4),
    }
    if "gold_gap" in result:
        gap = result["gold_gap"]
        out["v3_gold_gap_protected"] = gap["gold_gap_protected_bytes"]
        out["v3_false_positive_after_gold_gap"] = gap["false_positive_bytes_after_gold_gap"]
    return out


def per_label(
    documents: Sequence[score.Document],
    arms: Mapping[str, Mapping[str, Mapping[str, object]]],
    paired: frozenset[str],
) -> dict[str, dict[str, object]]:
    """Leaked bytes per label and arm, and the residual split between two arms."""
    leaked: dict[str, Counter[str]] = defaultdict(Counter)
    split: dict[str, dict[str, Counter[str]]] = {
        pair: defaultdict(Counter) for pair in ((SINGLE_PASS, RESOLVE), (OBSERVED, RESOLVE))
    }
    for document in documents:
        if document.uid not in paired or document.negative_category is not None:
            continue
        predicted = {
            arm: intervals(score.final_trace_predictions(document, responses[document.uid]))
            for arm, responses in arms.items()
        }
        for label in sorted({span.label for span in document.spans}):
            gold = score.merge_intervals(
                (span.start, span.end) for span in document.spans if span.label == label
            )
            for arm, covered in predicted.items():
                leaked[label][arm] += score.interval_length(gold) - score.intersection_length(gold, covered)
            residual = score.subtract_intervals(gold, predicted[BASELINE])
            for (left, right), counter in split.items():
                if left not in predicted or right not in predicted:
                    continue
                left_cov = intersect(residual, predicted[left])
                both = score.intersection_length(left_cov, predicted[right])
                left_bytes = score.interval_length(left_cov)
                right_bytes = score.intersection_length(residual, predicted[right])
                total = score.interval_length(residual)
                counter[label]["residual"] += total
                counter[label]["both"] += both
                counter[label][f"{left}_only"] += left_bytes - both
                counter[label][f"{right}_only"] += right_bytes - both
                counter[label]["neither"] += total - left_bytes - right_bytes + both
    return {
        "leaked_by_arm": {label: dict(values) for label, values in sorted(leaked.items())},
        "residual_split": {
            f"{left}_vs_{right}": {label: dict(values) for label, values in sorted(counter.items())}
            for (left, right), counter in split.items()
        },
    }


def acceptance(summaries: Mapping[str, Mapping[str, object]]) -> dict[str, object]:
    def paired_all(arm: str, field: str) -> int:
        return int(summaries[arm]["all"]["paired"][field])

    def buys(arm: str) -> int:
        return paired_all(BASELINE, "leaked") - paired_all(arm, "leaked")

    def added_fp(arm: str) -> int:
        return paired_all(arm, "false_positive") - paired_all(BASELINE, "false_positive")

    checks = {}
    for arm in (SINGLE_PASS, OBSERVED):
        if arm not in summaries or RESOLVE not in summaries:
            continue
        everything = summaries[arm]["all"]
        checks[arm] = {
            "bought_bytes": buys(arm),
            "resolve_bought_bytes": buys(RESOLVE),
            "matches_or_beats_resolve": buys(arm) >= buys(RESOLVE),
            "at_least_reference": buys(arm) >= REFERENCE_RESOLVE_BUYS,
            "added_false_positive_bytes": added_fp(arm),
            "resolve_added_false_positive_bytes": added_fp(RESOLVE),
            "added_false_positive_within_budget": added_fp(arm) <= MAX_ADDED_FALSE_POSITIVE_BYTES,
            "completed_every_request": everything["completed"] == everything["attempted"],
            "exact_restore_every_request": everything["exact_restores"] == everything["attempted"],
            "negative_false_flags": summaries[arm]["negative_nym_flags"]["trace_items"],
            "negative_flags_within_budget": summaries[arm]["negative_nym_flags"]["trace_items"]
            <= MAX_NEGATIVE_FALSE_FLAGS,
        }
    return checks


def cmd_score(args: argparse.Namespace) -> int:
    repo_root = Path(__file__).resolve().parents[2]
    out = args.out if args.out.is_absolute() else repo_root / args.out
    manifest = json.loads((out / "manifest.json").read_text(encoding="utf-8"))
    documents = load_population(repo_root)
    arms = {
        run["arm"]: load_responses(out / run["responses"])
        for run in manifest["runs"]
    }
    for arm, responses in arms.items():
        if set(responses) != {d.uid for d in documents}:
            raise RuntimeError(f"{arm}: responses do not cover the requested population")
    paired = frozenset(
        document.uid
        for document in documents
        if all(not is_refusal(responses[document.uid]) for responses in arms.values())
    )
    summaries = {arm: arm_summary(documents, responses, paired) for arm, responses in arms.items()}
    report = {
        "contract": "scored-labels-v3 (v2 headline bytes + gold-gap diagnostic)",
        "binary_sha256": manifest["binary_sha256"],
        "git": manifest["git"],
        "host": manifest["host"],
        "population": manifest["population"],
        "paired_documents": len(paired),
        "arms": summaries,
        "per_label": per_label(documents, arms, paired),
        "acceptance": acceptance(summaries),
        "refusals_by_document": {
            arm: sorted(
                (uid, str(responses[uid]["pipeline_error_code"]))
                for uid in responses
                if is_refusal(responses[uid])
            )
            for arm, responses in arms.items()
        },
    }
    (out / "paired-report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"paired_documents": len(paired), "acceptance": report["acceptance"]}, indent=2))
    return 0


def parse_args(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run")
    run.add_argument("--binary", type=Path, required=True)
    run.add_argument("--out", type=Path, required=True)
    run.add_argument("--model-dir", type=Path, default=release.DEFAULT_DAVLAN_MODEL)
    run.add_argument("--threshold", type=float, default=0.3)
    run.add_argument("--arm", action="append", choices=ARMS, help="repeatable; default all")
    run.set_defaults(handler=cmd_run)
    scoring = commands.add_parser("score")
    scoring.add_argument("--out", type=Path, required=True)
    scoring.set_defaults(handler=cmd_score)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    return args.handler(args)


if __name__ == "__main__":
    raise SystemExit(main())
