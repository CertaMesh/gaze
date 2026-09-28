"""Additional comparison metrics, shared by Gaze and every external adapter."""

from __future__ import annotations

import hashlib
from dataclasses import dataclass, field
from typing import Mapping, Sequence

import gaze_bench_score as score


def split_for_id(uid: str) -> str:
    """A stable, corpus-independent validation/test assignment."""
    return "validation" if hashlib.sha256(uid.encode()).digest()[0] < 128 else "test"


def _ratio(numerator: int, denominator: int) -> float:
    return numerator / denominator if denominator else 0.0


@dataclass
class ComparisonMetrics:
    mapping: Mapping[str, Sequence[str]]
    included_labels: frozenset[str] | None = None
    typed_mapping: Mapping[str, Sequence[str]] | None = None
    documents: int = 0
    pii_documents: int = 0
    leaking_documents: int = 0
    gold_entities: int = 0
    leaking_entities: int = 0
    total_bytes: int = 0
    predicted_bytes: int = 0
    true_positive_bytes: int = 0
    false_positive_bytes: int = 0
    leaked_bytes: int = 0
    typed_tp: int = 0
    typed_fp: int = 0
    typed_fn: int = 0

    def add(self, document: score.Document, predictions: Sequence[score.Span]) -> None:
        if self.included_labels is not None:
            selected = tuple(s for s in document.spans if s.label in self.included_labels)
            omitted = tuple(s for s in document.spans if s.label not in self.included_labels)
            from dataclasses import replace
            document = replace(
                document, spans=selected,
                excluded_spans=document.excluded_spans + omitted,
            )
            predictions = [
                s for s in predictions
                if self.included_labels.intersection(self.mapping.get(s.label, ()))
            ]
        unknown = {s.label for s in predictions} - self.mapping.keys()
        if unknown:
            raise ValueError(f"unmapped comparison labels: {sorted(unknown)}")
        gold, ignored, retained = score.contract_scoring_view(document, predictions)
        predicted = score.subtract_intervals(
            score.merge_intervals((s.start, s.end) for s in retained), ignored
        )
        gold_bytes = score.interval_length(gold)
        predicted_bytes = score.interval_length(predicted)
        tp_bytes = score.intersection_length(gold, predicted)
        self.documents += 1
        self.pii_documents += gold_bytes > 0
        self.leaking_documents += gold_bytes > tp_bytes
        self.gold_entities += len(document.spans)
        self.leaking_entities += sum(
            not score.interval_is_covered((s.start, s.end), predicted)
            for s in document.spans
        )
        self.total_bytes += len(document.text.encode("utf-8")) - score.interval_length(ignored)
        self.predicted_bytes += predicted_bytes
        self.true_positive_bytes += tp_bytes
        self.false_positive_bytes += predicted_bytes - tp_bytes
        self.leaked_bytes += gold_bytes - tp_bytes

        unmatched = list(document.spans)
        for prediction in retained:
            labels = (self.mapping if self.typed_mapping is None else self.typed_mapping)[prediction.label]
            match = next((i for i, gold_span in enumerate(unmatched)
                          if prediction.start == gold_span.start
                          and prediction.end == gold_span.end
                          and gold_span.label in labels), None)
            if match is None:
                self.typed_fp += 1
            else:
                self.typed_tp += 1
                unmatched.pop(match)
        self.typed_fn += len(unmatched)

    def result(self) -> dict[str, object]:
        precision = _ratio(self.typed_tp, self.typed_tp + self.typed_fp)
        recall = _ratio(self.typed_tp, self.typed_tp + self.typed_fn)
        return {
            "documents": self.documents,
            "pii_documents": self.pii_documents,
            "leaking_documents": self.leaking_documents,
            "document_leak_rate": _ratio(self.leaking_documents, self.pii_documents),
            "gold_entities": self.gold_entities,
            "leaking_entities": self.leaking_entities,
            "leaked_entity_rate": _ratio(self.leaking_entities, self.gold_entities),
            "total_bytes": self.total_bytes,
            "predicted_bytes": self.predicted_bytes,
            "redaction_load": _ratio(self.predicted_bytes, self.total_bytes),
            "true_positive_bytes": self.true_positive_bytes,
            "false_positive_bytes": self.false_positive_bytes,
            "leaked_bytes": self.leaked_bytes,
            "typed_entities": {
                "tp": self.typed_tp, "fp": self.typed_fp, "fn": self.typed_fn,
                "precision": precision, "recall": recall,
                "f1": _ratio(2 * precision * recall, precision + recall),
                "f2": _ratio(5 * precision * recall, 4 * precision + recall),
            },
        }
