#!/usr/bin/env python3
"""Generic ONNX Runtime token-classification subprocess wrapper."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Iterable

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer
from tokenizers.implementations import BertWordPieceTokenizer


MAX_INPUT_BYTES = 1024 * 1024


class OnnxTokenClassificationError(RuntimeError):
    pass


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", required=True, choices=["json"])
    parser.add_argument("--output-mode", required=True, choices=["typed"])
    parser.add_argument("--model-dir", required=True)
    parser.add_argument("--model-file", default="model.onnx")
    return parser.parse_args(argv)


def read_stdin() -> str:
    raw = sys.stdin.buffer.read(MAX_INPUT_BYTES + 1)
    if len(raw) > MAX_INPUT_BYTES:
        raise OnnxTokenClassificationError(f"stdin exceeds {MAX_INPUT_BYTES} byte cap")
    return raw.decode("utf-8")


def load_tokenizer(model_dir: Path) -> Tokenizer:
    tokenizer_json = model_dir / "tokenizer.json"
    if tokenizer_json.is_file():
        return Tokenizer.from_file(str(tokenizer_json))

    vocab = model_dir / "vocab.txt"
    if not vocab.is_file():
        raise OnnxTokenClassificationError("missing tokenizer.json or vocab.txt")
    return BertWordPieceTokenizer(str(vocab), lowercase=True)


def load_labels(model_dir: Path) -> dict[int, str]:
    for name in ["config.json", "label_map.json", "labels.json"]:
        path = model_dir / name
        if not path.is_file():
            continue
        data = json.loads(path.read_text(encoding="utf-8"))
        if isinstance(data, dict) and isinstance(data.get("id2label"), dict):
            return {int(key): str(value) for key, value in data["id2label"].items()}
        if isinstance(data, dict) and all(str(key).isdigit() for key in data):
            return {int(key): str(value) for key, value in data.items()}
    raise OnnxTokenClassificationError("missing id2label map")


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
            raise OnnxTokenClassificationError(f"unsupported ONNX input: {name}")
    return values


def softmax_score(logits: np.ndarray, label_id: int) -> float:
    values = logits.astype(np.float64)
    values = values - np.max(values)
    exp = np.exp(values)
    denom = float(np.sum(exp))
    if denom == 0.0 or not np.isfinite(denom):
        return 0.0
    score = float(exp[label_id] / denom)
    return score if np.isfinite(score) else 0.0


def split_label(label: str) -> tuple[str, str | None]:
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
        prefix, entity = split_label(raw_label)
        if prefix == "O" or entity is None or start == end:
            flush()
            continue
        if active is None or prefix == "B" or active["label"] != entity or active["end"] > start:
            flush()
            active = {"label": entity, "start": start, "end": end}
            active_scores = [score]
            continue
        active["end"] = end
        active_scores.append(score)

    flush()
    return spans


def run(model_dir: Path, model_file: str, text: str) -> list[dict[str, Any]]:
    if text == "":
        return []
    model_path = model_dir / model_file
    if not model_path.is_file():
        raise OnnxTokenClassificationError(f"missing model artifact: {model_file}")

    tokenizer = load_tokenizer(model_dir)
    labels = load_labels(model_dir)
    encoding = tokenizer.encode(text)
    if not encoding.ids:
        return []

    session = ort.InferenceSession(str(model_path), providers=["CPUExecutionProvider"])
    outputs = session.run(None, build_inputs(session, encoding))
    if not outputs:
        raise OnnxTokenClassificationError("ONNX model returned no outputs")
    logits = np.asarray(outputs[0])
    if logits.ndim == 3:
        logits = logits[0]
    if logits.ndim != 2:
        raise OnnxTokenClassificationError(f"unexpected logits shape: {list(logits.shape)}")

    predictions: list[tuple[str, int, int, float]] = []
    for index, offset in enumerate(encoding.offsets):
        if index >= logits.shape[0]:
            break
        start, end = int(offset[0]), int(offset[1])
        if start < 0 or end < start:
            continue
        label_id = int(np.argmax(logits[index]))
        label = labels.get(label_id, "O")
        predictions.append((label, start, end, softmax_score(logits[index], label_id)))
    return spans_from_predictions(predictions)


def main(argv: list[str]) -> int:
    try:
        args = parse_args(argv)
        spans = run(Path(args.model_dir), args.model_file, read_stdin())
        json.dump(spans, sys.stdout, separators=(",", ":"))
        sys.stdout.write("\n")
        return 0
    except Exception as exc:
        print(f"onnx-token-classification-runner: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
