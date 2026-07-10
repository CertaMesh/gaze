#!/usr/bin/env python3
"""Benchmark the current Gaze masking pipeline on a pinned synthetic corpus.

The benchmark is deliberately label-agnostic at its primary boundary: a PII
byte is safe only when Gaze replaces it. Entity and per-label recall remain as
diagnostics, but conventional NER F1 is not allowed to hide partial leaks.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Sequence


DATASET_REPO = "ai4privacy/pii-masking-micro-100k"
DATASET_REVISION = "3cd59c65631280839f830d3ba96dcdfe1785cab1"
DATASET_FILE = "data/validation.jsonl"
DATASET_URL = (
    f"https://huggingface.co/datasets/{DATASET_REPO}/resolve/"
    f"{DATASET_REVISION}/{DATASET_FILE}"
)
DATASET_SHA256 = "bb15da1b5fbb11b3cc6fd4c95eca256197573ecd066230eb3c1fe6898f27a578"
DATASET_ROWS = 9_990
DATASET_BYTES = 32_536_978

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


def fetch_dataset(path: Path) -> None:
    if path.exists():
        verify_dataset_file(path)
        return

    path.parent.mkdir(parents=True, exist_ok=True)
    print(f"fetching pinned OpenPII validation split to {path}", file=sys.stderr)
    request = urllib.request.Request(
        DATASET_URL,
        headers={"User-Agent": "gaze-openpii-benchmark/1"},
    )
    digest = hashlib.sha256()
    byte_count = 0
    with urllib.request.urlopen(request, timeout=120) as response:
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as output:
            temporary = Path(output.name)
            try:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    output.write(chunk)
                    digest.update(chunk)
                    byte_count += len(chunk)
            except BaseException:
                temporary.unlink(missing_ok=True)
                raise

    if digest.hexdigest() != DATASET_SHA256 or byte_count != DATASET_BYTES:
        temporary.unlink(missing_ok=True)
        raise RuntimeError(
            "downloaded dataset failed its pinned SHA-256 or byte-size check"
        )
    os.replace(temporary, path)


def verify_dataset_file(path: Path) -> None:
    size = path.stat().st_size
    digest = sha256_file(path)
    if size != DATASET_BYTES or digest != DATASET_SHA256:
        raise RuntimeError(
            f"dataset integrity mismatch for {path}: size={size}, sha256={digest}"
        )


def char_to_byte_offsets(text: str) -> list[int]:
    offsets = [0]
    byte_offset = 0
    for character in text:
        byte_offset += len(character.encode("utf-8"))
        offsets.append(byte_offset)
    return offsets


def load_dataset(
    path: Path,
    languages: frozenset[str] | None,
    max_documents: int | None,
) -> tuple[list[Document], dict[str, object]]:
    verify_dataset_file(path)
    documents: list[Document] = []
    seen_uids: set[str] = set()
    label_counts: Counter[str] = Counter()
    language_counts: Counter[str] = Counter()
    region_counts: Counter[str] = Counter()
    source_counts: Counter[str] = Counter()
    total_rows = 0
    total_spans = 0

    with path.open(encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            total_rows += 1
            row = json.loads(line)
            if row.get("split") != "validation":
                raise ValueError(f"line {line_number}: expected validation split")
            uid = str(row["uid"])
            if uid in seen_uids:
                raise ValueError(f"line {line_number}: duplicate uid {uid}")
            seen_uids.add(uid)

            text = row["source_text"]
            offsets = char_to_byte_offsets(text)
            spans: list[Span] = []
            for entity in row["privacy_mask"]:
                start = entity["start"]
                end = entity["end"]
                if not isinstance(start, int) or not isinstance(end, int):
                    raise ValueError(f"line {line_number}: non-integer entity offset")
                if start < 0 or end <= start or end > len(text):
                    raise ValueError(f"line {line_number}: invalid entity bounds")
                if text[start:end] != entity["value"]:
                    raise ValueError(f"line {line_number}: entity value/offset mismatch")
                label = entity["label"]
                spans.append(Span(offsets[start], offsets[end], label))
                label_counts[label] += 1
                total_spans += 1

            language = row["language"]
            region = row["region"]
            language_counts[language] += 1
            region_counts[f"{language}-{region}"] += 1
            source_counts[row["source_dataset"]] += 1
            if languages is not None and language not in languages:
                continue
            if max_documents is not None and len(documents) >= max_documents:
                continue
            documents.append(
                Document(
                    uid=uid,
                    text=text,
                    language=language,
                    region=region,
                    source_dataset=row["source_dataset"],
                    spans=tuple(spans),
                )
            )

    if total_rows != DATASET_ROWS:
        raise ValueError(f"expected {DATASET_ROWS} rows, found {total_rows}")
    if len(seen_uids) != DATASET_ROWS:
        raise ValueError("dataset UID cardinality mismatch")
    if not documents:
        raise ValueError("dataset filters selected zero documents")

    selected_labels = Counter(
        span.label for document in documents for span in document.spans
    )
    selected_languages = Counter(document.language for document in documents)
    selected_regions = Counter(
        f"{document.language}-{document.region}" for document in documents
    )
    selected_sources = Counter(document.source_dataset for document in documents)
    report: dict[str, object] = {
        "integrity": {
            "rows": total_rows,
            "unique_uids": len(seen_uids),
            "entities": total_spans,
            "sha256": DATASET_SHA256,
            "bytes": DATASET_BYTES,
            "label_counts": dict(sorted(label_counts.items())),
            "language_counts": dict(sorted(language_counts.items())),
            "region_counts": dict(sorted(region_counts.items())),
            "source_counts": dict(sorted(source_counts.items())),
        },
        "selection": {
            "documents": len(documents),
            "entities": sum(selected_labels.values()),
            "labels": dict(sorted(selected_labels.items())),
            "languages": dict(sorted(selected_languages.items())),
            "regions": dict(sorted(selected_regions.items())),
            "sources": dict(sorted(selected_sources.items())),
            "negative_only_documents": sum(not document.spans for document in documents),
        },
    }
    return documents, report


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
            response = json.loads(response_line)
            if response["fixture_id"] != document.uid:
                raise RuntimeError(
                    f"runner response mismatch: expected {document.uid}, "
                    f"received {response['fixture_id']}"
                )
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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--dataset",
        type=Path,
        default=Path("target/bench-data/openpii-micro/validation.jsonl"),
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("target/bench-data/openpii-micro/gaze-benchmark.json"),
    )
    parser.add_argument(
        "--model-dir",
        type=Path,
        default=Path(
            os.environ.get(
                "GAZE_NER_MODEL_DIR",
                "~/.local/share/gaze/models/davlan-mbert-ner-hrl",
            )
        ).expanduser(),
    )
    parser.add_argument("--threshold", type=float, default=0.3)
    parser.add_argument(
        "--kiji-model-dir",
        type=Path,
        default=Path(
            os.environ.get(
                "GAZE_KIJI_DISTILBERT_MODEL_DIR",
                "~/.local/share/gaze/models/kiji-distilbert",
            )
        ).expanduser(),
    )
    parser.add_argument(
        "--config",
        action="append",
        choices=(
            "rule-floor-core",
            "rule-floor-extended",
            "pass2-ner",
            "full-stack-kiji-resolve",
            "full-stack-opf-resolve",
            "pass3-kiji",
            "pass3-opf",
            "pass3-locale-aware",
        ),
        help="repeat to benchmark more than one pipeline config",
    )
    parser.add_argument(
        "--language",
        action="append",
        help="ISO language code; repeat to select multiple languages",
    )
    parser.add_argument("--max-documents", type=int)
    parser.add_argument(
        "--opf-command",
        type=Path,
        default=(
            Path(os.environ["GAZE_OPENAI_FILTER_OPF"])
            if os.environ.get("GAZE_OPENAI_FILTER_OPF")
            else None
        ),
    )
    parser.add_argument(
        "--opf-checkpoint",
        type=Path,
        default=(
            Path(os.environ["OPF_CHECKPOINT"])
            if os.environ.get("OPF_CHECKPOINT")
            else None
        ),
    )
    parser.add_argument(
        "--opf-daemon-socket",
        type=Path,
        default=(
            Path(os.environ["GAZE_OPF_DAEMON_SOCKET"])
            if os.environ.get("GAZE_OPF_DAEMON_SOCKET")
            else None
        ),
    )
    parser.add_argument("--no-download", action="store_true")
    parser.add_argument("--fetch-only", action="store_true")
    parser.add_argument("--skip-build", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = Path(__file__).resolve().parents[2]
    dataset_path = args.dataset if args.dataset.is_absolute() else repo_root / args.dataset
    output_path = args.output if args.output.is_absolute() else repo_root / args.output
    if args.no_download:
        verify_dataset_file(dataset_path)
    else:
        fetch_dataset(dataset_path)
    if args.fetch_only:
        print(
            json.dumps(
                {
                    "path": str(dataset_path),
                    "sha256": DATASET_SHA256,
                    "rows": DATASET_ROWS,
                    "bytes": DATASET_BYTES,
                },
                indent=2,
            )
        )
        return 0

    languages = frozenset(args.language) if args.language else None
    documents, dataset_report = load_dataset(
        dataset_path, languages=languages, max_documents=args.max_documents
    )
    configs = tuple(args.config) if args.config else DEFAULT_CONFIGS
    if any(
        config
        in {"pass2-ner", "full-stack-kiji-resolve", "full-stack-opf-resolve"}
        for config in configs
    ) and not args.model_dir.is_dir():
        raise FileNotFoundError(f"NER model directory does not exist: {args.model_dir}")
    if any("kiji" in config for config in configs) and not args.kiji_model_dir.is_dir():
        raise FileNotFoundError(
            f"Kiji model directory does not exist: {args.kiji_model_dir}"
        )
    if any("opf" in config for config in configs):
        if args.opf_command is None or not args.opf_command.is_file():
            raise FileNotFoundError("OPF command does not exist; pass --opf-command")
        if args.opf_checkpoint is None or not args.opf_checkpoint.is_dir():
            raise FileNotFoundError(
                "OPF checkpoint does not exist; pass --opf-checkpoint"
            )

    binary = repo_root / "target/debug/examples/clean_for_bench"
    if not args.skip_build:
        build_command = [
            "cargo",
            "build",
            "-q",
            "-p",
            "gaze-recognizers",
            "--example",
            "clean_for_bench",
        ]
        features = []
        if any("kiji" in config for config in configs):
            features.append("safety-net-kiji")
        if any("opf" in config for config in configs):
            features.append("safety-net-openai")
        if features:
            build_command.extend(["--features", ",".join(features)])
        subprocess.run(
            build_command,
            cwd=repo_root,
            check=True,
        )
    if not binary.is_file():
        raise FileNotFoundError(f"benchmark runner is missing: {binary}")

    runs = []
    for config in configs:
        print(f"running {config} on {len(documents)} documents", file=sys.stderr)
        runs.append(
            run_config(
                repo_root,
                binary,
                config,
                documents,
                args.model_dir,
                args.kiji_model_dir,
                args.opf_command,
                args.opf_checkpoint,
                args.opf_daemon_socket,
                args.threshold,
                output_path.parent,
            )
        )

    result = {
        "schema_version": 2,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "gaze": git_metadata(repo_root),
        "dataset": {
            "repository": DATASET_REPO,
            "revision": DATASET_REVISION,
            "file": DATASET_FILE,
            "url": DATASET_URL,
            "license": "CC-BY-4.0",
            "synthetic_only": True,
            "reserved_from_training": True,
            **dataset_report,
        },
        "scoring": {
            "primary_unit": "UTF-8 bytes",
            "primary_scope": "label-agnostic PII masking",
            "adjacent_and_overlapping_spans_are_merged": True,
            "full_entity_coverage_requires_every_byte": True,
            "direct_identifier_labels": sorted(DIRECT_IDENTIFIER_LABELS),
            "notes": [
                "All dataset labels are included in the primary masking score.",
                "Direct-identifier and contextual-PII recall are diagnostics.",
                "The source validation rows all contain PII; precision and FPR use non-PII bytes inside those rows.",
                "The scorecard is non-compensating: safety, reversibility, trust, availability, and latency are reported separately.",
                "Resolve coverage maps SafetyNet action spans from pre-safety clean text back to raw byte ranges.",
            ],
            "comparison_gates": {
                "pipeline_completion_rate": "must equal 1.0",
                "pii_byte_recall": "higher; no regression allowed",
                "full_entity_coverage_recall": "higher; no regression allowed",
                "zero_leak_document_rate": "higher; no regression allowed",
                "restore_exact_rate": "must equal 1.0",
                "manifest_valid_document_rate": "must equal 1.0",
                "byte_precision": "no regression without explicit review",
                "p95_latency_ms": "lower after correctness gates pass",
            },
        },
        "parameters": {
            "configs": list(configs),
            "languages": sorted(languages) if languages else "all",
            "max_documents": args.max_documents,
            "ner_model_dir": str(args.model_dir),
            "kiji_model_dir": str(args.kiji_model_dir),
            "opf_command": str(args.opf_command) if args.opf_command else None,
            "opf_checkpoint": str(args.opf_checkpoint) if args.opf_checkpoint else None,
            "opf_daemon_socket": (
                str(args.opf_daemon_socket) if args.opf_daemon_socket else None
            ),
            "ner_threshold": args.threshold,
        },
        "runs": runs,
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {output_path}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
