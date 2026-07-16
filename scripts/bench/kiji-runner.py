#!/usr/bin/env python3
"""Reference Kiji DistilBERT subprocess wrapper for Gaze SafetyNet."""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path
from typing import Any, Iterable, Sequence

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
            raise KijiRunnerError("unsupported ONNX input shape")
    return values


def label_for(logits: np.ndarray, id_to_label: dict[int, str]) -> tuple[str, float]:
    if getattr(logits, "ndim", None) != 1 or getattr(logits, "shape", None) != (9,):
        raise KijiRunnerError("invalid classifier row width")
    try:
        values = tuple(float(value) for value in logits)
    except (TypeError, ValueError, OverflowError) as exc:
        raise KijiRunnerError("invalid classifier row") from exc
    if len(values) != 9 or any(not math.isfinite(value) for value in values):
        raise KijiRunnerError("non-finite classifier row")
    label_id = max(range(len(values)), key=values.__getitem__)
    label = id_to_label.get(label_id)
    if label is None or label != PINNED_LABELS.get(label_id):
        raise KijiRunnerError("unknown classifier label id")
    return label, softmax_score(values, label_id)


def softmax_score(logits: Sequence[float], label_id: int) -> float:
    try:
        maximum = max(logits)
        exponentials = tuple(math.exp(value - maximum) for value in logits)
        denominator = sum(exponentials)
        score = exponentials[label_id] / denominator
    except (ArithmeticError, IndexError, TypeError, ValueError) as exc:
        raise KijiRunnerError("invalid classifier confidence") from exc
    if denominator <= 0.0 or not math.isfinite(denominator) or not math.isfinite(score):
        raise KijiRunnerError("non-finite classifier confidence")
    return score


def utf8_boundaries(text: str) -> tuple[int, set[int]]:
    boundaries = {0}
    byte_length = 0
    for character in text:
        byte_length += len(character.encode("utf-8"))
        boundaries.add(byte_length)
    return byte_length, boundaries


def validate_offset(
    offset: Any,
    text_byte_length: int,
    boundaries: set[int],
    token_index: int,
) -> tuple[int, int]:
    if not isinstance(offset, (list, tuple)) or len(offset) != 2:
        raise KijiRunnerError(f"token {token_index}: invalid token offset shape")
    start, end = offset
    if type(start) is not int or type(end) is not int:
        raise KijiRunnerError(f"token {token_index}: invalid token offset type")
    if start < 0 or end < start or end > text_byte_length:
        raise KijiRunnerError(f"token {token_index}: invalid token offset bounds")
    if start not in boundaries or end not in boundaries:
        raise KijiRunnerError(f"token {token_index}: invalid token offset boundary")
    return start, end


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
    offsets = encoding.offsets
    if len(offsets) != len(encoding.ids):
        raise KijiRunnerError("tokenizer returned mismatched offsets")

    session = ort.InferenceSession(str(model_path), providers=["CPUExecutionProvider"])
    outputs = session.run(None, build_inputs(session, encoding))
    if not outputs:
        raise KijiRunnerError("ONNX model returned no outputs")

    logits = np.asarray(outputs[0])
    expected_shape = (1, len(offsets), 9)
    if logits.ndim != 3 or tuple(logits.shape) != expected_shape:
        raise KijiRunnerError("ONNX model returned invalid logits shape")
    logits = logits[0]

    id_to_label = load_labels(labels_path)
    text_byte_length, boundaries = utf8_boundaries(text)
    predictions: list[tuple[str, int, int, float]] = []
    for index, offset in enumerate(offsets):
        try:
            start, end = validate_offset(offset, text_byte_length, boundaries, index)
            label, score = label_for(logits[index], id_to_label)
        except Exception as exc:
            raise KijiRunnerError(f"token {index}: token decode failed") from exc
        predictions.append((label, start, end, score))

    return spans_from_predictions(predictions)


def main(argv: list[str]) -> int:
    try:
        args = parse_args(argv)
        text = read_stdin()
        spans = run(Path(args.model_dir), text, args.precision)
        output = json.dumps(spans, separators=(",", ":"), allow_nan=False)
        sys.stdout.write(f"{output}\n")
        return 0
    except KijiRunnerError as exc:
        print(f"kiji-runner: {exc}", file=sys.stderr)
        return 1
    except Exception:
        print("kiji-runner: request failed", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
