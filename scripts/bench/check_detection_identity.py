#!/usr/bin/env python3
"""Prove two clean_for_bench builds produce byte-identical output on the full corpus.

Both binaries run the same config over every positive and negative document with one fixed
session hex and GAZE_BENCH_AUDIT_ROWS set. A document matches when clean text, manifest spans,
final protection trace, leak suspects, restore result and audit rows are equal (timings are
ignored). The two binaries must differ, so a stale or reused build cannot pass as evidence.

Each document goes to both warm processes back to back, so `timing.clean_ms` also yields a paired
latency ratio. On a loaded host only the ratio means anything; the load average is reported.

`--ignore-lineage` compares a candidate that adds typed trace lineage (record schema v2:
`provenance.settlement` and `provenance.contributions`) against a base without it. The
candidate must carry lineage on every trace item and the base on none; only those two keys
are removed before the comparison, so every other trace field still has to match.
`--agentic` adds the generated agentic-layer documents (A/D/R) to the corpus.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import statistics
import sys
from pathlib import Path

import dataiku_en_de_gaze_bench as dataiku
import run_no_opf_benchmark as runner
from bench_subprocess import BenchSubprocess

ROOT = Path(__file__).resolve().parents[2]
SESSION_HEX = "5e55104e"
COMPARED = (
    "clean_text",
    "manifest_spans",
    "final_protection_trace",
    "leak_suspects",
    "strict_would_reject",
    "restore",
    "audit_rows",
    "pipeline_error_code",
)


LINEAGE_KEYS = ("settlement", "contributions")


def strip_lineage(response: dict, *, expect: bool) -> tuple[dict, dict[str, int]]:
    """The response without typed lineage, and the role counts it carried.

    Raises when lineage is missing where expected or present where not: a comparison
    that silently tolerated either would prove nothing about the new field.
    """
    roles: dict[str, int] = {}
    trace = response.get("final_protection_trace")
    if not trace:
        return response, roles
    stripped = []
    for item in trace:
        provenance = dict(item["provenance"])
        present = [key for key in LINEAGE_KEYS if key in provenance]
        if present != (list(LINEAGE_KEYS) if expect else []):
            raise SystemExit(
                f"{response.get('fixture_id')}: trace lineage {present} where "
                f"{'all' if expect else 'none'} of {LINEAGE_KEYS} expected"
            )
        for contribution in provenance.get("contributions", ()):
            roles[contribution["role"]] = roles.get(contribution["role"], 0) + 1
        for key in LINEAGE_KEYS:
            provenance.pop(key, None)
        stripped.append({**item, "provenance": provenance})
    return {**response, "final_protection_trace": stripped}, roles


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def latency(timings: list[tuple[float, float]]) -> dict[str, object]:
    if not timings:
        return {}
    base = [pair[0] for pair in timings]
    candidate = [pair[1] for pair in timings]
    return {
        "documents": len(timings),
        "load_average": os.getloadavg(),
        "base_p50_ms": statistics.median(base),
        "candidate_p50_ms": statistics.median(candidate),
        "p50_ratio": statistics.median(candidate) / statistics.median(base),
        "paired_median_ratio": statistics.median(
            right / left for left, right in timings if left > 0
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--config", default="policy-file")
    parser.add_argument("--policy", type=Path, help="GAZE_BENCH_POLICY for policy-file")
    parser.add_argument(
        "--dataset", type=Path,
        default=ROOT / "target/bench-data/dataiku-en-de/test.parquet",
    )
    parser.add_argument("--negative-corpus", type=Path, default=ROOT / runner.NEGATIVE_CORPUS)
    parser.add_argument("--limit", type=int, help="first N documents only (smoke runs)")
    parser.add_argument("--ignore-lineage", action="store_true",
                        help="candidate adds typed trace lineage; compare everything else")
    parser.add_argument("--agentic", action="store_true",
                        help="also run the generated agentic-layer documents")
    args = parser.parse_args()

    hashes = {"base": sha256(args.base), "candidate": sha256(args.candidate)}
    if hashes["base"] == hashes["candidate"]:
        print("base and candidate binaries are identical; nothing is compared", file=sys.stderr)
        return 2

    dataiku.verify_dataset(args.dataset.resolve())
    positive, _ = dataiku.load_documents(args.dataset.resolve())
    negative, _ = runner.load_negative_documents(args.negative_corpus.resolve())
    generated = []
    if args.agentic:
        import agentic_layers as agentic
        generated = [item.to_document() for item in agentic.generate(agentic.PUBLISHED_PARTITION)]
    documents = (positive + negative + generated)[: args.limit]

    env = dict(os.environ)
    env["GAZE_BENCH_AUDIT_ROWS"] = "1"
    env.pop("GAZE_BENCH_RANDOM_SESSION", None)
    if args.policy:
        env["GAZE_BENCH_POLICY"] = str(args.policy.resolve())
    command = ["--config", args.config]

    mismatches = []
    timings = []
    audit_rows = 0
    errors = 0
    roles: dict[str, int] = {}
    traced = 0
    with BenchSubprocess([str(args.base), *command], cwd=ROOT, env=env) as base, \
            BenchSubprocess([str(args.candidate), *command], cwd=ROOT, env=env) as candidate:
        for index, doc in enumerate(documents, 1):
            request = {
                "fixture_id": doc.uid,
                "locale_chain": doc.locale_chain,
                "text": doc.text,
                "session_hex": SESSION_HEX,
            }
            left = base.exchange(request)
            right = candidate.exchange(request)
            if "audit_rows" not in left and "pipeline_error_code" not in left:
                raise SystemExit("base response has no audit_rows; is GAZE_BENCH_AUDIT_ROWS wired?")
            if args.ignore_lineage:
                left, _ = strip_lineage(left, expect=False)
                right, counted = strip_lineage(right, expect=True)
                traced += bool(counted)
                for role, count in counted.items():
                    roles[role] = roles.get(role, 0) + count
            fields = [key for key in COMPARED if left.get(key) != right.get(key)]
            if fields:
                mismatches.append({"id": doc.uid, "fields": fields})
            audit_rows += len(left.get("audit_rows") or ())
            errors += "pipeline_error_code" in left
            if "timing" in left and "timing" in right:
                timings.append((left["timing"]["clean_ms"], right["timing"]["clean_ms"]))
            if index % 250 == 0:
                print(f"compared {index}/{len(documents)}", file=sys.stderr, flush=True)

    print(json.dumps({
        "config": args.config,
        "policy": str(args.policy) if args.policy else None,
        "policy_sha256": sha256(args.policy) if args.policy else None,
        "binaries_sha256": hashes,
        "documents": len(documents),
        "audit_rows": audit_rows,
        "pipeline_errors": errors,
        "differing_documents": len(mismatches),
        "lineage": {"documents_with_trace": traced, "roles": dict(sorted(roles.items()))}
        if args.ignore_lineage else None,
        "latency": latency(timings),
        "mismatches": mismatches[:50],
    }, indent=2))
    return 1 if mismatches else 0


if __name__ == "__main__":
    raise SystemExit(main())
