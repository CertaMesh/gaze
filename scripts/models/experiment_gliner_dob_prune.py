# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy==2.5.3", "onnx==1.23.0", "onnxruntime==1.24.2", "tokenizers==0.23.2"]
# ///
"""Reproduce the rejected, lightly pruned GLiNER DOB memory experiment.

Run with ``uv run scripts/models/experiment_gliner_dob_prune.py SOURCE OUTPUT``. The
ONNX Runtime's BASIC graph rewrite removes redundant load work while retaining
standard ONNX operators. Keep all but the last 20,000 ranked Unigram pieces,
plus every single-character piece and the special tokens. The recipe
is independent of Gaze's benchmark. This bundle changed held-out scores and
has not passed the required decision and benchmark gates. Do not ship it.
"""

import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort
from tokenizers import Tokenizer


SOURCE_BUNDLE_SHA256 = "eddb1943a13778f91bf9e51f23cdeb1f8b54d9c83f7a5831aa06320fe7cc4b23"
FILES = ("model.onnx", "tokenizer.json", "gliner_config.json")
TAIL_PIECES = 20_000
EMBEDDING = "token_rep_layer.bert_layer.model.embeddings.word_embeddings.weight_quantized"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_source(source: Path) -> None:
    sums_path = source / "SHA256SUMS"
    if sha256(sums_path) != SOURCE_BUNDLE_SHA256:
        raise ValueError("source bundle checksum pin does not match")
    lines = sums_path.read_text(encoding="ascii").splitlines()
    entries = [line.split("  ", 1) for line in lines]
    if len(entries) != len(FILES) or [name for _, name in entries] != list(FILES):
        raise ValueError("source bundle manifest has unexpected files")
    for digest, name in entries:
        artifact = source / name
        if artifact.is_symlink() or sha256(artifact) != digest:
            raise ValueError(f"source bundle artifact failed verification: {name}")


def optimize(source: Path, output: Path) -> dict[str, int | str]:
    verify_source(source)
    if output.exists() and any(output.iterdir()):
        raise ValueError(f"output directory is not empty: {output}")
    output.mkdir(parents=True, exist_ok=True)
    output.chmod(0o700)

    options = ort.SessionOptions()
    options.intra_op_num_threads = 1
    options.inter_op_num_threads = 1
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_BASIC
    optimized_path = output / "model.onnx"
    options.optimized_model_filepath = str(optimized_path)
    ort.InferenceSession(str(source / "model.onnx"), options, providers=["CPUExecutionProvider"])
    model = onnx.load(optimized_path)
    embedding = next(t for t in model.graph.initializer if t.name == EMBEDDING)
    tokenizer = json.loads((source / "tokenizer.json").read_text(encoding="utf-8"))
    config = json.loads((source / "gliner_config.json").read_text(encoding="utf-8"))
    vocab = tokenizer["model"]["vocab"]
    rows, width = embedding.dims
    assert rows == len(vocab) + 4 and width == 768
    keep = sorted(
        i for i, (piece, _) in enumerate(vocab)
        if i < len(vocab) - TAIL_PIECES or len(piece.removeprefix("▁")) <= 1
    )
    old_ids = keep + list(range(len(vocab), rows))
    remap = {old: new for new, old in enumerate(old_ids)}
    raw = np.frombuffer(embedding.raw_data, dtype=np.uint8).reshape(rows, width)
    embedding.raw_data = raw[old_ids].tobytes()
    embedding.ClearField("dims")
    embedding.dims.extend((len(old_ids), width))
    class_token = config["class_token_index"]
    constants = [
        t for t in model.graph.initializer
        if t.data_type == onnx.TensorProto.INT64
        and not t.dims
        and np.array_equal(onnx.numpy_helper.to_array(t), class_token)
    ]
    if len(constants) != 1:
        raise ValueError(f"expected one class-token constant, found {len(constants)}")
    constants[0].raw_data = np.int64(remap[class_token]).tobytes()
    onnx.checker.check_model(model)
    onnx.save(model, optimized_path)

    tokenizer["model"]["vocab"] = [vocab[i] for i in keep]
    tokenizer["model"]["unk_id"] = remap[tokenizer["model"]["unk_id"]]
    for token in tokenizer["added_tokens"]:
        token["id"] = remap[token["id"]]
    for special in tokenizer["post_processor"]["special_tokens"].values():
        special["ids"] = [remap[i] for i in special["ids"]]
    (output / "tokenizer.json").write_text(
        json.dumps(tokenizer, ensure_ascii=False, separators=(",", ":")), encoding="utf-8"
    )
    Tokenizer.from_file(str(output / "tokenizer.json"))
    config["class_token_index"] = remap[class_token]
    config["vocab_size"] = len(old_ids)
    (output / "gliner_config.json").write_text(
        json.dumps(config, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    for name in FILES:
        (output / name).chmod(0o600)
    sums = "".join(f"{sha256(output / name)}  {name}\n" for name in FILES)
    (output / "SHA256SUMS").write_text(sums, encoding="ascii")
    (output / "SHA256SUMS").chmod(0o600)
    return {
        "source_bundle_sha256": SOURCE_BUNDLE_SHA256,
        "output_bundle_sha256": sha256(output / "SHA256SUMS"),
        "source_model_bytes": (source / "model.onnx").stat().st_size,
        "output_model_bytes": (output / "model.onnx").stat().st_size,
        "source_vocab": rows,
        "output_vocab": len(old_ids),
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    print(json.dumps(optimize(args.source, args.output), indent=2))
