#!/usr/bin/env python3
"""Measure warm persistent-model latency for pinned NER benchmark candidates."""

from __future__ import annotations

import argparse
import importlib.util
import json
import statistics
import time
from pathlib import Path

import numpy as np
import onnxruntime as ort
from transformers import pipeline

from safety_net_bench_lib import load_fixtures


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument(
        "--corpus-dir",
        type=Path,
        default=Path("crates/gaze-recognizers/testdata/coverage-loop/corpus"),
    )
    return parser.parse_args()


def repo_path(root: Path, path: Path) -> Path:
    return path if path.is_absolute() else root / path


def latency_summary(samples: list[float]) -> dict[str, float | int]:
    ordered = sorted(samples)
    p95_index = int((len(ordered) - 1) * 0.95)
    return {
        "warm_p50_ms": round(statistics.median(ordered), 6),
        "warm_p95_ms": round(ordered[p95_index], 6),
        "warm_mean_ms": round(statistics.mean(ordered), 6),
        "fixture_count": len(ordered),
    }


def load_onnx_runner(root: Path):
    runner = root / "scripts" / "onnx-token-classification-runner.py"
    spec = importlib.util.spec_from_file_location("onnx_token_classification_runner", runner)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load {runner}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def measure_tinybert(root: Path, texts: list[str]) -> dict[str, float | int]:
    runner = load_onnx_runner(root)
    model_dir = root / ".huggingface-cache/models/openobscure-tinybert4l-pii-ner-int8"
    tokenizer = runner.load_tokenizer(model_dir)
    labels = runner.load_labels(model_dir)
    session = ort.InferenceSession(
        str(model_dir / "model_int8.onnx"), providers=["CPUExecutionProvider"]
    )

    def infer(text: str) -> None:
        encoding = tokenizer.encode(text)
        if not encoding.ids:
            return
        output = session.run(None, runner.build_inputs(session, encoding))[0]
        logits = np.asarray(output)
        if logits.ndim == 3:
            logits = logits[0]
        predictions = []
        for index, offset in enumerate(encoding.offsets):
            if index >= logits.shape[0]:
                break
            start = int(offset[0])
            end = int(offset[1])
            if start == end:
                continue
            label_id = int(np.argmax(logits[index]))
            predictions.append((labels.get(label_id, "O"), start, end, 0.0))
        runner.spans_from_predictions(predictions)

    infer(texts[0])
    samples = []
    for text in texts:
        start = time.perf_counter()
        infer(text)
        samples.append((time.perf_counter() - start) * 1000.0)
    return latency_summary(samples)


def measure_transformers(model_dir: Path, texts: list[str]) -> dict[str, float | int]:
    ner = pipeline(
        "ner",
        model=str(model_dir),
        tokenizer=str(model_dir),
        aggregation_strategy="simple",
        device=-1,
    )
    ner(texts[0])
    samples = []
    for text in texts:
        start = time.perf_counter()
        ner(text)
        samples.append((time.perf_counter() - start) * 1000.0)
    return latency_summary(samples)


def main() -> int:
    args = parse_args()
    root = args.repo_root.resolve()
    texts = [fixture.text for fixture in load_fixtures(repo_path(root, args.corpus_dir))]
    results = {
        "openobscure-tinybert4l-pii-ner-int8": measure_tinybert(root, texts),
        "mrm8488-mobilebert-ner": measure_transformers(
            root / ".huggingface-cache/models/mrm8488-mobilebert-ner", texts
        ),
        "osiria-minilm-italian-ner": measure_transformers(
            root / ".huggingface-cache/models/osiria-minilm-italian-ner", texts
        ),
    }
    json.dump(results, fp=sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    import sys

    raise SystemExit(main())
