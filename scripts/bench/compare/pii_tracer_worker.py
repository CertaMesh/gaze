"""PII-Tracer inference worker: one JSON request per stdin line, one reply per stdout line.

Runs in its own virtualenv because the checkpoint needs transformers>=5.2, which the
comparison's locked environment cannot take. The checkpoint ships Python code that
`trust_remote_code` executes, so nothing loads until the model directory's files
match the pinned SHA-256 values below; a mismatch aborts before any import of the
model code. The code is reviewed: it imports only torch and transformers and opens
no network connection.

Long documents follow the model card ("chunk longer documents before calling
predict"): non-overlapping token windows, each decoded independently. Truncation
would silently drop the tail of a document, which the byte scorer would count as
leaked bytes the tool was never asked to cover.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path

REVISION = "d25c16f2e57e321f6d2527715c01df9112f956f5"
PINNED_SHA256 = {
    "modeling_pii_masking.py": "c43724b9ac105a9b74d44aea7565e4c43ddc9d203f519324baa342be01269869",
    "model.safetensors": "f6204155ec540c9323f706e284110ee848b462f0325dc1ece5c7263fc517bbd0",
    "config.json": "7162c30da7009cfb9c9af6204782177ef6f5d88e2653fd87d6514308d97dcaa7",
    "tokenizer.json": "cae14d1c8dda080f23792355b0692b826bf1f1da3c86ebc1b37548a391cf6526",
    "tokenizer_config.json": "aa9c1b0a1c9b48c2f70bacdf64f7dab25194be4ffea0c6a6e4da262360a91d0a",
}
# Leaves room for the tokenizer to split a window boundary differently on re-encode.
WINDOW_MARGIN = 16


def verify_files(model_dir: Path) -> None:
    for name, expected in PINNED_SHA256.items():
        digest = hashlib.sha256()
        with (model_dir / name).open("rb") as handle:
            for block in iter(lambda: handle.read(1 << 20), b""):
                digest.update(block)
        if digest.hexdigest() != expected:
            raise SystemExit(f"{name} does not match PII-Tracer revision {REVISION}; refusing to load")


def windows(offsets: list[tuple[int, int]], size: int) -> list[tuple[int, int]]:
    """Character ranges of consecutive token windows that together cover the text."""
    starts = list(range(0, len(offsets), size))
    ranges = []
    for index, first in enumerate(starts):
        begin = 0 if index == 0 else offsets[first][0]
        end = offsets[starts[index + 1]][0] if index + 1 < len(starts) else offsets[-1][1]
        ranges.append((begin, end))
    return ranges


def detect(model: object, text: str) -> list[list[object]]:
    limit = model.config.max_seq_len
    encoded = model.tokenizer(text, return_offsets_mapping=True)
    offsets = [tuple(pair) for pair in encoded["offset_mapping"]]
    if len(offsets) <= limit:
        chunks = [(0, len(text))]
    else:
        chunks = windows(offsets, limit - WINDOW_MARGIN)
    found = []
    for begin, end in chunks:
        piece = text[begin:end]
        if len(model.tokenizer(piece)["input_ids"]) > limit:
            raise RuntimeError("chunk exceeds the model window; refusing to truncate")
        spans, _sensitivity = model.predict(piece)
        found.extend([begin + span.start, begin + span.end, span.label] for span in spans)
    return found


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model-dir", type=Path, required=True)
    args = parser.parse_args()
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    os.environ["TOKENIZERS_PARALLELISM"] = "false"
    verify_files(args.model_dir)
    import torch
    from transformers import AutoModel

    model = AutoModel.from_pretrained(str(args.model_dir), trust_remote_code=True).eval()
    reply = sys.stdout
    sys.stdout = sys.stderr  # library chatter must never corrupt the reply channel
    versions = {"torch": torch.__version__, "device": "cpu",
                "dtype": str(next(model.parameters()).dtype).removeprefix("torch.")}
    import transformers
    versions["transformers"] = transformers.__version__
    reply.write("PII_TRACER_READY " + json.dumps(versions, sort_keys=True) + "\n")
    reply.flush()
    with torch.inference_mode():
        for line in sys.stdin:
            request = json.loads(line)
            try:
                answer = {"id": request["id"], "spans": detect(model, request["text"])}
            except Exception as error:  # reported to the parent, which fails the run
                answer = {"id": request["id"], "error": f"{type(error).__name__}: {error}"}
            reply.write(json.dumps(answer, ensure_ascii=False) + "\n")
            reply.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
