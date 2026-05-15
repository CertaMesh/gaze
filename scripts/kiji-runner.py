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
LABEL_FALLBACK = {
    0: "O",
    1: "B-MISC",
    2: "I-MISC",
    3: "B-PER",
    4: "I-PER",
    5: "B-ORG",
    6: "I-ORG",
    7: "B-LOC",
    8: "I-LOC",
}


class KijiRunnerError(RuntimeError):
    pass


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
    with path.open("r", encoding="utf-8") as handle:
        data = json.load(handle)

    id_to_label: dict[int, str] = {}
    if isinstance(data, dict) and isinstance(data.get("id2label"), dict):
        for key, value in data["id2label"].items():
            id_to_label[int(key)] = str(value)
    elif isinstance(data, dict) and isinstance(data.get("labels"), list):
        # Gaze's pinned labels.json records the closed allowed upstream labels,
        # not the ONNX classifier id order. The upstream DistilBERT NER export
        # uses the standard id2label order below.
        allowed = {
            str(label)
            for item in data["labels"]
            for label in item.get("upstream", [])
        }
        id_to_label = {
            key: value
            for key, value in LABEL_FALLBACK.items()
            if value == "O" or value in allowed
        }

    if not id_to_label:
        id_to_label = dict(LABEL_FALLBACK)
    return id_to_label


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
