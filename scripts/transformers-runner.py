#!/usr/bin/env python3
"""Generic Hugging Face transformers NER subprocess wrapper."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

from transformers import pipeline


MAX_INPUT_BYTES = 1024 * 1024


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", required=True, choices=["json"])
    parser.add_argument("--output-mode", required=True, choices=["typed"])
    parser.add_argument("--checkpoint", required=True)
    parser.add_argument("--device", default="cpu", choices=["cpu"])
    return parser.parse_args(argv)


def read_stdin() -> str:
    raw = sys.stdin.buffer.read(MAX_INPUT_BYTES + 1)
    if len(raw) > MAX_INPUT_BYTES:
        raise RuntimeError(f"stdin exceeds {MAX_INPUT_BYTES} byte cap")
    return raw.decode("utf-8")


def normalize_label(raw: dict[str, Any]) -> str:
    label = str(raw.get("entity_group") or raw.get("entity") or raw.get("label") or "")
    if "-" in label:
        prefix, rest = label.split("-", 1)
        if prefix in {"B", "I"}:
            label = rest
    return label


def run(checkpoint: Path | str, text: str) -> list[dict[str, Any]]:
    if text == "":
        return []

    ner = pipeline(
        "ner",
        model=str(checkpoint),
        tokenizer=str(checkpoint),
        aggregation_strategy="simple",
        device=-1,
    )
    spans: list[dict[str, Any]] = []
    for raw in ner(text):
        start = int(raw["start"])
        end = int(raw["end"])
        if start == end:
            continue
        spans.append(
            {
                "label": normalize_label(raw),
                "start": start,
                "end": end,
                "score": float(raw.get("score", 0.0)),
            }
        )
    return spans


def main(argv: list[str]) -> int:
    try:
        args = parse_args(argv)
        spans = run(Path(args.checkpoint).expanduser(), read_stdin())
        json.dump(spans, sys.stdout, separators=(",", ":"))
        sys.stdout.write("\n")
        return 0
    except Exception as exc:
        print(f"transformers-runner: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
