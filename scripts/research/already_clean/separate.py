"""Independent multilingual encoder comparator; pinned download, local inference."""

import argparse
import json
import time
import urllib.request
from pathlib import Path

import numpy as np
from tokenizers import Tokenizer

from extract import encode, session, sha
from fixtures import write

REPO = "Xenova/paraphrase-multilingual-MiniLM-L12-v2"
REVISION = "2c4055b12046f11709e9df2c122e59ffbdc2f900"
FILES = {
    "onnx/model_quantized.onnx": "66fc00f5f29afcaff34092e1bdd20008ca3918265a82fb9695a551e510cc4ebc",
    "tokenizer.json": "b60b6b43406a48bf3638526314f3d232d97058bc93472ff2de930d43686fa441",
}


def download(directory):
    for name, digest in FILES.items():
        path = directory / Path(name).name
        if not path.exists() or sha(path) != digest:
            path.parent.mkdir(parents=True, exist_ok=True)
            temporary = path.with_suffix(".partial")
            urllib.request.urlretrieve(f"https://huggingface.co/{REPO}/resolve/{REVISION}/{name}", temporary)
            assert sha(temporary) == digest
            temporary.replace(path)


def run(directory, output, download_only=False):
    download(directory)
    if download_only:
        print(json.dumps({"downloaded": str(directory), "revision": REVISION}))
        return
    fixture_hash = write(output / "fixtures.jsonl")
    rows = [json.loads(line) for line in (output / "fixtures.jsonl").read_text().splitlines()]
    runtime = session(directory / "model_quantized.onnx")
    tokenizer = Tokenizer.from_file(str(directory / "tokenizer.json"))
    tokenizer.no_truncation()
    tokenizer.no_padding()
    features, times = [], []
    for index, row in enumerate(rows):
        encoding, feed = encode(tokenizer, row["text"], runtime)
        assert len(encoding.ids) <= 128, "exceeds encoder's documented sentence limit"
        start = time.perf_counter()
        tensor = runtime.run(None, feed)[0][0]
        times.append(time.perf_counter() - start)
        assert tensor.shape == (len(encoding.ids), 384) and np.isfinite(tensor).all()
        a, b = row["span"]
        positions = [i for i, (left, right) in enumerate(encoding.offsets) if left < b and right > a and left < right]
        assert positions
        features.append(np.concatenate([tensor[positions].mean(0), tensor.mean(0)]))
        if (index + 1) % 128 == 0:
            print(json.dumps({"model": "minilm", "completed": index + 1}), flush=True)
    np.savez_compressed(output / "minilm-features.npz", ids=np.asarray([r["id"] for r in rows]), embedding=np.asarray(features))
    (output / "minilm-metadata.json").write_text(json.dumps({"repository": REPO, "revision": REVISION, "hashes": FILES, "fixture_sha256": fixture_hash, "documents": len(rows), "forward_s": times, "model_bytes": (directory / 'model_quantized.onnx').stat().st_size, "failures": 0, "scope": "independent pretrained sentence encoder with supervised candidate+document pooling, NOT a neuron interpretability result"}, indent=2))
    print(json.dumps({"model": "minilm", "documents": len(rows), "forward_total_s": sum(times)}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--download-only", action="store_true")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    run(args.directory, args.output, args.download_only)
