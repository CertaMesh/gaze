#!/usr/bin/env python3
"""Alternate process-cold CLI cleans; validate output and record child peak RSS.

Each sample starts a fresh default release CLI with core/en-US and an empty cwd.
The OS page cache is not flushed. This measures loader + rule construction + first
clean + JSON serialization, and not isolated recognizer throughput. Both inputs
are synthetic and differ by exactly one ZWSP. No model/policy download occurs.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import threading
import time


def host():
    return {"load_average": os.getloadavg(), "cpu_count": os.cpu_count(),
            "memory_pressure": subprocess.check_output(["memory_pressure"], text=True).splitlines()[-1]}


def once(binary, text, cwd):
    started = time.perf_counter_ns()
    child = subprocess.Popen([str(binary), "clean", "--locale", "en-US"],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             cwd=cwd, env=dict(os.environ, GAZE_DISABLE_OS_KEYSTORE="1"))
    captured = {}
    readers = [threading.Thread(target=lambda name=name, stream=stream: captured.__setitem__(name, stream.read()))
               for name, stream in (("stdout", child.stdout), ("stderr", child.stderr))]
    for reader in readers:
        reader.start()
    child.stdin.write(text.encode())
    child.stdin.close()
    _, status, usage = os.wait4(child.pid, 0)
    elapsed = (time.perf_counter_ns() - started) / 1e6
    child.returncode = os.waitstatus_to_exitcode(status)
    for reader in readers:
        reader.join()
    child.stdout.close()
    child.stderr.close()
    if child.returncode:
        raise RuntimeError(f"clean failed, exit {child.returncode}")
    response = json.loads(captured["stdout"])
    if not response["entries"] or response["clean_text"] == text:
        raise RuntimeError("sample did not perform protection")
    return {"milliseconds": elapsed,
            "peak_rss_bytes": usage.ru_maxrss if sys.platform == "darwin" else usage.ru_maxrss * 1024,
            "detections": response["stats"]["detections"], "exit_code": child.returncode}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-bin", type=Path, required=True)
    parser.add_argument("--candidate-bin", type=Path, required=True)
    parser.add_argument("--base-sha", required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--rounds", type=int, default=7)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.rounds < 5:
        parser.error("at least five alternating rounds are required")
    binaries = {"base": args.base_bin.resolve(strict=True), "candidate": args.candidate_bin.resolve(strict=True)}
    inputs = {"plain": "Order received. Card: 4111111111111111; email cedar@example.invalid.",
              "zwsp": "Order received. Card: 4111\u200b111111111111; email cedar@example.invalid."}
    before = host()
    samples = {side: {kind: [] for kind in inputs} for side in binaries}
    with tempfile.TemporaryDirectory(prefix="gaze-invisible-cold-") as cwd:
        for index in range(args.rounds):
            sides = list(binaries) if index % 2 == 0 else list(reversed(binaries))
            for kind, text in inputs.items():
                for side in sides:
                    samples[side][kind].append(once(binaries[side], text, cwd))
    summary = {side: {kind: {
        "median_ms": statistics.median(row["milliseconds"] for row in rows),
        "min_ms": min(row["milliseconds"] for row in rows), "max_ms": max(row["milliseconds"] for row in rows),
        "peak_rss_min_bytes": min(row["peak_rss_bytes"] for row in rows),
        "peak_rss_max_bytes": max(row["peak_rss_bytes"] for row in rows),
        "detections": sorted({row["detections"] for row in rows}),
    } for kind, rows in kinds.items()} for side, kinds in samples.items()}
    report = {"schema": "gaze-invisible-process-cold-v1", "rounds": args.rounds,
              "base_sha": args.base_sha, "candidate_sha": args.candidate_sha,
              "binary_sha256": {side: hashlib.sha256(binary.read_bytes()).hexdigest() for side, binary in binaries.items()},
              "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "inputs_sha256": {kind: hashlib.sha256(text.encode()).hexdigest() for kind, text in inputs.items()},
              "host_before": before, "host_after": host(), "summary": summary, "samples": samples,
              "measurement": "fresh process to exit; loader, original regex compilation, detection and output; warm OS page cache; shared host",
              "interpretation": "descriptive cold samples; ZWSP base may leak where candidate protects, so detection work differs"}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"summary": summary, "host_before": before, "host_after": report["host_after"]}))


if __name__ == "__main__":
    main()
