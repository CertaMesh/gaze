#!/usr/bin/env python3
"""Check that fresh CLI sessions protect the same raw values on a seeded corpus."""

from __future__ import annotations

import argparse
import json
import subprocess
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

import dataiku_en_de_gaze_bench as dataiku
import gaze_bench_score as score
import run_no_opf_benchmark as runner


ROOT = Path(__file__).resolve().parents[2]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--dataset",
        type=Path,
        default=ROOT / "target/bench-data/dataiku-en-de/test.parquet",
    )
    parser.add_argument(
        "--negative-corpus",
        type=Path,
        default=ROOT / "crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl",
    )
    parser.add_argument("--seed", type=int, default=20260710)
    parser.add_argument("--sample-size", type=int, default=50)
    parser.add_argument("--heavy-count", type=int, default=10)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--expect-stable", action="store_true")
    return parser.parse_args()


def selected_documents(args: argparse.Namespace) -> list[score.Document]:
    positives, _ = dataiku.load_documents(args.dataset)
    negatives, _ = runner.load_negative_documents(args.negative_corpus)
    population = positives + negatives
    sample, _ = score.stratified_sample(population, args.sample_size, seed=args.seed)
    chosen = {document.uid: document for document in sample}
    for document in sorted(population, key=lambda item: (-len(item.spans), item.uid)):
        if len(chosen) >= args.sample_size + args.heavy_count:
            break
        chosen.setdefault(document.uid, document)
    return list(chosen.values())


def check_document(
    document: score.Document, args: argparse.Namespace
) -> dict[str, object]:
    manifests = []
    tokens = []
    for _ in range(args.repetitions):
        result = subprocess.run(
            [str(args.binary), "clean", "--policy", str(args.policy)],
            input=document.text.encode(),
            capture_output=True,
            timeout=180,
            check=False,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"CLI failed for document {document.uid}: exit {result.returncode}"
            )
        output = json.loads(result.stdout)
        entries = output["entries"]
        manifests.append(
            tuple(sorted((entry["class"], entry["raw"]) for entry in entries))
        )
        if entries:
            tokens.append(entries[0]["token"])
    return {
        "uid": document.uid,
        "language": document.language,
        "unstable": len(set(manifests)) > 1,
        "manifest_sizes": [len(manifest) for manifest in manifests],
        "fresh_token_prefixes": (
            len(set(tokens)) == args.repetitions if tokens else None
        ),
        "gold_spans": len(document.spans),
    }


def main() -> int:
    args = parse_args()
    if min(
        args.sample_size, args.repetitions, args.workers
    ) <= 0 or args.heavy_count < 0:
        raise ValueError("sample size, repetitions, and workers must be positive")
    documents = selected_documents(args)
    rows = []
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = [pool.submit(check_document, document, args) for document in documents]
        for future in as_completed(futures):
            rows.append(future.result())
            print(f"{len(rows)}/{len(documents)}", flush=True)
    rows.sort(key=lambda row: row["uid"])
    summary = {
        "binary": str(args.binary),
        "seed": args.seed,
        "repetitions": args.repetitions,
        "documents": len(rows),
        "languages": dict(Counter(row["language"] for row in rows)),
        "unstable_documents": sum(row["unstable"] for row in rows),
        "fresh_token_prefix_docs": sum(
            row["fresh_token_prefixes"] is True for row in rows
        ),
        "max_manifest_entries": max(max(row["manifest_sizes"]) for row in rows),
        "rows": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in summary.items() if key != "rows"}))
    return int(args.expect_stable and summary["unstable_documents"] > 0)


if __name__ == "__main__":
    raise SystemExit(main())
