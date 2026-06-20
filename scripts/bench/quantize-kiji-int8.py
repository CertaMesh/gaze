#!/usr/bin/env python3
"""Quantize the pinned Kiji DistilBERT ONNX model to int8.

This helper is intentionally offline: it reads an already-fetched fp32 Kiji
bundle, emits `model.int8.onnx`, and writes a separate `SHA256SUMS.int8`
manifest so fp32 and int8 keep independent integrity pins.
"""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

from onnxruntime.quantization import QuantType, quantize_dynamic


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Quantize pinned Kiji DistilBERT ONNX bundle to int8."
    )
    parser.add_argument(
        "model_dir",
        type=Path,
        help="Directory containing SHA256SUMS, labels.json, model.onnx, tokenizer.json.",
    )
    parser.add_argument(
        "--output-name",
        default="model.int8.onnx",
        help="Quantized ONNX filename inside model_dir.",
    )
    parser.add_argument(
        "--manifest-name",
        default="SHA256SUMS.int8",
        help="Quantized checksum manifest filename inside model_dir.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    model_dir = args.model_dir.expanduser().resolve()
    input_model = model_dir / "model.onnx"
    output_model = model_dir / args.output_name
    manifest_path = model_dir / args.manifest_name

    for required in [input_model, model_dir / "labels.json", model_dir / "tokenizer.json"]:
        if not required.is_file():
            raise SystemExit(f"missing required Kiji artifact: {required}")

    quantize_dynamic(str(input_model), str(output_model), weight_type=QuantType.QInt8)

    entries = [
        f"{sha256(model_dir / 'labels.json')}  labels.json\n",
        f"{sha256(output_model)}  {output_model.name}\n",
        f"{sha256(model_dir / 'tokenizer.json')}  tokenizer.json\n",
    ]
    manifest = "".join(entries)
    manifest_path.write_text(manifest, encoding="utf-8")

    print(f"{output_model.name} {sha256(output_model)}")
    print(f"{manifest_path.name} {hashlib.sha256(manifest.encode('utf-8')).hexdigest()}")
    print(manifest, end="")


if __name__ == "__main__":
    main()
