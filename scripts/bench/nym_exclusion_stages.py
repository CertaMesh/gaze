#!/usr/bin/env python3
"""Where every model-positive Nym byte goes in a single-pass arm (solo todo 3738, Stage A).

`single_pass_nym_paired.py run` runs the single-pass arms with `clean_for_bench
--exclusion-trace`, which records, per document, the candidates the Nym adapters put into
the pool (normalized and raw byte ranges) and every audit row naming a Nym adapter. This
script joins that trace with the same run's final protection trace and follows each
model-positive span through the stages between the model and the output:

1. normalization mapping: the span maps back onto raw bytes (a joiner or fullwidth
   character inside it changes the byte count, never drops the span);
2. pool admission (locale basis `format`, score floor): a candidate without any audit row
   never reached arbitration;
3. validator veto: a row decided by `ValidatorVeto`;
4. arbitration: a loser row decided by any rung other than containment precedence;
5. containment rung: a loser row decided by `ContainmentPrecedence` (a rule container took
   the whole span);
6. recovery: a candidate that lost in primary resolution yet leaves whole;
7. action policy: a winner row whose action is `Preserve`;
8. residual net: bytes a Nym-class fragment covers (`primary_pipeline.residual` rows);
9. publication: bytes of the span that leave raw.

Bytes are scored on the output (the final protection trace), never on manifest arithmetic.
A byte is *lost* when it leaves raw; each lost byte is attributed to the stage that decided
its span. The target is 0 lost bytes.

    uv run --project scripts/bench python scripts/bench/nym_exclusion_stages.py \
        --out target/bench-data/single-pass --arm single-pass-nym
"""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Mapping, Sequence

import gaze_bench_score as score
import single_pass_nym_paired as paired

STAGES = (
    "normalization_mapping",
    "pool_admission",
    "validator_veto",
    "arbitration",
    "containment_rung",
    "recovery",
    "action_policy",
    "residual_net",
    "publication",
)


def load_trace(path: Path) -> dict[str, dict[str, object]]:
    documents = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        row = json.loads(line)
        documents[row["fixture_id"]] = row
    return documents


def covered(intervals: Sequence[tuple[int, int]], span: tuple[int, int]) -> int:
    return score.intersection_length(intervals, [span])


def classify(
    document: score.Document,
    traced: Mapping[str, object],
    response: Mapping[str, object],
) -> list[dict[str, object]]:
    items = [
        (
            (item["raw_start"], item["raw_end"]),
            item["class"],
            item["action"],
            tuple(item["provenance"]["source_ids"]),
        )
        for item in response["final_protection_trace"]
    ]
    rows_by_id: dict[str, list[Mapping[str, object]]] = defaultdict(list)
    for row in traced["rows"]:
        rows_by_id[row["recognizer_id"]].append(row)
    def is_whole(candidate: Mapping[str, object], own_class: str | None) -> bool:
        return any(
            span == tuple(candidate["raw"]) and candidate["id"] in sources and cls == own_class
            for span, cls, _, sources in items
        )

    class_of = {row["recognizer_id"]: row["class"] for row in traced["rows"]}
    partial_by_id = Counter(
        candidate["id"]
        for candidate in traced["candidates"]
        if not is_whole(candidate, class_of.get(candidate["id"]))
    )
    fates = []
    for candidate in traced["candidates"]:
        identifier = candidate["id"]
        raw = tuple(candidate["raw"])
        normalized = tuple(candidate["normalized"])
        rows = rows_by_id.get(identifier, [])
        own_class = next((row["class"] for row in rows), None)
        whole = is_whole(candidate, own_class)
        own_items = score.merge_intervals(
            span for span, cls, _, sources in items if cls == own_class and identifier in sources
        )
        other_items = score.merge_intervals(
            span for span, cls, _, sources in items if not (cls == own_class and identifier in sources)
        )
        all_items = score.merge_intervals([*own_items, *other_items])
        length = raw[1] - raw[0]
        raw_bytes = length - covered(all_items, raw)
        loser_tiers = Counter(row["decided_by"] for row in rows if row["conflict_loser"])
        winner_actions = Counter(row["action"] for row in rows if not row["conflict_loser"] and not row["provenance_stage"])
        residual_rows = sum(row["provenance_stage"] == "primary_pipeline.residual" for row in rows)
        if not rows:
            stage = "pool_admission"
        elif "ValidatorVeto" in loser_tiers:
            stage = "validator_veto"
        elif whole and loser_tiers and partial_by_id[identifier] == 0:
            # Every loser row of this adapter here belongs to a span that still left whole:
            # it lost in primary resolution and came back in a gap.
            stage = "recovery"
        elif whole and winner_actions.get("Preserve"):
            stage = "action_policy"
        elif whole:
            stage = "publication"
        elif "ContainmentPrecedence" in loser_tiers:
            stage = "containment_rung"
        elif loser_tiers:
            stage = "arbitration"
        elif winner_actions.get("Preserve"):
            stage = "action_policy"
        else:
            stage = "publication"
        fates.append(
            {
                "document": document.uid,
                "id": identifier,
                "bytes": length,
                "normalization_changed_bytes": (raw[1] - raw[0]) != (normalized[1] - normalized[0]),
                "decided_at": stage,
                "whole": whole,
                "own_class_bytes": covered(own_items, raw),
                "residual_rows": residual_rows,
                "raw_bytes": raw_bytes,
                "loser_tiers": dict(loser_tiers),
            }
        )
    return fates


def enumerate_stages(out: Path, arm: str) -> dict[str, object]:
    repo_root = Path(__file__).resolve().parents[2]
    documents = {document.uid: document for document in paired.load_population(repo_root)}
    trace = load_trace(out / f"{arm}.exclusion.jsonl")
    responses = paired.load_responses(out / f"{arm}.jsonl")
    if set(trace) != set(documents):
        raise RuntimeError("the exclusion trace does not cover the requested population")
    fates = []
    refused = []
    for uid, document in documents.items():
        response = responses[uid]
        if paired.is_refusal(response):
            refused.append(uid)
            continue
        fates.extend(classify(document, trace[uid], response))
    table = {}
    for stage in STAGES:
        rows = [fate for fate in fates if fate["decided_at"] == stage]
        table[stage] = {
            "spans_decided": len(rows),
            "bytes_decided": sum(fate["bytes"] for fate in rows),
            "bytes_left_under_a_nym_token": sum(fate["own_class_bytes"] for fate in rows),
            "bytes_left_under_another_token": sum(
                fate["bytes"] - fate["own_class_bytes"] - fate["raw_bytes"] for fate in rows
            ),
            "bytes_lost_raw": sum(fate["raw_bytes"] for fate in rows),
        }
    table["normalization_mapping"]["spans_whose_bytes_normalization_changed"] = sum(
        fate["normalization_changed_bytes"] for fate in fates
    )
    beaten = [fate for fate in fates if not fate["whole"]]
    table["residual_net"] = {
        "note": "spans that did not leave whole: bytes a Nym-class fragment still covers",
        "fragment_rows": sum(fate["residual_rows"] for fate in fates),
        "spans": len(beaten),
        "bytes_covered_by_nym_fragments": sum(fate["own_class_bytes"] for fate in beaten),
        "bytes_lost_raw": sum(fate["raw_bytes"] for fate in beaten),
    }
    by_label: dict[str, Counter[str]] = defaultdict(Counter)
    for fate in fates:
        by_label[fate["id"]][fate["decided_at"]] += 1
        by_label[fate["id"]]["bytes"] += fate["bytes"]
        by_label[fate["id"]]["bytes_lost_raw"] += fate["raw_bytes"]
    lost = [fate for fate in fates if fate["raw_bytes"]]
    return {
        "arm": arm,
        "model_positive_spans": len(fates),
        "model_positive_bytes": sum(fate["bytes"] for fate in fates),
        "refused_documents": refused,
        "stages": table,
        "by_adapter": {label: dict(counts) for label, counts in sorted(by_label.items())},
        "lost_bytes_total": sum(fate["raw_bytes"] for fate in fates),
        "lost_spans": [
            {key: fate[key] for key in ("document", "id", "decided_at", "raw_bytes", "loser_tiers")}
            for fate in lost
        ],
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--arm", default=paired.SINGLE_PASS, choices=(paired.SINGLE_PASS, paired.OBSERVED))
    args = parser.parse_args(argv)
    repo_root = Path(__file__).resolve().parents[2]
    out = args.out if args.out.is_absolute() else repo_root / args.out
    report = enumerate_stages(out, args.arm)
    (out / f"{args.arm}.exclusion-stages.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: report[key] for key in ("arm", "model_positive_spans", "model_positive_bytes", "lost_bytes_total", "stages")}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
