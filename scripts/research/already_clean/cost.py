"""Five interleaved process trials. These are model micro-costs, not Gaze latency."""

import argparse
import json
import os
import resource
import subprocess
import sys
import time
from pathlib import Path

os.environ.setdefault("OMP_NUM_THREADS", "2")
os.environ.setdefault("OPENBLAS_NUM_THREADS", "2")

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer

from extract import MODEL_ROOT, SPECS, encode, session, taps


def worker(model, mode, directory, profile=False):
    spec = SPECS[model]
    original = MODEL_ROOT / spec["directory"] / spec["file"]
    path = original if mode == "original" else directory / f"{model}-tapped.onnx"
    rows = [json.loads(line) for line in (directory / "fixtures.jsonl").read_text().splitlines()]
    rows = [r for r in rows if r["evaluation"] == "train" and r["language"] in {"en", "de"} and r["id"].split('/')[3] == "0"]
    tokenizer = Tokenizer.from_file(str(MODEL_ROOT / spec["directory"] / "tokenizer.json"))
    tokenizer.no_truncation()
    tokenizer.no_padding()
    t0 = time.perf_counter()
    if profile:
        options = ort.SessionOptions()
        options.intra_op_num_threads = 2
        options.inter_op_num_threads = 1
        options.enable_profiling = True
        options.profile_file_prefix = str(directory / f"{model}-ort-profile")
        runtime = ort.InferenceSession(str(path), options, providers=["CPUExecutionProvider"])
    else:
        runtime = session(path)
    load_s = time.perf_counter() - t0
    feeds = [encode(tokenizer, r["text"], runtime)[1] for r in rows]
    names = ["logits"] if mode == "original" else ["logits"] + [name for name, _ in taps(model, spec["layers"]).values()]
    for feed in feeds[:2]:
        runtime.run(names, feed)
    cpu = time.process_time()
    t0 = time.perf_counter()
    finite = 0
    checksum = 0.0
    for feed in feeds:
        output = runtime.run(names, feed)
        assert all(np.isfinite(array).all() for array in output)
        finite += 1
        checksum += float(output[0].sum())
        if mode == "tapped":
            # Consume the captured tensors inside the timed region, as the extractor does.
            checksum += sum(float(array.mean()) for array in output[1:])
    wall_s = time.perf_counter() - t0
    cpu_s = time.process_time() - cpu
    result = {"mode": mode, "documents": finite, "tokens": sum(f["input_ids"].size for f in feeds), "wall_s": wall_s, "cpu_s": cpu_s, "load_s": load_s, "peak_rss_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss, "checksum": checksum, "failures": 0}
    if profile:
        profile_path = runtime.end_profiling()
        nodes = [e for e in json.loads(Path(profile_path).read_text()) if e.get("cat") == "Node"]
        operators = {}
        for event in nodes:
            name = event.get("args", {}).get("op_name", "unknown")
            operators[name] = operators.get(name, 0) + event.get("dur", 0)
        result["profile_operator_us"] = dict(sorted(operators.items(), key=lambda item: -item[1]))
        result["profile_path"] = profile_path
    return result


def run(model, directory, output):
    trials = []
    for _ in range(5):
        for mode in ["original", "tapped"]:
            result = subprocess.run([sys.executable, __file__, "--model", model, "--directory", str(directory), "--worker", mode], check=True, capture_output=True, text=True)
            trials.append(json.loads(result.stdout))
    result = subprocess.run([sys.executable, __file__, "--model", model, "--directory", str(directory), "--worker", "original", "--profile"], check=True, capture_output=True, text=True)
    output.write_text(json.dumps({"model": model, "scope": "18 short EN/DE train docs; warm CPU model inference plus consumed outputs, tokenization excluded; five interleaved fresh-process trials; shared host, NOT Gaze end-to-end", "trials": trials, "unreported_profile": json.loads(result.stdout)}, indent=2))
    print(json.dumps({"model": model, "trials": len(trials), "failures": 0, "output": str(output)}), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", choices=SPECS, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--worker", choices=["original", "tapped"])
    parser.add_argument("--profile", action="store_true")
    args = parser.parse_args()
    if args.worker:
        print(json.dumps(worker(args.model, args.worker, args.directory, args.profile)))
    else:
        assert args.output
        run(args.model, args.directory, args.output)
