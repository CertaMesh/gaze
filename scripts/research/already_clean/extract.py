"""Read-only ONNX taps. Never loaded by Gaze. Run with --limit for a pilot."""

import argparse
import hashlib
import json
import os
import platform
import resource
import subprocess
import time
from pathlib import Path

os.environ.setdefault("OMP_NUM_THREADS", "2")
os.environ.setdefault("OPENBLAS_NUM_THREADS", "2")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")

import numpy as np
import onnx
import onnxruntime as ort
from onnx import TensorProto, helper
from tokenizers import Tokenizer

from fixtures import write


MODEL_ROOT = Path.home() / ".local/share/gaze/models"
SPECS = {
    "davlan": {
        "directory": "davlan-mbert-ner-hrl", "file": "model.onnx", "layers": [3, 7, 11],
        "hashes": {"model.onnx": "1213fdd405d295768b0d41d8214062f2f278f0e3acff6af67d8fd47360d2be0f", "tokenizer.json": "bf1b59b7b11c95f194f51708d918eea378e09d05f84c0e1656dc5180e8117088", "config.json": "8e5caefadaf9923a9e7d3de42ca97780c68fc4d83519d333f141b299e40af638"},
    },
    "nym": {
        "directory": "nym-small-int8", "file": "model_int8.onnx", "layers": [3, 7, 15],
        "hashes": {"model_int8.onnx": "139006aea2cbd8e709d322f056232570de54661f624143be4893aaa387190286", "tokenizer.json": "c299144e68dfec1dc536204a7ae3712710c5c6cade9269a83f5042250d47d8de", "config.json": "3f07065571e22bb73eba28ddb1fae4509c0cf703762e3bfa7a6ab2111cf7cb88"},
    },
}


def sha(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def session(path):
    options = ort.SessionOptions()
    options.intra_op_num_threads = 2
    options.inter_op_num_threads = 1
    options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    return ort.InferenceSession(str(path), options, providers=["CPUExecutionProvider"])


def taps(model, layers):
    result = {}
    for layer in layers:
        if model == "davlan":
            prefix = f"/bert/encoder/layer.{layer}"
            result[f"mlp_{layer}"] = (prefix + "/intermediate/intermediate_act_fn/Mul_1_output_0", 3072)
            result[f"hidden_{layer}"] = (prefix + "/output/LayerNorm/Add_1_output_0", 768)
        else:
            prefix = f"/m/model/layers.{layer}"
            result[f"mlp_{layer}"] = (prefix + "/mlp/Mul_2_output_0", 1152)
            result[f"hidden_{layer}"] = (prefix + "/Add_1_output_0", 384)
            result[f"gate_{layer}"] = (prefix + "/mlp/act/Mul_1_output_0", 1152)
            result[f"value_{layer}"] = (prefix + "/mlp/Slice_1_output_0", 1152)
    return result


def instrument(source, destination, tap_map):
    graph = onnx.load(str(source))
    available = {name for n in graph.graph.node for name in n.output}
    for name, width in tap_map.values():
        if name not in available:
            raise ValueError(f"missing native tensor: {name}")
        graph.graph.output.append(helper.make_tensor_value_info(name, TensorProto.FLOAT, ["batch_size", "sequence_length", width]))
    onnx.checker.check_model(graph)
    onnx.save(graph, str(destination))


def encode(tokenizer, text, runtime):
    encoding = tokenizer.encode(text)
    assert len(encoding.ids) < 512, "research short-document contract exceeded"
    values = {"input_ids": encoding.ids, "attention_mask": encoding.attention_mask, "token_type_ids": encoding.type_ids}
    feed = {i.name: np.asarray([values[i.name]], dtype=np.int64) for i in runtime.get_inputs()}
    return encoding, feed


def target_indices(labels, target):
    if target == "name":
        return [i for i, tag in enumerate(labels) if tag.split("-", 1)[-1] in {"PER", "GIVEN_NAME", "SURNAME"}]
    return [i for i, tag in enumerate(labels) if tag.split("-", 1)[-1] in {"ACCOUNT_NUMBER", "CUSTOMER_ID", "EMPLOYEE_ID", "GOVERNMENT_ID", "MEDICAL_RECORD_NUMBER"}]


def token_spans(offsets, logits, labels, target, threshold):
    # Deliberately token-level: no claim of matching Gaze's joiners/word growth/neutral view.
    probabilities = softmax(logits)
    indices = target_indices(labels, target)
    spans = []
    for offset, probs in zip(offsets, probabilities, strict=True):
        if offset[0] == offset[1]:
            continue
        if int(probs.argmax()) in indices and float(probs[indices].sum()) >= threshold:
            spans.append(list(offset))
    return spans


def softmax(array):
    shifted = array - array.max(axis=-1, keepdims=True)
    exponential = np.exp(shifted)
    return exponential / exponential.sum(axis=-1, keepdims=True)


def extract(model, output, limit=None):
    spec = SPECS[model]
    directory = MODEL_ROOT / spec["directory"]
    for filename, expected in spec["hashes"].items():
        assert sha(directory / filename) == expected, f"pinned artifact mismatch: {filename}"
    fixture_path = output / "fixtures.jsonl"
    fixture_hash = write(fixture_path)
    rows = [json.loads(line) for line in fixture_path.read_text().splitlines()]
    if limit:
        rows = [r for r in rows if r["split"] == "train"][:limit]
    tap_map = taps(model, spec["layers"])
    graph_path = output / f"{model}-tapped.onnx"
    instrument(directory / spec["file"], graph_path, tap_map)
    started = time.perf_counter()
    runtime = session(graph_path)
    original = session(directory / spec["file"])
    load_s = time.perf_counter() - started
    tokenizer = Tokenizer.from_file(str(directory / "tokenizer.json"))
    tokenizer.no_truncation()
    tokenizer.no_padding()
    labels_dict = json.loads((directory / "config.json").read_text())["id2label"]
    labels = [labels_dict[str(i)] for i in range(len(labels_dict))]
    arrays = {key: [] for key in tap_map}
    arrays.update({"logits": [], "mass": [], "ids": []})
    records, parity, times = [], [], []
    output_names = ["logits"] + [name for name, _ in tap_map.values()]
    for index, row in enumerate(rows):
        encoding, feed = encode(tokenizer, row["text"], runtime)
        t0 = time.perf_counter()
        outputs = runtime.run(output_names, feed)
        times.append(time.perf_counter() - t0)
        logits = outputs[0][0]
        assert logits.shape == (len(encoding.ids), len(labels)) and np.isfinite(logits).all()
        # Graph parity is checked throughout the data, including every split/language.
        if index % 16 == 0 or index == len(rows) - 1:
            reference = original.run(["logits"], feed)[0][0]
            delta = float(np.abs(reference - logits).max())
            assert delta <= 1e-3, f"tap changed logits: {delta}"
            assert np.array_equal(reference.argmax(-1), logits.argmax(-1)), "tap changed predicted tag"
            parity.append({"id": row["id"], "max_logit_delta": delta})
        a, b = row["span"]
        positions = [i for i, (left, right) in enumerate(encoding.offsets) if left < b and right > a and left < right]
        assert positions, f"unrepresented candidate: {row['id']}"
        probabilities = softmax(logits[positions])
        indices = target_indices(labels, row["target"])
        mass = probabilities[:, indices].sum(-1) if indices else np.zeros(len(positions))
        arrays["mass"].append([mass.mean(), mass.min(), mass.max()])
        arrays["logits"].append(np.concatenate([probabilities.mean(0), probabilities.min(0), probabilities.max(0), [row["target"] == "identifier"]]))
        tensors = {key: value[0] for key, value in zip(tap_map, outputs[1:], strict=True)}
        for key, tensor in tensors.items():
            assert tensor.shape == (len(encoding.ids), tap_map[key][1]) and np.isfinite(tensor).all()
            arrays[key].append(tensor[positions].mean(0))
        if model == "nym":
            for layer in spec["layers"]:
                assert np.allclose(tensors[f"mlp_{layer}"], tensors[f"gate_{layer}"] * tensors[f"value_{layer}"], rtol=1e-5, atol=1e-5)
        arrays["ids"].append(row["id"])
        records.append({"id": row["id"], "tokens": len(encoding.ids), "target_tokens": len(positions), "spans": token_spans(encoding.offsets, logits, labels, row["target"], 0.3), "gate_double_negative_fraction": {str(layer): float(((tensors[f'gate_{layer}'][positions] < 0) & (tensors[f'value_{layer}'][positions] < 0)).mean()) for layer in spec["layers"]} if model == "nym" else {}})
        if (index + 1) % 32 == 0:
            print(json.dumps({"model": model, "completed": index + 1, "total": len(rows)}), flush=True)
    np.savez_compressed(output / f"{model}-features.npz", **{k: np.asarray(v) for k, v in arrays.items()})
    (output / f"{model}-tokens.json").write_text(json.dumps(records, ensure_ascii=False))
    metadata = {
        "model": model, "model_hashes": spec["hashes"], "model_bytes": (directory / spec["file"]).stat().st_size,
        "fixture_sha256": fixture_hash, "documents": len(rows), "pilot": bool(limit), "tap_map": tap_map,
        "source_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "hardware": subprocess.check_output(["system_profiler", "SPHardwareDataType"], text=True).split("Serial Number")[0],
        "platform": platform.platform(), "threads": 2, "parity": parity, "session_load_s": load_s,
        "forward_s": times, "timing_scope": "one shared-host instrumented traversal; descriptive, no speed comparison",
        "process_peak_rss_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
        "rss_scope": "whole extractor with both original and tapped sessions; NOT production incremental RSS",
        "failures": 0, "work_count": sum(r["tokens"] for r in records),
    }
    (output / f"{model}-metadata.json").write_text(json.dumps(metadata, indent=2))
    print(json.dumps({"model": model, "documents": len(rows), "parity_max": max(p["max_logit_delta"] for p in parity), "forward_total_s": sum(times), "rss_bytes": metadata["process_peak_rss_bytes"]}), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("model", choices=SPECS)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--limit", type=int)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    extract(args.model, args.output, args.limit)
