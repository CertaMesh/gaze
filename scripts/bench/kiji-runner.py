#!/usr/bin/env python3
"""Reference Kiji DistilBERT subprocess wrapper for Gaze SafetyNet."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Iterable

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer


MAX_INPUT_BYTES = 1024 * 1024
PINNED_LABELS = {
    0: "O",
    1: "B-PER",
    2: "I-PER",
    3: "B-ORG",
    4: "I-ORG",
    5: "B-LOC",
    6: "I-LOC",
    7: "B-MISC",
    8: "I-MISC",
}
PINNED_VOCABULARY = {
    "person": frozenset({"B-PER", "I-PER"}),
    "organization": frozenset({"B-ORG", "I-ORG"}),
    "location": frozenset({"B-LOC", "I-LOC"}),
    "miscellaneous": frozenset({"B-MISC", "I-MISC"}),
}
PINNED_LABELS_MANIFEST_KEYS = frozenset({"schema_version", "source", "source_commit", "labels"})
PINNED_LABELS_SOURCE = "onnx-community/distilbert-NER-ONNX"
PINNED_LABELS_SOURCE_COMMIT = "3a19fe9404a4469d91aa3d551558a97f68872f67"


class KijiRunnerError(RuntimeError):
    pass


def reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise KijiRunnerError("invalid labels metadata: duplicate key")
        result[key] = value
    return result


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run pinned Kiji DistilBERT ONNX NER and emit Gaze JSON spans."
    )
    parser.add_argument("--format", required=True, choices=["json"])
    parser.add_argument("--output-mode", required=True, choices=["typed"])
    parser.add_argument("--model-dir", required=True)
    parser.add_argument("--precision", choices=["fp32", "int8"], default="fp32")
    return parser.parse_args(argv)


def read_stdin() -> str:
    raw = sys.stdin.buffer.read(MAX_INPUT_BYTES + 1)
    if len(raw) > MAX_INPUT_BYTES:
        raise KijiRunnerError(f"stdin exceeds {MAX_INPUT_BYTES} byte cap")
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise KijiRunnerError("stdin is not valid UTF-8") from exc


def load_labels(path: Path) -> dict[int, str]:
    try:
        with path.open("r", encoding="utf-8") as handle:
            data = json.load(handle, object_pairs_hook=reject_duplicate_keys)
    except KijiRunnerError:
        raise
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise KijiRunnerError("invalid labels metadata") from exc

    if not isinstance(data, dict):
        raise KijiRunnerError("invalid labels metadata shape")

    metadata_keys = set(data)
    if metadata_keys == {"id2label"}:
        id_to_label = data["id2label"]
        expected_keys = {str(index) for index in PINNED_LABELS}
        if not isinstance(id_to_label, dict) or set(id_to_label) != expected_keys:
            raise KijiRunnerError("invalid pinned id2label mapping")
        for index, expected_label in PINNED_LABELS.items():
            if type(id_to_label[str(index)]) is not str:
                raise KijiRunnerError("invalid pinned id2label mapping")
            if id_to_label[str(index)] != expected_label:
                raise KijiRunnerError("invalid pinned id2label mapping")
        return dict(PINNED_LABELS)

    if metadata_keys != PINNED_LABELS_MANIFEST_KEYS:
        raise KijiRunnerError("invalid labels metadata shape")
    if type(data["schema_version"]) is not int or data["schema_version"] != 1:
        raise KijiRunnerError("invalid pinned label vocabulary metadata")
    if data["source"] != PINNED_LABELS_SOURCE:
        raise KijiRunnerError("invalid pinned label vocabulary metadata")
    if data["source_commit"] != PINNED_LABELS_SOURCE_COMMIT:
        raise KijiRunnerError("invalid pinned label vocabulary metadata")

    labels = data["labels"]
    if not isinstance(labels, list) or len(labels) != len(PINNED_VOCABULARY):
        raise KijiRunnerError("invalid pinned label vocabulary")

    seen_ids: set[str] = set()
    seen_labels: set[str] = set()
    for item in labels:
        if not isinstance(item, dict) or set(item) != {"id", "upstream"}:
            raise KijiRunnerError("invalid pinned label vocabulary")
        entity_id = item["id"]
        upstream = item["upstream"]
        if type(entity_id) is not str or entity_id in seen_ids:
            raise KijiRunnerError("invalid pinned label vocabulary")
        if not isinstance(upstream, list) or any(type(label) is not str for label in upstream):
            raise KijiRunnerError("invalid pinned label vocabulary")
        if len(upstream) != len(set(upstream)):
            raise KijiRunnerError("invalid pinned label vocabulary")
        expected = PINNED_VOCABULARY.get(entity_id)
        if expected is None or frozenset(upstream) != expected:
            raise KijiRunnerError("invalid pinned label vocabulary")
        if seen_labels.intersection(upstream):
            raise KijiRunnerError("invalid pinned label vocabulary")
        seen_ids.add(entity_id)
        seen_labels.update(upstream)

    if seen_ids != set(PINNED_VOCABULARY):
        raise KijiRunnerError("invalid pinned label vocabulary")
    if seen_labels != set(PINNED_LABELS.values()) - {"O"}:
        raise KijiRunnerError("invalid pinned label vocabulary")
    return dict(PINNED_LABELS)


def build_inputs(session: ort.InferenceSession, encoding: Any) -> dict[str, np.ndarray]:
    ids = np.asarray([encoding.ids], dtype=np.int64)
    mask = np.asarray([encoding.attention_mask], dtype=np.int64)
    type_ids = np.asarray([encoding.type_ids], dtype=np.int64)

    values: dict[str, np.ndarray] = {}
    for input_meta in session.get_inputs():
        name = input_meta.name
        lowered = name.lower()
        if "input_ids" in lowered:
            values[name] = ids
        elif "attention" in lowered or "mask" in lowered:
            values[name] = mask
        elif "token_type" in lowered or "segment" in lowered:
            values[name] = type_ids
        else:
            raise KijiRunnerError(f"unsupported ONNX input: {name}")
    return values


def label_for(logits: np.ndarray, id_to_label: dict[int, str]) -> tuple[str, float]:
    label_id = int(np.argmax(logits))
    score = softmax_score(logits, label_id)
    return id_to_label.get(label_id, "O"), score


def softmax_score(logits: np.ndarray, label_id: int) -> float:
    values = logits.astype(np.float64)
    values = values - np.max(values)
    exp = np.exp(values)
    denom = float(np.sum(exp))
    if denom == 0.0 or not np.isfinite(denom):
        return 0.0
    score = float(exp[label_id] / denom)
    if not np.isfinite(score):
        return 0.0
    return score


def split_bio(label: str) -> tuple[str, str | None]:
    if label == "O" or not label:
        return "O", None
    if "-" not in label:
        return "B", label
    prefix, entity = label.split("-", 1)
    if prefix not in {"B", "I"} or not entity:
        return "O", None
    return prefix, entity


def spans_from_predictions(
    predictions: Iterable[tuple[str, int, int, float]],
) -> list[dict[str, Any]]:
    spans: list[dict[str, Any]] = []
    active: dict[str, Any] | None = None
    active_scores: list[float] = []

    def flush() -> None:
        nonlocal active, active_scores
        if active is not None:
            active["score"] = sum(active_scores) / len(active_scores)
            spans.append(active)
        active = None
        active_scores = []

    for raw_label, start, end, score in predictions:
        prefix, entity = split_bio(raw_label)
        if prefix == "O" or entity is None or start == end:
            flush()
            continue

        if (
            active is None
            or prefix == "B"
            or active["label"] != entity
            or active["end"] > start
        ):
            flush()
            active = {"label": entity, "start": start, "end": end}
            active_scores = [score]
            continue

        active["end"] = end
        active_scores.append(score)

    flush()
    return spans


def run(model_dir: Path, text: str, precision: str = "fp32") -> list[dict[str, Any]]:
    if text == "":
        return []

    model_name = "model.int8.onnx" if precision == "int8" else "model.onnx"
    model_path = model_dir / model_name
    tokenizer_path = model_dir / "tokenizer.json"
    labels_path = model_dir / "labels.json"
    for path in [model_path, tokenizer_path, labels_path]:
        if not path.is_file():
            raise KijiRunnerError(f"missing model artifact: {path.name}")

    tokenizer = Tokenizer.from_file(str(tokenizer_path))
    encoding = tokenizer.encode(text)
    if not encoding.ids:
        return []

    session = ort.InferenceSession(str(model_path), providers=["CPUExecutionProvider"])
    outputs = session.run(None, build_inputs(session, encoding))
    if not outputs:
        raise KijiRunnerError("ONNX model returned no outputs")

    logits = np.asarray(outputs[0])
    if logits.ndim == 3:
        logits = logits[0]
    if logits.ndim != 2:
        raise KijiRunnerError(f"unexpected logits shape: {list(logits.shape)}")

    id_to_label = load_labels(labels_path)
    predictions: list[tuple[str, int, int, float]] = []
    for index, offset in enumerate(encoding.offsets):
        if index >= logits.shape[0]:
            break
        start, end = int(offset[0]), int(offset[1])
        if start < 0 or end < start:
            print(f"kiji-runner: skipping invalid token offset at {index}", file=sys.stderr)
            continue
        try:
            label, score = label_for(logits[index], id_to_label)
        except Exception as exc:  # keep one bad token from failing the whole request
            print(f"kiji-runner: skipping token {index}: {exc}", file=sys.stderr)
            continue
        predictions.append((label, start, end, score))

    return spans_from_predictions(predictions)


def main(argv: list[str]) -> int:
    try:
        args = parse_args(argv)
        text = read_stdin()
        spans = run(Path(args.model_dir), text, args.precision)
        json.dump(spans, sys.stdout, separators=(",", ":"))
        sys.stdout.write("\n")
        return 0
    except Exception as exc:
        print(f"kiji-runner: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
