#!/usr/bin/env python3
"""Score any system's spans on the public PII-TRACE subset with the dataset's own metrics.

The dataset card documents exact typed-span P/R/F1 over (turn, source, start, end, label),
micro and macro, plus untyped exact-span F1. The paper's headline table is label-agnostic
character P/R/F1. Perplexity publishes no scorer, so both are implemented here from those
definitions. Nothing here reproduces a published number: the paper's figures are for the
1,922-document, 13-language test split, which is not public (see loaders.PII_TRACE).

Typed matching for tools with their own labels goes through the composed map that
theirbench.py writes (`labels.<family>.json`: tool label -> dataset labels covered). A
predicted span is a typed hit when its (message, start, end) equals a gold span whose label
the tool label covers. For macro F1 a predicted span is assigned the gold label it hit; a
miss is assigned the first (alphabetical) dataset label its tool label covers, and a span
whose label covers nothing counts as a false positive in the micro totals only.

Only aggregate numbers are written.
"""

from __future__ import annotations

import argparse
import json
import platform
import sys
from collections import defaultdict
from pathlib import Path
from typing import Mapping, Sequence

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import loaders  # noqa: E402

LABELS = ("private_person", "private_date", "private_url", "private_address", "account_number",
          "private_email", "private_phone", "other_pii", "secret")


def prf(tp: float, fp: float, fn: float, beta: float = 1.0) -> dict[str, float]:
    precision = tp / (tp + fp) if tp + fp else 0.0
    recall = tp / (tp + fn) if tp + fn else 0.0
    b2 = beta * beta
    f = (1 + b2) * precision * recall / (b2 * precision + recall) if precision + recall else 0.0
    return {"precision": round(precision, 4), "recall": round(recall, 4), f"f{beta:g}": round(f, 4)}


def union(spans: Sequence[tuple[int, int]]) -> list[tuple[int, int]]:
    merged: list[list[int]] = []
    for start, end in sorted(spans):
        if merged and start <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], end)
        else:
            merged.append([start, end])
    return [(start, end) for start, end in merged]


def overlap(left: Sequence[tuple[int, int]], right: Sequence[tuple[int, int]]) -> int:
    total, i, j = 0, 0, 0
    while i < len(left) and j < len(right):
        total += max(0, min(left[i][1], right[j][1]) - max(left[i][0], right[j][0]))
        if left[i][1] < right[j][1]:
            i += 1
        else:
            j += 1
    return total


def score_messages(
    messages: Sequence[tuple[str, str, Sequence[tuple[int, int, str]]]],
    predictions: Sequence[Sequence[Sequence[object]]],
    composed: Mapping[str, Sequence[str]],
) -> dict[str, object]:
    if len(messages) != len(predictions):
        raise ValueError("predictions must cover every message in order")
    char_tp = char_pred = char_gold = 0
    typed_tp = typed_pred = typed_gold = untyped_tp = untyped_pred = 0
    per_label = {label: defaultdict(int) for label in LABELS}
    for (uid, text, gold), found in zip(messages, predictions):
        spans = sorted({(int(start), int(end), str(label)) for start, end, label in found})
        unknown = sorted({label for _, _, label in spans} - composed.keys())
        if unknown:
            raise ValueError(f"{uid}: unmapped labels {unknown}; review the composed label map")
        if any(not 0 <= start < end <= len(text) for start, end, _ in spans):
            raise ValueError(f"{uid}: prediction outside the message")
        predicted_chars = union([(start, end) for start, end, _ in spans])
        gold_chars = union([(start, end) for start, end, _ in gold])
        char_tp += overlap(predicted_chars, gold_chars)
        char_pred += sum(end - start for start, end in predicted_chars)
        char_gold += sum(end - start for start, end in gold_chars)
        gold_by_span: dict[tuple[int, int], list[str]] = defaultdict(list)
        for start, end, label in gold:
            gold_by_span[(start, end)].append(label)
        matched_gold: set[tuple[int, int, str]] = set()
        untyped_hits = {(start, end) for start, end, _ in spans} & gold_by_span.keys()
        untyped_tp += len(untyped_hits)
        untyped_pred += len({(start, end) for start, end, _ in spans})
        for start, end, label in spans:
            covered = sorted(composed[label])
            hit = next((g for g in gold_by_span.get((start, end), ()) if g in covered
                        and (start, end, g) not in matched_gold), None)
            typed_pred += 1
            if hit is not None:
                typed_tp += 1
                matched_gold.add((start, end, hit))
                per_label[hit]["tp"] += 1
            elif covered:
                per_label[covered[0]]["fp"] += 1
        typed_gold += len(gold)
        for start, end, label in gold:
            if (start, end, label) not in matched_gold:
                per_label[label]["fn"] += 1
    macro = {label: prf(v["tp"], v["fp"], v["fn"])["f1"] for label, v in per_label.items()}
    return {
        "messages": len(messages),
        "char_level_label_agnostic": {**prf(char_tp, char_pred - char_tp, char_gold - char_tp),
                                      "f2": prf(char_tp, char_pred - char_tp, char_gold - char_tp, 2)["f2"],
                                      "tp_chars": char_tp, "predicted_chars": char_pred, "gold_chars": char_gold},
        "exact_typed_micro": {**prf(typed_tp, typed_pred - typed_tp, typed_gold - typed_tp),
                              "tp": typed_tp, "predicted": typed_pred, "gold": typed_gold},
        "exact_typed_macro_f1": round(sum(macro.values()) / len(macro), 4),
        "exact_typed_f1_by_label": macro,
        "exact_untyped_micro": {**prf(untyped_tp, untyped_pred - untyped_tp, typed_gold - untyped_tp),
                                "tp": untyped_tp, "predicted": untyped_pred, "gold": typed_gold},
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--data", type=Path, required=True, help="data/train.parquet of perplexity-ai/PII-TRACE")
    parser.add_argument("--predictions", type=Path, required=True, help="JSONL: {index, spans: [[start, end, label], ...]}")
    parser.add_argument("--labels", type=Path, required=True, help="theirbench.py composed map for this tool family")
    parser.add_argument("--system", required=True)
    parser.add_argument("--max-messages", type=int, help="smoke only; never published")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    messages = loaders.pii_trace_messages(args.data)
    rows = [json.loads(line) for line in args.predictions.read_text(encoding="utf-8").splitlines()]
    if [row["index"] for row in rows] != list(range(len(messages))):
        raise SystemExit("predictions must cover every message in order")
    predictions = [row["spans"] for row in rows]
    if args.max_messages:
        messages, predictions = messages[:args.max_messages], predictions[:args.max_messages]
    composed = json.loads(args.labels.read_text(encoding="utf-8"))
    scored = score_messages(messages, predictions, composed)
    report = {
        "benchmark": "PII-TRACE public subset", "dataset": loaders.PII_TRACE, "system": args.system,
        "smoke_limit": args.max_messages,
        "scorer": "dataset card exact typed/untyped span P/R/F1 and the paper's label-agnostic character P/R/F1, "
                  "implemented from their definitions (no vendor scorer is published)",
        "overall": {"char_f1": scored["char_level_label_agnostic"]["f1"],
                    "char_f2": scored["char_level_label_agnostic"]["f2"],
                    "exact_typed_micro_f1": scored["exact_typed_micro"]["f1"],
                    "exact_typed_macro_f1": scored["exact_typed_macro_f1"],
                    "exact_untyped_micro_f1": scored["exact_untyped_micro"]["f1"]},
        "scored": {**scored, "char_f1": scored["char_level_label_agnostic"]["f1"]},
        "versions": {"python": platform.python_version()}, "hardware": platform.platform(),
    }
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report["overall"]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
