"""Retain aggregate correctness counters around an unchanged external driver.

The driver retains custody of its inputs and native predictions. This wrapper
reads only fixed integer counters from run_config and file/revision receipts.
Driver stdout/stderr (including child processes) is discarded; errors expose
one fixed code. The sidecar cannot contain document identifiers or details.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import sys

COUNTERS = frozenset({
    "attempted_documents", "completed_documents", "failed_closed_documents",
    "contract_documents", "restore_exact_documents", "restore_failures",
    "restore_success_decisions", "restore_decision_failures",
    "manifest_valid_documents", "manifest_invalid_documents",
})
RECEIPTS = frozenset({"driver_sha256", "wrapper_sha256", "binary_sha256",
                      "policy_sha256", "harness_revision", "runtime_revision",
                      "ner_manifest_sha256", "nym_manifest_sha256", "inputs_sha256",
                      "driver_output_sha256"})
SETS = {
    "sealed": ("openpii/all", "openpii/en-de-fr", "gretel/en", "gretel/de", "gretel/fr"),
    "piibench-commercial": ("test",),
    "pii-trace": ("test",),
}
INPUTS = {
    "sealed": ("openpii_validation.jsonl", "gretel_English.parquet", "gretel_German.parquet", "gretel_France.parquet"),
    "piibench-commercial": ("manifest.json", "data/test_5k.jsonl", "data/val.jsonl", "data/test.jsonl", "data/label_mapping.json"),
    "pii-trace": ("train.parquet",),
}

def count(value):
    if type(value) is not int or value < 0:
        raise ValueError("invalid aggregate count")
    return value

def quality_counts(run):
    # Deliberately never iterate the run or inspect error telemetry/populations.
    availability = run["pipeline_availability"]
    contract = run["pipeline_contract"]
    result = {name: count(availability[name]) for name in (
        "attempted_documents", "completed_documents", "failed_closed_documents")}
    result["contract_documents"] = count(contract["documents"])
    for name in ("restore_exact_documents", "restore_success_decisions", "manifest_valid_documents"):
        result[name] = count(contract[name])
    for target, source in (("restore_failures", "restore_exact_documents"),
                           ("restore_decision_failures", "restore_success_decisions"),
                           ("manifest_invalid_documents", "manifest_valid_documents")):
        result[target] = count(result["contract_documents"] - result[source])
    validate_counts(result)
    return result

def validate_counts(row):
    if type(row) is not dict or set(row) != COUNTERS:
        raise ValueError("invalid aggregate schema")
    for value in row.values(): count(value)
    if row["completed_documents"] + row["failed_closed_documents"] != row["attempted_documents"]:
        raise ValueError("inconsistent aggregate counts")
    if row["contract_documents"] != row["completed_documents"]:
        raise ValueError("inconsistent aggregate counts")
    for passed, failed in (("restore_exact_documents", "restore_failures"),
                           ("restore_success_decisions", "restore_decision_failures"),
                           ("manifest_valid_documents", "manifest_invalid_documents")):
        if row[passed] + row[failed] != row["contract_documents"]:
            raise ValueError("inconsistent aggregate counts")

def validate_receipt(value):
    if type(value) is not dict or set(value) != {"schema_version", "benchmark", "receipts", "quality"}:
        raise ValueError("invalid receipt schema")
    if type(value["schema_version"]) is not int or value["schema_version"] != 1 or value["benchmark"] not in SETS:
        raise ValueError("invalid receipt schema")
    kind = value["benchmark"]
    receipts = value["receipts"]
    if type(receipts) is not dict or set(receipts) != RECEIPTS:
        raise ValueError("invalid receipt schema")
    for name, digest in receipts.items():
        if name == "inputs_sha256":
            if type(digest) is not dict or set(digest) != set(INPUTS[kind]):
                raise ValueError("invalid input receipt schema")
            digests = digest.values()
        else:
            digests = (digest,)
        for item in digests:
            width = 40 if name in {"runtime_revision", "harness_revision"} else 64
            if type(item) is not str or not re.fullmatch("[0-9a-f]{" + str(width) + "}", item):
                raise ValueError("invalid digest receipt")
    if type(value["quality"]) is not dict or set(value["quality"]) != set(SETS[kind]):
        raise ValueError("invalid aggregate set schema")
    for row in value["quality"].values(): validate_counts(row)

def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1048576), b""): digest.update(block)
    return digest.hexdigest()

def revision(root):
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=root):
        raise ValueError("dirty source")
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()

@contextmanager
def discard_driver_output():
    sys.stdout.flush(); sys.stderr.flush()
    saved = [os.dup(fd) for fd in (1, 2)]
    try:
        with open(os.devnull, "w") as sink:
            for fd in (1, 2): os.dup2(sink.fileno(), fd)
            yield
    finally:
        # Flush while still directed to /dev/null, including exceptional exits.
        sys.stdout.flush(); sys.stderr.flush()
        for fd, original in zip((1, 2), saved):
            os.dup2(original, fd); os.close(original)

def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--benchmark", choices=SETS, required=True)
    for name in ("driver", "harness-root", "runtime-root", "binary", "policy", "model-dir", "nym-model-dir", "data", "driver-output", "quality-output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--driver-sha256", required=True)
    parser.add_argument("--predictions-dir", type=Path)
    args = parser.parse_args()
    try:
        if sha256(args.driver) != args.driver_sha256: raise ValueError("driver receipt differs")
        if args.driver_output.exists() or args.quality_output.exists(): raise ValueError("fresh outputs required")
        receipts = {
            "driver_sha256": sha256(args.driver), "wrapper_sha256": sha256(__file__),
            "binary_sha256": sha256(args.binary), "policy_sha256": sha256(args.policy),
            "harness_revision": revision(args.harness_root), "runtime_revision": revision(args.runtime_root),
            "ner_manifest_sha256": sha256(args.model_dir / "SHA256SUMS"),
            "nym_manifest_sha256": sha256(args.nym_model_dir / "SHA256SUMS"),
            "inputs_sha256": {name: sha256(args.data / name) for name in INPUTS[args.benchmark]},
        }
        sys.path.insert(0, str(args.harness_root / "scripts/bench"))
        import gaze_bench_score as score
        original = score.run_config
        quality = []
        def measured(*positional, **keywords):
            run = original(*positional, **keywords)
            quality.append(quality_counts(run))
            return run
        score.run_config = measured
        if args.benchmark == "sealed":
            driver_args = ["--gaze", args.harness_root, "--binary", args.binary, "--policy", args.policy, "--data", args.data, "--model-dir", args.model_dir, "--out", args.driver_output]
        else:
            if args.predictions_dir is None: raise ValueError("private predictions directory required")
            driver_args = ["--benchmark", args.benchmark, "--tool", "gaze-full", "--gaze-binary", args.binary, "--gaze-policy", args.policy, "--gaze-model-dir", args.model_dir, "--predictions-dir", args.predictions_dir, "--output", args.driver_output]
            driver_args += ["--piibench-data", args.data] if args.benchmark == "piibench-commercial" else ["--pii-trace-data", args.data / "train.parquet"]
        saved_argv = sys.argv
        try:
            sys.argv = [str(args.driver), *map(str, driver_args)]
            with discard_driver_output():
                try: runpy.run_path(str(args.driver), run_name="__main__")
                except SystemExit as error:
                    if error.code not in (None, 0): raise RuntimeError("driver failed") from None
        finally:
            sys.argv = saved_argv
            score.run_config = original
        if len(quality) != len(SETS[args.benchmark]): raise ValueError("aggregate coverage differs")
        receipts["driver_output_sha256"] = sha256(args.driver_output)
        output = {"schema_version": 1, "benchmark": args.benchmark, "receipts": receipts,
                  "quality": dict(zip(SETS[args.benchmark], quality))}
        validate_receipt(output)
        args.quality_output.write_text(json.dumps(output, indent=2) + "\n")
    except BaseException:
        print("EXTERNAL_MEASUREMENT_FAILED", file=sys.stderr)
        return 1
    print("EXTERNAL_AGGREGATE_RECEIPT_OK")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
