"""Aggregate record-context byte changes without retaining document values.

The ordinary oracle aggregate has no per-document traces, so this module runs
inside a fresh paired oracle replay. Only class/kind counters leave memory.
"""

from __future__ import annotations

import hashlib
import re
from collections import Counter
from dataclasses import dataclass
from typing import Iterable, Mapping

import gaze_bench_score as score

RECORD_SOURCE = re.compile(r"^(?:context/|dictionary:)?record-v2-([0-9a-f]{64})-\d+(?:\[#\d+\])?$")
FIELDS = (
    "gold_recovered_bytes",
    "eligible_recovered_bytes",
    "false_positive_added_bytes",
    "gold_lost_bytes",
    "eligible_lost_bytes",
    "false_positive_removed_bytes",
)


def canonical_class(class_name: str) -> str:
    return {
        "Email": "email",
        "Name": "name",
        "Location": "location",
        "Organization": "organization",
    }.get(class_name, class_name)


def intersect_intervals(
    left: Iterable[tuple[int, int]], right: Iterable[tuple[int, int]]
) -> list[tuple[int, int]]:
    result = []
    for a, b in left:
        for c, d in right:
            start, end = max(a, c), min(b, d)
            if start < end:
                result.append((start, end))
    return score.merge_intervals(result)


def scored_view(document: score.Document, response: Mapping[str, object]) -> list[tuple[int, int]]:
    if "pipeline_error_code" in response:
        raise ValueError("record attribution requires successful paired responses")
    predictions = score.final_trace_predictions(document, response)
    _, ignored, selected = score.contract_scoring_view(document, predictions)
    return score.subtract_intervals(
        score.merge_intervals((span.start, span.end) for span in selected), ignored
    )


def match_group_and_kind(
    text: str, class_name: str, values: list[tuple[str, str]], common_words: frozenset[str]
) -> tuple[str, str]:
    candidates = []
    for record_class, raw in values:
        if record_class != class_name:
            continue
        canonical = " ".join(raw.split())
        collapsed = " ".join(text.split())
        if collapsed.casefold() != canonical.casefold():
            continue
        if (
            record_class == "Name"
            and len(canonical.split()) == 1
            and canonical.casefold() in common_words
        ):
            kind = "corroborated_single"
        elif text == raw:
            kind = "exact"
        elif collapsed == canonical:
            kind = "whitespace_flexible"
        elif text.casefold() == canonical.casefold():
            kind = "case_folded"
        else:
            kind = "whitespace_case_folded"
        group = (
            "name_single" if len(canonical.split()) == 1 else "name_multi"
        ) if record_class == "Name" else (
            "address_part" if record_class == "Location" else canonical_class(record_class)
        )
        candidates.append((group, kind))
    if not candidates:
        return canonical_class(class_name), "unmatched_term"
    if len(set(candidates)) != 1:
        return "unattributed", "ambiguous_term"
    return candidates[0]


def trace_bucket(
    document: score.Document,
    item: Mapping[str, object],
    values: list[tuple[str, str]],
    common_words: frozenset[str],
) -> tuple[str, str]:
    digests = {
        hashlib.sha256(canonical_class(class_name).encode("utf-8")).hexdigest(): class_name
        for class_name, _ in values
    }
    provenance = item.get("provenance")
    sources = provenance.get("source_ids", []) if isinstance(provenance, dict) else []
    classes = {
        digests[match.group(1)]
        for source in sources
        if isinstance(source, str)
        if (match := RECORD_SOURCE.fullmatch(source)) and match.group(1) in digests
    }
    if len(classes) != 1:
        return "unattributed", "no_unique_record_source"
    class_name = next(iter(classes))
    text = document.text.encode("utf-8")[item["raw_start"] : item["raw_end"]].decode("utf-8")
    return match_group_and_kind(text, class_name, values, common_words)


def add_bytes(
    totals: Counter[tuple[str, str, str, str]],
    bucket: tuple[str, str, str],
    segments: Iterable[tuple[int, int]],
    gold: list[tuple[int, int]],
    eligible: list[tuple[int, int]],
    *,
    added: bool,
) -> None:
    segments = score.merge_intervals(segments)
    if not segments:
        return
    group, kind, population = bucket
    gold_bytes = score.intersection_length(segments, gold)
    eligible_bytes = score.intersection_length(segments, eligible)
    fp_bytes = score.interval_length(segments) - gold_bytes
    suffix = "recovered" if added else "lost"
    totals[(group, kind, population, f"gold_{suffix}_bytes")] += gold_bytes
    totals[(group, kind, population, f"eligible_{suffix}_bytes")] += eligible_bytes
    fp_field = "false_positive_added_bytes" if added else "false_positive_removed_bytes"
    totals[(group, kind, population, fp_field)] += fp_bytes


def attribute_document(
    document: score.Document,
    baseline_predicted: list[tuple[int, int]],
    response: Mapping[str, object],
    values: list[tuple[str, str]],
    common_words: frozenset[str],
    eligible_labels: frozenset[str],
    *,
    decoy: bool,
) -> Counter[tuple[str, str, str, str]]:
    """Attribute changed scored bytes; unassignable changes get an explicit row."""
    record_predicted = scored_view(document, response)
    new = score.subtract_intervals(record_predicted, baseline_predicted)
    removed = score.subtract_intervals(baseline_predicted, record_predicted)
    gold = score.merge_intervals((span.start, span.end) for span in document.spans)
    encoded = document.text.encode("utf-8")
    eligible_values = {value for _, value in values}
    eligible = score.merge_intervals(
        (span.start, span.end)
        for span in document.spans
        if span.label in eligible_labels
        if encoded[span.start : span.end].decode("utf-8") in eligible_values
    )
    population = "decoy" if decoy else "non_decoy"
    totals: Counter[tuple[str, str, str, str]] = Counter()
    assigned: list[tuple[int, int]] = []
    trace = response["final_protection_trace"]
    if not isinstance(trace, list):
        raise ValueError("record attribution requires a final protection trace")
    for item in trace:
        if not isinstance(item, dict):
            raise ValueError("invalid record attribution trace item")
        segments = intersect_intervals(
            [(item["raw_start"], item["raw_end"])], new
        )
        segments = score.subtract_intervals(segments, score.merge_intervals(assigned))
        if not segments:
            continue
        group, kind = trace_bucket(document, item, values, common_words)
        add_bytes(totals, (group, kind, population), segments, gold, eligible, added=True)
        assigned.extend(segments)
    leftovers = score.subtract_intervals(new, score.merge_intervals(assigned))
    add_bytes(
        totals, ("unattributed", "trace_gap", population), leftovers, gold, eligible, added=True
    )
    add_bytes(
        totals, ("unattributed", "baseline_removed", population), removed, gold, eligible,
        added=False,
    )
    return totals


@dataclass
class AttributionRecorder:
    common_words: frozenset[str]
    eligible_labels: frozenset[str]
    baseline: dict[str, list[tuple[int, int]]]
    totals: Counter[tuple[str, str, str, str]]

    @classmethod
    def create(
        cls, common_words: frozenset[str], eligible_labels: frozenset[str]
    ) -> "AttributionRecorder":
        return cls(common_words, eligible_labels, {}, Counter())

    def record_baseline(self, document: score.Document, response: Mapping[str, object]) -> None:
        self.baseline[document.uid] = scored_view(document, response)

    def record_candidate(
        self, document: score.Document, response: Mapping[str, object],
        values: list[tuple[str, str]], *, decoy: bool,
    ) -> None:
        baseline = self.baseline.pop(document.uid)
        self.totals.update(attribute_document(
            document, baseline, response, values, self.common_words, self.eligible_labels,
            decoy=decoy,
        ))

    def result(
        self, *, leaked_fall: int, false_positive_rise: int,
        eligible_leak_fall: int | None = None,
    ) -> dict[str, object]:
        if self.baseline:
            raise ValueError("record attribution has unpaired baseline documents")
        sums = {
            field: sum(value for (*_, metric), value in self.totals.items() if metric == field)
            for field in FIELDS
        }
        if sums["gold_recovered_bytes"] - sums["gold_lost_bytes"] != leaked_fall:
            raise ValueError("record attribution does not reconcile leaked bytes")
        if (
            sums["false_positive_added_bytes"] - sums["false_positive_removed_bytes"]
            != false_positive_rise
        ):
            raise ValueError("record attribution does not reconcile false positives")
        if eligible_leak_fall is not None and (
            sums["eligible_recovered_bytes"] - sums["eligible_lost_bytes"] != eligible_leak_fall
        ):
            raise ValueError("record attribution does not reconcile eligible leaked bytes")
        buckets = sorted({key[:3] for key in self.totals})
        rows = [
            {"record_class": group, "match_kind": kind, "population": population,
             **{field: self.totals[(group, kind, population, field)] for field in FIELDS}}
            for group, kind, population in buckets
        ]
        return {"schema_version": 1, "rows": rows, "totals": sums}
