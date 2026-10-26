#!/usr/bin/env python3
"""Diagnostic debug CLI timings with checked work counts and exact restore."""
import argparse
import json
from pathlib import Path
import statistics
import subprocess
import time


def invoke(binary, command, payload):
    start = time.perf_counter()
    result = subprocess.run([str(binary), command], input=payload, capture_output=True, text=True)
    seconds = time.perf_counter() - start
    if result.returncode:
        raise SystemExit(f"{command} failed ({result.returncode}): {result.stderr}")
    return json.loads(result.stdout), seconds


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--samples", type=int, default=1)
    args = parser.parse_args()
    if args.samples < 1:
        parser.error("--samples must be positive")
    binary = args.binary.resolve()
    for repeat in [False, True]:
        for count in [500, 1000, 2000, 4000]:
            records = [{"customerId": f"C-{i:06}", "n": i} for i in range(count)]
            if repeat:
                for record in records:
                    record["note"] = "ref " + record["customerId"]
            original = json.dumps(records, separators=(",", ":"))
            durations = []
            for _ in range(args.samples):
                reply, seconds = invoke(binary, "clean", original)
                clean = reply["clean_text"]
                parsed = json.loads(clean)
                assert len(parsed) == count
                assert len(reply["entries"]) == count
                for i, record in enumerate(parsed):
                    assert record.keys() == records[i].keys()
                    assert record["n"] == i
                    assert record["customerId"] != records[i]["customerId"]
                    if repeat:
                        assert record["note"] == "ref " + record["customerId"]
                restored, _ = invoke(binary, "restore", json.dumps({"text": clean, "session_blob": reply["session_blob"]}))
                assert restored["text"] == original
                durations.append(seconds)
            print(json.dumps(dict(records=count, repeat=repeat, bytes=len(original.encode()), samples=args.samples,
                                  median_seconds=statistics.median(durations), min_seconds=min(durations), max_seconds=max(durations),
                                  entries=count, exact_restore=True, errors=0)), flush=True)


if __name__ == "__main__":
    main()
