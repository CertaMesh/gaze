#!/usr/bin/env python3
"""Shared fail-closed scoring for Gaze synthetic PII benchmarks.

The benchmark is deliberately label-agnostic at its primary boundary: a PII
byte is safe only when Gaze replaces it. Entity and per-label recall remain as
diagnostics, but conventional NER F1 is not allowed to hide partial leaks.
"""

from __future__ import annotations

import copy
import hashlib
import json
import math
import os
import statistics
import subprocess
import sys
import time
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Mapping, Sequence


SCORECARD_SCHEMA_VERSION = 3
DEFAULT_SAMPLE_SEED = 20_260_710
SAMPLING_STRATEGY = "deterministic-stratified-language-region-v1"

# These are direct or account-linked identifiers. The remaining observed
# labels (date, time, age, gender, sex, title, amount, and currency) are still
# included in the all-PII score, but are reported separately as contextual PII.
DIRECT_IDENTIFIER_LABELS = frozenset(
    {
        "ACCOUNTNUM",
        "BANKNAME",
        "BUILDINGNUM",
        "CITY",
        "CREDITCARDNUMBER",
        "DRIVERLICENSENUM",
        "EMAIL",
        "GIVENNAME",
        "IDCARDNUM",
        "ORGANISATION",
        "PASSPORTNUM",
        "SOCIALNUM",
        "STREET",
        "SURNAME",
        "TAXNUM",
        "TELEPHONENUM",
        "URL",
        "ZIPCODE",
    }
)

DEFAULT_CONFIGS = (
    "rule-floor-extended",
    "pass2-ner",
    "full-stack-kiji-resolve",
)


@dataclass(frozen=True)
class Span:
    start: int
    end: int
    label: str


@dataclass(frozen=True)
class Document:
    uid: str
    text: str
    language: str
    region: str
    source_dataset: str
    spans: tuple[Span, ...]

    @property
    def locale_chain(self) -> list[str]:
        locale = f"{self.language}-{self.region}" if self.region else self.language
        return [locale, "global"]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def char_to_byte_offsets(text: str) -> list[int]:
    offsets = [0]
    byte_offset = 0
    for character in text:
        byte_offset += len(character.encode("utf-8"))
        offsets.append(byte_offset)
    return offsets


def population_summary(documents: Sequence[Document]) -> dict[str, object]:
    labels = Counter(span.label for document in documents for span in document.spans)
    languages = Counter(document.language for document in documents)
    regions = Counter(
        f"{document.language}-{document.region}"
        if document.region
        else document.language
        for document in documents
    )
    sources = Counter(document.source_dataset for document in documents)
    return {
        "documents": len(documents),
        "entities": sum(labels.values()),
        "labels": dict(sorted(labels.items())),
        "languages": dict(sorted(languages.items())),
        "regions": dict(sorted(regions.items())),
        "sources": dict(sorted(sources.items())),
        "negative_only_documents": sum(not document.spans for document in documents),
    }


def _stratum(document: Document) -> tuple[str, str]:
    return document.language, document.region


def _document_rank(seed: int, document: Document) -> bytes:
    payload = json.dumps(
        [seed, document.language, document.region, document.uid],
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(payload).digest()


def _proportional_allocation(
    capacities: Mapping[tuple[str, str], int], budget: int
) -> dict[tuple[str, str], int]:
    allocations = {key: 0 for key in capacities}
    if budget <= 0:
        return allocations
    capacity_total = sum(capacities.values())
    if budget > capacity_total:
        raise ValueError("sample allocation exceeds available population")
    quotas = {
        key: budget * capacity / capacity_total
        for key, capacity in capacities.items()
    }
    for key, quota in quotas.items():
        allocations[key] = min(capacities[key], math.floor(quota))
    remaining = budget - sum(allocations.values())
    order = sorted(
        capacities,
        key=lambda key: (
            -(quotas[key] - math.floor(quotas[key])),
            -capacities[key],
            key,
        ),
    )
    for key in order:
        if remaining == 0:
            break
        if allocations[key] < capacities[key]:
            allocations[key] += 1
            remaining -= 1
    if remaining:
        raise RuntimeError("deterministic stratum allocation left unassigned slots")
    return allocations


def _allocate_strata(
    strata: Mapping[tuple[str, str], Sequence[Document]], sample_size: int
) -> dict[tuple[str, str], int]:
    capacities = {key: len(value) for key, value in strata.items()}
    if sample_size >= len(capacities):
        allocations = {key: 1 for key in capacities}
        remaining_capacities = {key: value - 1 for key, value in capacities.items()}
        extra = _proportional_allocation(
            remaining_capacities, sample_size - len(capacities)
        )
        return {key: allocations[key] + extra[key] for key in capacities}
    return _proportional_allocation(capacities, sample_size)


def stratified_sample(
    documents: Sequence[Document],
    max_documents: int | None,
    seed: int = DEFAULT_SAMPLE_SEED,
) -> tuple[list[Document], dict[str, object]]:
    if type(seed) is not int:
        raise TypeError("sampling seed must be an integer")
    if not documents:
        raise ValueError("available population is empty")
    if max_documents is not None and (
        type(max_documents) is not int or max_documents <= 0
    ):
        raise ValueError("--max-documents must be greater than zero")
    uids = [document.uid for document in documents]
    if len(set(uids)) != len(uids):
        raise ValueError("document IDs must be unique before sampling")

    sample_size = min(max_documents or len(documents), len(documents))
    if sample_size == len(documents):
        evaluated = list(documents)
    else:
        strata: defaultdict[tuple[str, str], list[Document]] = defaultdict(list)
        for document in documents:
            strata[_stratum(document)].append(document)
        allocation = _allocate_strata(strata, sample_size)
        evaluated = []
        for key in sorted(strata):
            ranked = sorted(
                strata[key], key=lambda document: (_document_rank(seed, document), document.uid)
            )
            evaluated.extend(ranked[: allocation[key]])
        evaluated.sort(key=lambda document: (_document_rank(seed, document), document.uid))

    evaluated_ids = sorted(document.uid for document in evaluated)
    digest_payload = json.dumps(
        evaluated_ids, ensure_ascii=False, separators=(",", ":")
    ).encode("utf-8")
    report: dict[str, object] = {
        "strategy": SAMPLING_STRATEGY,
        "seed": seed,
        "requested_max_documents": max_documents,
        "available_population": population_summary(documents),
        "evaluated_population": population_summary(evaluated),
        "evaluated_document_ids": evaluated_ids,
        "evaluated_document_ids_digest": {
            "algorithm": "sha256",
            "value": hashlib.sha256(digest_payload).hexdigest(),
        },
    }
    return evaluated, report


class ResponseValidationError(RuntimeError):
    """The Rust benchmark producer violated its closed schema-v3 wire contract."""


def _expect_object(value: object, context: str) -> dict[str, object]:
    if not isinstance(value, dict) or not all(
        isinstance(key, str) for key in value
    ):
        raise ResponseValidationError(f"{context}: expected an object")
    return value


def _expect_exact_keys(
    value: object, required: frozenset[str], context: str
) -> dict[str, object]:
    result = _expect_object(value, context)
    missing = sorted(required - result.keys())
    unknown = sorted(result.keys() - required)
    if missing or unknown:
        parts = []
        if missing:
            parts.append(f"missing fields {missing}")
        if unknown:
            parts.append(f"unknown fields {unknown}")
        raise ResponseValidationError(f"{context}: {'; '.join(parts)}")
    return result


def _expect_string(value: object, context: str) -> str:
    if not isinstance(value, str):
        raise ResponseValidationError(f"{context}: expected a string")
    return value


def _expect_bool(value: object, context: str) -> bool:
    if type(value) is not bool:
        raise ResponseValidationError(f"{context}: expected a boolean")
    return value


def _expect_int(value: object, context: str, *, minimum: int = 0) -> int:
    if type(value) is not int or value < minimum:
        raise ResponseValidationError(
            f"{context}: expected an integer greater than or equal to {minimum}"
        )
    return value


def _expect_number(value: object, context: str) -> float:
    if type(value) not in {int, float} or not math.isfinite(float(value)):
        raise ResponseValidationError(f"{context}: expected a finite number")
    if float(value) < 0:
        raise ResponseValidationError(f"{context}: expected a non-negative number")
    return float(value)


def _expect_list(value: object, context: str) -> list[object]:
    if not isinstance(value, list):
        raise ResponseValidationError(f"{context}: expected an array")
    return value


MANIFEST_SPAN_FIELDS = frozenset(
    {"raw_start", "raw_end", "clean_start", "clean_end", "class"}
)
LEAK_SUSPECT_FIELDS = frozenset(
    {
        "clean_start",
        "clean_end",
        "action_start",
        "action_end",
        "class",
        "safety_net_id",
        "kind",
    }
)
SAFETY_NET_STATS_FIELDS = frozenset(
    {
        "suspect_count",
        "uncovered_count",
        "partial_bleed_count",
        "class_mismatch_count",
        "locale_skipped_count",
    }
)
RESTORE_FIELDS = frozenset(
    {
        "exact",
        "decision",
        "unknown_token_count",
        "manifest_bypass_count",
        "fresh_pii_detected_count",
        "phase_execution_mask",
    }
)
MANIFEST_INTEGRITY_FIELDS = frozenset(
    {
        "spans",
        "invalid_clean_bounds",
        "invalid_raw_bounds",
        "overlapping_clean_spans",
        "non_monotonic_raw_spans",
        "token_restore_failures",
        "raw_value_mismatches",
    }
)
SUCCESS_TIMING_FIELDS = frozenset(
    {"total_ms", "pass1_ms", "pass2_ms", "pass3_ms", "restore_ms", "post_policy_scan_ms"}
)
SUCCESS_RESPONSE_FIELDS = frozenset(
    {
        "fixture_id",
        "clean_text",
        "manifest_spans",
        "pre_safety_text_len",
        "pre_safety_manifest_spans",
        "leak_suspects",
        "safety_net_mode",
        "strict_would_reject",
        "initial_safety_net_stats",
        "post_policy_safety_net_stats",
        "restore",
        "manifest_integrity",
        "timing",
    }
)
PIPELINE_ERROR_RESPONSE_FIELDS = frozenset(
    {"fixture_id", "pipeline_error_stage", "pipeline_error_code", "timing"}
)


def _validate_manifest_span(value: object, context: str) -> None:
    span = _expect_exact_keys(value, MANIFEST_SPAN_FIELDS, context)
    for field in ("raw_start", "raw_end", "clean_start", "clean_end"):
        _expect_int(span[field], f"{context}.{field}")
    _expect_string(span["class"], f"{context}.class")


def _validate_leak_suspect(value: object, context: str) -> None:
    suspect = _expect_exact_keys(value, LEAK_SUSPECT_FIELDS, context)
    for field in ("clean_start", "clean_end", "action_start", "action_end"):
        _expect_int(suspect[field], f"{context}.{field}")
    for field in ("class", "safety_net_id", "kind"):
        _expect_string(suspect[field], f"{context}.{field}")


def _validate_safety_net_stats(value: object, context: str) -> None:
    stats = _expect_exact_keys(value, SAFETY_NET_STATS_FIELDS, context)
    for field in SAFETY_NET_STATS_FIELDS:
        _expect_int(stats[field], f"{context}.{field}")


def _validate_restore(value: object, context: str) -> None:
    restore = _expect_exact_keys(value, RESTORE_FIELDS, context)
    _expect_bool(restore["exact"], f"{context}.exact")
    _expect_string(restore["decision"], f"{context}.decision")
    for field in RESTORE_FIELDS - {"exact", "decision"}:
        _expect_int(restore[field], f"{context}.{field}")


def _validate_manifest_integrity(value: object, context: str) -> None:
    integrity = _expect_exact_keys(value, MANIFEST_INTEGRITY_FIELDS, context)
    for field in MANIFEST_INTEGRITY_FIELDS:
        _expect_int(integrity[field], f"{context}.{field}")


def _validate_success_timing(value: object, context: str) -> None:
    timing = _expect_exact_keys(value, SUCCESS_TIMING_FIELDS, context)
    for field in SUCCESS_TIMING_FIELDS:
        if field == "pass2_ms" and timing[field] is None:
            continue
        _expect_number(timing[field], f"{context}.{field}")


def validate_response(document: Document, value: object) -> dict[str, object]:
    response = _expect_object(value, f"{document.uid}: response")
    if "pipeline_error_code" in response:
        response = _expect_exact_keys(
            response,
            PIPELINE_ERROR_RESPONSE_FIELDS,
            f"{document.uid}: pipeline error response",
        )
        fixture_id = _expect_string(response["fixture_id"], "fixture_id")
        _expect_string(response["pipeline_error_stage"], "pipeline_error_stage")
        _expect_string(response["pipeline_error_code"], "pipeline_error_code")
        timing = _expect_exact_keys(
            response["timing"], frozenset({"total_ms"}), "pipeline error timing"
        )
        _expect_number(timing["total_ms"], "pipeline error timing.total_ms")
    else:
        response = _expect_exact_keys(
            response, SUCCESS_RESPONSE_FIELDS, f"{document.uid}: success response"
        )
        fixture_id = _expect_string(response["fixture_id"], "fixture_id")
        _expect_string(response["clean_text"], "clean_text")
        manifest = _expect_list(response["manifest_spans"], "manifest_spans")
        for index, span in enumerate(manifest):
            _validate_manifest_span(span, f"manifest_spans[{index}]")
        pre_safety_length = response["pre_safety_text_len"]
        if pre_safety_length is not None:
            _expect_int(pre_safety_length, "pre_safety_text_len")
        pre_safety_manifest = response["pre_safety_manifest_spans"]
        if pre_safety_manifest is not None:
            for index, span in enumerate(
                _expect_list(pre_safety_manifest, "pre_safety_manifest_spans")
            ):
                _validate_manifest_span(span, f"pre_safety_manifest_spans[{index}]")
        suspects = _expect_list(response["leak_suspects"], "leak_suspects")
        for index, suspect in enumerate(suspects):
            _validate_leak_suspect(suspect, f"leak_suspects[{index}]")
        _expect_string(response["safety_net_mode"], "safety_net_mode")
        _expect_bool(response["strict_would_reject"], "strict_would_reject")
        _validate_safety_net_stats(
            response["initial_safety_net_stats"], "initial_safety_net_stats"
        )
        post_policy = response["post_policy_safety_net_stats"]
        if post_policy is not None:
            _validate_safety_net_stats(post_policy, "post_policy_safety_net_stats")
        _validate_restore(response["restore"], "restore")
        _validate_manifest_integrity(
            response["manifest_integrity"], "manifest_integrity"
        )
        _validate_success_timing(response["timing"], "timing")

    if fixture_id != document.uid:
        raise ResponseValidationError(
            f"runner response mismatch: expected {document.uid}, received {fixture_id}"
        )
    return response




def merge_intervals(intervals: Iterable[tuple[int, int]]) -> list[tuple[int, int]]:
    merged: list[tuple[int, int]] = []
    for start, end in sorted(intervals):
        if end <= start:
            continue
        if merged and start <= merged[-1][1]:
            previous_start, previous_end = merged[-1]
            merged[-1] = (previous_start, max(previous_end, end))
        else:
            merged.append((start, end))
    return merged


def interval_length(intervals: Sequence[tuple[int, int]]) -> int:
    return sum(end - start for start, end in intervals)


def intersection_length(
    left: Sequence[tuple[int, int]], right: Sequence[tuple[int, int]]
) -> int:
    total = 0
    left_index = 0
    right_index = 0
    while left_index < len(left) and right_index < len(right):
        left_start, left_end = left[left_index]
        right_start, right_end = right[right_index]
        total += max(0, min(left_end, right_end) - max(left_start, right_start))
        if left_end <= right_end:
            left_index += 1
        else:
            right_index += 1
    return total


def interval_is_covered(
    interval: tuple[int, int], covering: Sequence[tuple[int, int]]
) -> bool:
    start, end = interval
    return any(cover_start <= start and cover_end >= end for cover_start, cover_end in covering)


def interval_overlaps(
    interval: tuple[int, int], candidates: Sequence[tuple[int, int]]
) -> bool:
    start, end = interval
    return any(candidate_start < end and start < candidate_end for candidate_start, candidate_end in candidates)


def safe_ratio(numerator: int | float, denominator: int | float) -> float:
    if denominator == 0:
        return 1.0 if numerator == 0 else 0.0
    return numerator / denominator


class MetricAccumulator:
    def __init__(self) -> None:
        self.documents = 0
        self.documents_without_leaks = 0
        self.documents_with_false_positives = 0
        self.pii_bytes = 0
        self.predicted_bytes = 0
        self.true_positive_bytes = 0
        self.non_pii_bytes = 0
        self.false_positive_bytes = 0
        self.entities = 0
        self.entities_fully_covered = 0
        self.entities_overlapped = 0
        self.entities_exact = 0
        self.prediction_spans = 0
        self.prediction_spans_overlapped = 0

    def add(self, document: Document, predictions: Sequence[Span]) -> None:
        gold = merge_intervals((span.start, span.end) for span in document.spans)
        predicted = merge_intervals((span.start, span.end) for span in predictions)
        text_bytes = len(document.text.encode("utf-8"))
        gold_bytes = interval_length(gold)
        predicted_bytes = interval_length(predicted)
        true_positive_bytes = intersection_length(gold, predicted)
        false_positive_bytes = predicted_bytes - true_positive_bytes

        self.documents += 1
        self.documents_without_leaks += true_positive_bytes == gold_bytes
        self.documents_with_false_positives += false_positive_bytes > 0
        self.pii_bytes += gold_bytes
        self.predicted_bytes += predicted_bytes
        self.true_positive_bytes += true_positive_bytes
        self.non_pii_bytes += text_bytes - gold_bytes
        self.false_positive_bytes += false_positive_bytes
        self.entities += len(document.spans)
        self.entities_fully_covered += sum(
            interval_is_covered((span.start, span.end), predicted)
            for span in document.spans
        )
        self.entities_overlapped += sum(
            interval_overlaps((span.start, span.end), predicted)
            for span in document.spans
        )
        exact = {(span.start, span.end) for span in predictions}
        self.entities_exact += sum(
            (span.start, span.end) in exact for span in document.spans
        )
        self.prediction_spans += len(predictions)
        self.prediction_spans_overlapped += sum(
            interval_overlaps((span.start, span.end), gold) for span in predictions
        )

    def result(self) -> dict[str, object]:
        precision = safe_ratio(self.true_positive_bytes, self.predicted_bytes)
        recall = safe_ratio(self.true_positive_bytes, self.pii_bytes)
        f1 = safe_ratio(2 * precision * recall, precision + recall)
        f2 = safe_ratio(5 * precision * recall, 4 * precision + recall)
        return {
            "documents": self.documents,
            "zero_leak_document_rate": safe_ratio(
                self.documents_without_leaks, self.documents
            ),
            "documents_without_leaks": self.documents_without_leaks,
            "documents_with_false_positives": self.documents_with_false_positives,
            "utf8_bytes": {
                "pii": self.pii_bytes,
                "predicted": self.predicted_bytes,
                "true_positive": self.true_positive_bytes,
                "false_positive": self.false_positive_bytes,
                "leaked": self.pii_bytes - self.true_positive_bytes,
                "precision": precision,
                "recall": recall,
                "leak_rate": 1.0 - recall,
                "f1": f1,
                "f2": f2,
                "false_positive_rate": safe_ratio(
                    self.false_positive_bytes, self.non_pii_bytes
                ),
            },
            "entities": {
                "gold": self.entities,
                "fully_covered": self.entities_fully_covered,
                "overlapped": self.entities_overlapped,
                "exact_boundary": self.entities_exact,
                "full_coverage_recall": safe_ratio(
                    self.entities_fully_covered, self.entities
                ),
                "overlap_recall": safe_ratio(self.entities_overlapped, self.entities),
                "exact_boundary_recall": safe_ratio(self.entities_exact, self.entities),
            },
            "prediction_spans": {
                "total": self.prediction_spans,
                "overlapped_gold": self.prediction_spans_overlapped,
                "overlap_precision": safe_ratio(
                    self.prediction_spans_overlapped, self.prediction_spans
                ),
            },
        }


class RecallAccumulator:
    def __init__(self) -> None:
        self.bytes = 0
        self.covered_bytes = 0
        self.entities = 0
        self.fully_covered = 0
        self.overlapped = 0

    def add(self, spans: Sequence[Span], predictions: Sequence[Span]) -> None:
        predicted = merge_intervals((span.start, span.end) for span in predictions)
        for span in spans:
            interval = (span.start, span.end)
            span_bytes = span.end - span.start
            self.bytes += span_bytes
            self.covered_bytes += intersection_length([interval], predicted)
            self.entities += 1
            self.fully_covered += interval_is_covered(interval, predicted)
            self.overlapped += interval_overlaps(interval, predicted)

    def result(self) -> dict[str, object]:
        return {
            "entities": self.entities,
            "fully_covered": self.fully_covered,
            "overlapped": self.overlapped,
            "full_coverage_recall": safe_ratio(self.fully_covered, self.entities),
            "overlap_recall": safe_ratio(self.overlapped, self.entities),
            "utf8_bytes": self.bytes,
            "covered_utf8_bytes": self.covered_bytes,
            "byte_recall": safe_ratio(self.covered_bytes, self.bytes),
            "leaked_utf8_bytes": self.bytes - self.covered_bytes,
        }


class ContractAccumulator:
    INTEGRITY_ERROR_FIELDS = (
        "invalid_clean_bounds",
        "invalid_raw_bounds",
        "overlapping_clean_spans",
        "non_monotonic_raw_spans",
        "token_restore_failures",
        "raw_value_mismatches",
    )

    def __init__(self) -> None:
        self.documents = 0
        self.restore_exact_documents = 0
        self.restore_success_decisions = 0
        self.manifest_valid_documents = 0
        self.manifest_spans = 0
        self.integrity_errors: Counter[str] = Counter()
        self.initial_suspects = 0
        self.initial_actionable_suspects = 0
        self.initial_class_mismatches = 0
        self.strict_would_reject_documents = 0
        self.post_policy_scanned_documents = 0
        self.post_policy_zero_suspect_documents = 0
        self.post_policy_suspects = 0

    def add(self, response: dict[str, object]) -> None:
        self.documents += 1
        restore = response["restore"]
        integrity = response["manifest_integrity"]
        initial = response["initial_safety_net_stats"]
        assert isinstance(restore, dict)
        assert isinstance(integrity, dict)
        assert isinstance(initial, dict)

        self.restore_exact_documents += bool(restore["exact"])
        self.restore_success_decisions += restore["decision"] == "success"
        self.manifest_spans += int(integrity["spans"])
        document_integrity_errors = 0
        for field in self.INTEGRITY_ERROR_FIELDS:
            count = int(integrity[field])
            self.integrity_errors[field] += count
            document_integrity_errors += count
        self.manifest_valid_documents += document_integrity_errors == 0

        self.initial_suspects += int(initial["suspect_count"])
        self.initial_actionable_suspects += int(initial["uncovered_count"]) + int(
            initial["partial_bleed_count"]
        )
        self.initial_class_mismatches += int(initial["class_mismatch_count"])
        self.strict_would_reject_documents += bool(response["strict_would_reject"])

        post_policy = response["post_policy_safety_net_stats"]
        if post_policy is not None:
            assert isinstance(post_policy, dict)
            self.post_policy_scanned_documents += 1
            suspects = int(post_policy["suspect_count"])
            self.post_policy_suspects += suspects
            self.post_policy_zero_suspect_documents += suspects == 0

    def result(self) -> dict[str, object]:
        return {
            "documents": self.documents,
            "restore_exact_documents": self.restore_exact_documents,
            "restore_exact_rate": safe_ratio(
                self.restore_exact_documents, self.documents
            ),
            "restore_success_decisions": self.restore_success_decisions,
            "restore_success_decision_rate": safe_ratio(
                self.restore_success_decisions, self.documents
            ),
            "manifest_spans": self.manifest_spans,
            "manifest_valid_documents": self.manifest_valid_documents,
            "manifest_valid_document_rate": safe_ratio(
                self.manifest_valid_documents, self.documents
            ),
            "manifest_integrity_errors": dict(sorted(self.integrity_errors.items())),
            "initial_safety_net_suspects": self.initial_suspects,
            "initial_actionable_suspects": self.initial_actionable_suspects,
            "initial_class_mismatches": self.initial_class_mismatches,
            "strict_would_reject_documents": self.strict_would_reject_documents,
            "strict_acceptance_rate": 1.0
            - safe_ratio(self.strict_would_reject_documents, self.documents),
            "post_policy_scanned_documents": self.post_policy_scanned_documents,
            "post_policy_suspects": self.post_policy_suspects,
            "post_policy_zero_suspect_documents": self.post_policy_zero_suspect_documents,
            "post_policy_zero_suspect_rate": (
                safe_ratio(
                    self.post_policy_zero_suspect_documents,
                    self.post_policy_scanned_documents,
                )
                if self.post_policy_scanned_documents
                else None
            ),
        }


def percentile(values: Sequence[float], quantile: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    position = (len(ordered) - 1) * quantile
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def timing_summary(values: Sequence[float]) -> dict[str, float]:
    return {
        "mean": statistics.fmean(values) if values else 0.0,
        "median": statistics.median(values) if values else 0.0,
        "p95": percentile(values, 0.95),
        "p99": percentile(values, 0.99),
        "max": max(values, default=0.0),
    }


def validate_prediction(document: Document, raw: dict[str, object]) -> Span:
    start = raw["raw_start"]
    end = raw["raw_end"]
    label = raw["class"]
    if not isinstance(start, int) or not isinstance(end, int) or not isinstance(label, str):
        raise RuntimeError(f"{document.uid}: invalid prediction shape")
    text_bytes = len(document.text.encode("utf-8"))
    if start < 0 or end <= start or end > text_bytes:
        raise RuntimeError(f"{document.uid}: invalid prediction bounds {start}:{end}")
    return Span(start, end, label)


def map_clean_actions_to_raw(
    document: Document,
    clean_length: int,
    manifest: Sequence[dict[str, object]],
    actions: Sequence[dict[str, object]],
) -> list[Span]:
    raw_length = len(document.text.encode("utf-8"))
    ordered_manifest = sorted(manifest, key=lambda span: int(span["clean_start"]))
    segments: list[tuple[int, int, int, int, bool]] = []
    clean_cursor = 0
    raw_cursor = 0
    for span in ordered_manifest:
        clean_start = int(span["clean_start"])
        clean_end = int(span["clean_end"])
        raw_start = int(span["raw_start"])
        raw_end = int(span["raw_end"])
        if clean_start < clean_cursor or raw_start < raw_cursor:
            raise RuntimeError(f"{document.uid}: non-monotonic pre-safety manifest")
        if clean_start - clean_cursor != raw_start - raw_cursor:
            raise RuntimeError(f"{document.uid}: pre-safety plain-text mapping drift")
        if clean_start > clean_cursor:
            segments.append((clean_cursor, clean_start, raw_cursor, raw_start, False))
        segments.append((clean_start, clean_end, raw_start, raw_end, True))
        clean_cursor = clean_end
        raw_cursor = raw_end
    if clean_length - clean_cursor != raw_length - raw_cursor:
        raise RuntimeError(f"{document.uid}: pre-safety trailing mapping drift")
    if clean_cursor < clean_length:
        segments.append((clean_cursor, clean_length, raw_cursor, raw_length, False))

    mapped: list[Span] = []
    for action in actions:
        action_start = int(action["action_start"])
        action_end = int(action["action_end"])
        label = str(action["class"])
        if action_start < 0 or action_end <= action_start or action_end > clean_length:
            raise RuntimeError(f"{document.uid}: invalid SafetyNet action span")
        for clean_start, clean_end, raw_start, raw_end, is_token in segments:
            overlap_start = max(action_start, clean_start)
            overlap_end = min(action_end, clean_end)
            if overlap_start >= overlap_end:
                continue
            if is_token:
                mapped.append(Span(raw_start, raw_end, label))
            else:
                mapped.append(
                    Span(
                        raw_start + overlap_start - clean_start,
                        raw_start + overlap_end - clean_start,
                        label,
                    )
                )
    return mapped


def effective_predictions(
    document: Document, response: dict[str, object]
) -> list[Span]:
    pre_safety_manifest = response["pre_safety_manifest_spans"]
    if pre_safety_manifest is None:
        return [
            validate_prediction(document, span) for span in response["manifest_spans"]
        ]
    assert isinstance(pre_safety_manifest, list)
    predictions = [
        validate_prediction(document, span) for span in pre_safety_manifest
    ]
    if response["safety_net_mode"] != "resolve":
        return predictions

    suspects = response["leak_suspects"]
    assert isinstance(suspects, list)
    has_class_mismatch = any(suspect["kind"] == "class_mismatch" for suspect in suspects)
    actions = [
        suspect
        for suspect in suspects
        if has_class_mismatch
        or suspect["kind"] in {"uncovered", "partial_bleed"}
    ]
    clean_length = response["pre_safety_text_len"]
    if not isinstance(clean_length, int):
        raise RuntimeError(f"{document.uid}: missing pre-safety text length")
    predictions.extend(
        map_clean_actions_to_raw(
            document, clean_length, pre_safety_manifest, actions
        )
    )
    return predictions


def run_config(
    repo_root: Path,
    binary: Path,
    config: str,
    documents: Sequence[Document],
    model_dir: Path,
    kiji_model_dir: Path,
    opf_command: Path | None,
    opf_checkpoint: Path | None,
    opf_daemon_socket: Path | None,
    threshold: float,
    diagnostics_dir: Path,
) -> dict[str, object]:
    environment = os.environ.copy()
    environment["GAZE_NER_MODEL_DIR"] = str(model_dir)
    environment["GAZE_NER_THRESHOLD"] = str(threshold)
    environment["GAZE_KIJI_DISTILBERT_MODEL_DIR"] = str(kiji_model_dir)
    if opf_command is not None:
        environment["GAZE_OPENAI_FILTER_OPF"] = str(opf_command)
    if opf_checkpoint is not None:
        environment["OPF_CHECKPOINT"] = str(opf_checkpoint)
    if opf_daemon_socket is not None:
        environment["GAZE_OPF_DAEMON_SOCKET"] = str(opf_daemon_socket)
    command = [str(binary), "--config", config]
    diagnostics_dir.mkdir(parents=True, exist_ok=True)
    stderr_path = diagnostics_dir / f"{config}.stderr.log"
    stderr_handle = stderr_path.open("w", encoding="utf-8")
    started = time.perf_counter()
    process = subprocess.Popen(
        command,
        cwd=repo_root,
        env=environment,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=stderr_handle,
        text=True,
        encoding="utf-8",
        bufsize=1,
    )
    assert process.stdin is not None
    assert process.stdout is not None

    overall = MetricAccumulator()
    per_language: defaultdict[str, MetricAccumulator] = defaultdict(MetricAccumulator)
    per_label: defaultdict[str, RecallAccumulator] = defaultdict(RecallAccumulator)
    direct = RecallAccumulator()
    contextual = RecallAccumulator()
    contract = ContractAccumulator()
    pipeline_errors: Counter[str] = Counter()
    pipeline_error_stages: Counter[str] = Counter()
    timing: defaultdict[str, list[float]] = defaultdict(list)
    first_response_ms: float | None = None

    try:
        for index, document in enumerate(documents):
            request = {
                "fixture_id": document.uid,
                "locale_chain": document.locale_chain,
                "text": document.text,
            }
            process.stdin.write(json.dumps(request, ensure_ascii=False) + "\n")
            process.stdin.flush()
            response_line = process.stdout.readline()
            if not response_line:
                return_code = process.poll()
                raise RuntimeError(
                    f"benchmark runner exited before {document.uid}; status={return_code}"
                )
            if first_response_ms is None:
                first_response_ms = (time.perf_counter() - started) * 1000.0
            response = validate_response(document, json.loads(response_line))
            if "pipeline_error_code" in response:
                pipeline_errors[str(response["pipeline_error_code"])] += 1
                pipeline_error_stages[str(response["pipeline_error_stage"])] += 1
                for key, value in response["timing"].items():
                    if value is not None:
                        timing[key].append(float(value))
                continue
            predictions = effective_predictions(document, response)
            overall.add(document, predictions)
            per_language[document.language].add(document, predictions)
            direct_spans = [
                span for span in document.spans if span.label in DIRECT_IDENTIFIER_LABELS
            ]
            contextual_spans = [
                span for span in document.spans if span.label not in DIRECT_IDENTIFIER_LABELS
            ]
            direct.add(direct_spans, predictions)
            contextual.add(contextual_spans, predictions)
            contract.add(response)
            for label in {span.label for span in document.spans}:
                per_label[label].add(
                    [span for span in document.spans if span.label == label], predictions
                )
            for key, value in response["timing"].items():
                if value is not None:
                    timing[key].append(float(value))
            if (index + 1) % 500 == 0:
                print(
                    f"{config}: scored {index + 1}/{len(documents)} documents",
                    file=sys.stderr,
                )
    finally:
        process.stdin.close()
        return_code = process.wait(timeout=30)
        stderr_handle.close()
        if return_code != 0:
            raise RuntimeError(f"benchmark runner failed with status {return_code}")

    wall_seconds = time.perf_counter() - started
    return {
        "config": config,
        "metrics": overall.result(),
        "direct_identifier_recall": direct.result(),
        "contextual_pii_recall": contextual.result(),
        "pipeline_contract": contract.result(),
        "pipeline_availability": {
            "attempted_documents": len(documents),
            "completed_documents": len(documents) - sum(pipeline_errors.values()),
            "completion_rate": safe_ratio(
                len(documents) - sum(pipeline_errors.values()), len(documents)
            ),
            "failed_closed_documents": sum(pipeline_errors.values()),
            "errors": dict(sorted(pipeline_errors.items())),
            "error_stages": dict(sorted(pipeline_error_stages.items())),
        },
        "per_language": {
            key: value.result() for key, value in sorted(per_language.items())
        },
        "per_label_recall": {
            key: value.result() for key, value in sorted(per_label.items())
        },
        "latency_ms": {
            key: timing_summary(value) for key, value in sorted(timing.items())
        },
        "warm_latency_ms": {
            key: timing_summary(value[1:])
            for key, value in sorted(timing.items())
            if len(value) > 1
        },
        "process": {
            "wall_seconds": wall_seconds,
            "documents_per_second": len(documents) / wall_seconds,
            "start_to_first_response_ms": first_response_ms,
            "stderr_log": str(stderr_path.relative_to(repo_root)),
            "stderr_bytes": stderr_path.stat().st_size,
        },
    }


def git_metadata(repo_root: Path) -> dict[str, object]:
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo_root,
        check=True,
        text=True,
        capture_output=True,
    ).stdout.strip()
    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=repo_root,
        check=True,
        text=True,
        capture_output=True,
    ).stdout
    return {"revision": revision, "dirty": bool(status.strip())}


SCORING_METADATA: dict[str, object] = {
    "primary_unit": "UTF-8 bytes",
    "primary_scope": "label-agnostic whole-pipeline PII pseudonymization",
    "adjacent_and_overlapping_spans_are_merged": True,
    "full_entity_coverage_requires_every_byte": True,
    "direct_identifier_labels": sorted(DIRECT_IDENTIFIER_LABELS),
    "non_compensating_axes": [
        "safety",
        "reversibility",
        "manifest_integrity",
        "strict_availability",
        "precision",
        "latency",
    ],
    "notes": [
        "All labeled PII bytes are included in the primary safety score.",
        "A protected byte counts for safety even when its action is not reversible.",
        "Restore, manifest, pipeline, and telemetry failures remain separate hard failures.",
        "Final protection evidence contains offsets, classes, counts, and stable IDs only; never PII values.",
        "Latency is considered only after correctness gates pass.",
    ],
    "comparison_gates": {
        "pipeline_completion_rate": "must equal 1.0 for release readiness; no regression",
        "leaked_labeled_utf8_bytes": "must equal 0 for release readiness; no regression",
        "full_entity_coverage_recall": "must equal 1.0 for release readiness; no regression",
        "zero_leak_document_rate": "must equal 1.0 for release readiness; no regression",
        "restore_exact_rate": "must equal 1.0 for release readiness; no regression",
        "manifest_valid_document_rate": "must equal 1.0 for release readiness; no regression",
        "strict_rejections": "must equal 0 for release readiness; no regression",
        "byte_precision": "no regression without explicit review",
        "p95_latency_ms": "lower only after every correctness gate passes",
    },
}


def assemble_scorecard(
    *,
    repo_root: Path,
    dataset_metadata: Mapping[str, object],
    dataset_report: Mapping[str, object],
    sampling_report: Mapping[str, object],
    parameters: Mapping[str, object],
    runs: Sequence[Mapping[str, object]],
) -> dict[str, object]:
    available_population = copy.deepcopy(sampling_report["available_population"])
    evaluated_population = copy.deepcopy(sampling_report["evaluated_population"])
    dataset = copy.deepcopy(dict(dataset_metadata))
    dataset.update(
        {
            key: copy.deepcopy(value)
            for key, value in dataset_report.items()
            if key != "selection"
        }
    )
    dataset.update(
        {
            "available_population": available_population,
            "evaluated_population": evaluated_population,
            "selection": copy.deepcopy(evaluated_population),
            "sampling": {
                key: copy.deepcopy(value)
                for key, value in sampling_report.items()
                if key not in {"available_population", "evaluated_population"}
            },
        }
    )
    return {
        "schema_version": SCORECARD_SCHEMA_VERSION,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "gaze": git_metadata(repo_root),
        "dataset": dataset,
        "scoring": copy.deepcopy(SCORING_METADATA),
        "parameters": copy.deepcopy(dict(parameters)),
        "runs": [copy.deepcopy(dict(run)) for run in runs],
    }


def _scorecard_runs(scorecard: Mapping[str, object]) -> dict[str, Mapping[str, object]]:
    if type(scorecard.get("schema_version")) is not int:
        raise ValueError("scorecard schema_version must be integer 3")
    if scorecard["schema_version"] != SCORECARD_SCHEMA_VERSION:
        raise ValueError(
            f"unsupported scorecard schema_version {scorecard['schema_version']}"
        )
    raw_runs = scorecard.get("runs")
    if not isinstance(raw_runs, list):
        raise ValueError("scorecard runs must be an array")
    result: dict[str, Mapping[str, object]] = {}
    for raw_run in raw_runs:
        if not isinstance(raw_run, dict) or not isinstance(raw_run.get("config"), str):
            raise ValueError("scorecard run must be an object with a string config")
        config = raw_run["config"]
        if config in result:
            raise ValueError(f"duplicate scorecard config {config}")
        result[config] = raw_run
    return result


def _run_correctness_values(run: Mapping[str, object]) -> dict[str, int | float]:
    metrics = run["metrics"]
    contract = run["pipeline_contract"]
    availability = run["pipeline_availability"]
    if not all(isinstance(value, dict) for value in (metrics, contract, availability)):
        raise ValueError("scorecard run correctness sections must be objects")
    utf8_bytes = metrics["utf8_bytes"]
    entities = metrics["entities"]
    if not isinstance(utf8_bytes, dict) or not isinstance(entities, dict):
        raise ValueError("scorecard metric sections must be objects")
    return {
        "pipeline_completion_rate": float(availability["completion_rate"]),
        "leaked_labeled_utf8_bytes": int(utf8_bytes["leaked"]),
        "pii_byte_recall": float(utf8_bytes["recall"]),
        "full_entity_coverage_recall": float(entities["full_coverage_recall"]),
        "zero_leak_document_rate": float(metrics["zero_leak_document_rate"]),
        "restore_exact_rate": float(contract["restore_exact_rate"]),
        "manifest_valid_document_rate": float(
            contract["manifest_valid_document_rate"]
        ),
        "strict_rejections": int(contract["strict_would_reject_documents"]),
        "byte_precision": float(utf8_bytes["precision"]),
    }


def compare_scorecards(
    candidate: Mapping[str, object], baseline: Mapping[str, object]
) -> dict[str, object]:
    candidate_runs = _scorecard_runs(candidate)
    baseline_runs = _scorecard_runs(baseline)
    regression_failures: list[dict[str, object]] = []
    readiness_failures: list[dict[str, object]] = []
    higher_is_better = (
        "pipeline_completion_rate",
        "pii_byte_recall",
        "full_entity_coverage_recall",
        "zero_leak_document_rate",
        "restore_exact_rate",
        "manifest_valid_document_rate",
        "byte_precision",
    )
    lower_is_better = ("leaked_labeled_utf8_bytes", "strict_rejections")
    readiness_targets: dict[str, int | float] = {
        "pipeline_completion_rate": 1.0,
        "leaked_labeled_utf8_bytes": 0,
        "pii_byte_recall": 1.0,
        "full_entity_coverage_recall": 1.0,
        "zero_leak_document_rate": 1.0,
        "restore_exact_rate": 1.0,
        "manifest_valid_document_rate": 1.0,
        "strict_rejections": 0,
    }

    for config, run in sorted(candidate_runs.items()):
        candidate_values = _run_correctness_values(run)
        baseline_run = baseline_runs.get(config)
        if baseline_run is None:
            regression_failures.append(
                {"config": config, "gate": "baseline_config", "reason": "missing"}
            )
        else:
            baseline_values = _run_correctness_values(baseline_run)
            for gate in higher_is_better:
                if candidate_values[gate] < baseline_values[gate]:
                    regression_failures.append(
                        {
                            "config": config,
                            "gate": gate,
                            "candidate": candidate_values[gate],
                            "baseline": baseline_values[gate],
                        }
                    )
            for gate in lower_is_better:
                if candidate_values[gate] > baseline_values[gate]:
                    regression_failures.append(
                        {
                            "config": config,
                            "gate": gate,
                            "candidate": candidate_values[gate],
                            "baseline": baseline_values[gate],
                        }
                    )
        for gate, target in readiness_targets.items():
            if candidate_values[gate] != target:
                readiness_failures.append(
                    {
                        "config": config,
                        "gate": gate,
                        "actual": candidate_values[gate],
                        "required": target,
                    }
                )

    return {
        "schema_version": SCORECARD_SCHEMA_VERSION,
        "regression": {
            "passed": not regression_failures,
            "failures": regression_failures,
        },
        "release_readiness": {
            "passed": not readiness_failures,
            "failures": readiness_failures,
        },
    }
