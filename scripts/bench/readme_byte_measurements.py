#!/usr/bin/env python3
"""Capture a tagged release once, then replay byte-micro README measurements.

Observation archives and native diagnostics stay private. Published receipts
contain only counts, input digests, commands and release/build identities.
Run datasets serially while holding the machine's benchmark lease.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
from pathlib import Path

import gaze_bench_score as score
import scorecard_record as records
import run_no_opf_benchmark as runner
import dataiku_en_de_gaze_bench as dataiku
import openpii_gaze_bench as openpii
from tagged_gaze import tag_commit

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).parent / "compare/theirbench"))
import loaders  # noqa: E402

SEALED_PINS = {
    "openpii_validation.jsonl": openpii.DATASET_SHA256,
    "gretel_English.parquet": "c02b06d3c5b7c375525136d6f74acc52ab8fc07fa863d0aa50734910ec0ef2ad",
    "gretel_German.parquet": "bfc64380bc24453f3460ffee6afbd7a3442b60dd26d5341c9fe0f1cca43216c3",
    "gretel_France.parquet": "39fc2777822bbecab2d89bb23cbed40d9825ff4117897b8b025c02287f4ba845",
}


def digest(path: Path) -> str:
    return score.sha256_file(path)


def load_sets(kind: str, path: Path) -> tuple[dict, dict]:
    if kind == "own":
        dataiku.verify_dataset(path)
        positive, _ = dataiku.load_documents(path)
        negative, _ = runner.load_negative_documents(ROOT / runner.NEGATIVE_CORPUS)
        return {"holdout": positive + negative}, {
            "dataiku": digest(path), "negative_corpus": digest(ROOT / runner.NEGATIVE_CORPUS),
        }
    if kind == "presidio-research":
        docs, identity = loaders.load_presidio_research(path)
        return {"test": docs}, {"dataset": identity["sha256"]}
    if kind == "piibench-commercial":
        sets, identity = loaders.load_piibench_commercial(path)
        expected = json.loads((ROOT / "docs/reference/benchmarks/their-benchmarks.json").read_text())
        pin = expected["benchmarks"][kind]["identity"]["test_5k_sha256"]
        if identity["test_5k_sha256"] != pin:
            raise ValueError("commercial input differs from the published dataset")
        return sets, {"dataset": pin, "manifest": digest(path / "manifest.json")}
    if kind == "pii-trace":
        sets, identity = loaders.load_pii_trace(path)
        return sets, {"dataset": identity["sha256"]}
    import pyarrow.parquet as parquet
    for name, pin in SEALED_PINS.items():
        if digest(path / name) != pin:
            raise ValueError("sealed input digest differs")
    docs, _ = openpii.load_dataset(path / "openpii_validation.jsonl", None)
    sets = {"openpii/all": docs}
    for name, language, region in (("English", "en", "US"), ("German", "de", "DE"), ("France", "fr", "FR")):
        documents = []
        for index, row in enumerate(parquet.read_table(path / f"gretel_{name}.parquet").to_pylist()):
            text = row["generated_text"]
            offsets = score.char_to_byte_offsets(text)
            spans = tuple(score.Span(offsets[s["start"]], offsets[s["end"]], s["label"])
                          for s in json.loads(row["pii_spans"])
                          if 0 <= s["start"] < s["end"] <= len(text))
            documents.append(score.Document(uid=f"gretel-{language}-{index}", text=text,
                                           language=language, region=region,
                                           source_dataset="gretel-finance", spans=spans))
        sets[f"gretel/{language}"] = documents
    return sets, SEALED_PINS.copy()


def summarize(run: dict) -> dict:
    byte = run["metrics"]["utf8_bytes"]
    availability = run["pipeline_availability"]
    contract = run["pipeline_contract"]
    return {
        "documents": availability["attempted_documents"],
        "processed": availability["completed_documents"],
        "refused": availability["failed_closed_documents"],
        "gold_bytes": byte["pii"], "leaked_bytes": byte["leaked"],
        "false_positive_bytes": byte["false_positive"],
        "restore_exact": contract["restore_exact_documents"],
        "manifest_valid": contract["manifest_valid_documents"],
    }


def totals(sets: dict) -> dict:
    result = {key: sum(row[key] for row in sets.values()) for key in next(iter(sets.values()))}
    tp = result["gold_bytes"] - result["leaked_bytes"]
    denominator = 5 * tp + 4 * result["leaked_bytes"] + result["false_positive_bytes"]
    result["byte_micro_f2"] = 5 * tp / denominator if denominator else 0.0
    return result


def replay(path: Path, contract: score.ScoredLabelContract) -> dict:
    header, observations = records._read(path)
    inventory = {row["id"]: row for row in header["documents"]}
    # Reuse the established record replay without its validator-census stage:
    # these report-only external datasets have no checksum census.
    _, run = records._replay_group((header, header["scorecard"], inventory, observations,
                                   "C", 0, "policy-file", contract))
    return summarize(run)


def measure(args) -> None:
    root = args.runtime_root.resolve()
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    if revision != tag_commit(args.tag, ROOT):
        raise ValueError("runtime checkout is not the release tag")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=root):
        raise ValueError("runtime checkout is dirty")
    subprocess.run(["git", "tag", "-v", args.tag], cwd=ROOT, check=True, capture_output=True)
    build = json.loads(args.build_receipts.read_text())["v0" + args.tag.split(".")[1]]
    if (build["revision"] != revision or build["binary_sha256"] != digest(args.binary)
            or build["policy_sha256"] != digest(args.policy)):
        raise ValueError("build receipt differs from the measured inputs")
    args.output.mkdir(parents=True, exist_ok=False)
    sets, inputs = load_sets(args.dataset, args.input)
    all_gold = score.SCORED_LABEL_CONTRACT_V1
    contract = runner.load_scored_label_contract(ROOT, Path("docs/reference/benchmarks/scored-labels-v4.json")) if args.dataset == "own" else all_gold
    rows, references = {}, {}
    for split, documents in sets.items():
        corpus = hashlib.sha256(json.dumps({"inputs": inputs, "split": split}, sort_keys=True).encode()).hexdigest()
        writer = records.RecordWriter(documents, {"documents": {doc.uid: None for doc in documents}}, corpus_sha256=corpus)
        run = score.run_config(root, args.binary.resolve(), "policy-file", documents,
                               args.model_dir, None, None, None, 0.3,
                               args.output / "diagnostics" / split,
                               base_environment=runner.build_no_opf_environment(os.environ),
                               policy_path=args.policy.resolve(),
                               record_document=lambda config, doc, response, measurements:
                               writer.add("C", config, doc, response, measurements))
        card = {"runs": [run], "dataset": {"integrity": {"sha256": corpus}},
                "parameters": {"ner_threshold": 0.3}, "scoring": {}}
        path = args.output / (split.replace("/", "-") + ".jsonl.gz")
        references[split] = writer.write(path, card, add_reference=False)
        rows[split] = replay(path, contract)
        if args.dataset != "own" and rows[split] != summarize(run):
            raise ValueError("capture and replay counters differ")
        print(f"{args.tag} {args.dataset} {split}: recorded and replayed", flush=True)
    receipt = {
        "schema_version": 1, "tag": args.tag, "revision": revision,
        "dataset": args.dataset, "contract": "v4" if args.dataset == "own" else "all-gold",
        "binary_sha256": digest(args.binary), "policy_sha256": digest(args.policy),
        "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "driver_sha256": digest(Path(__file__)), "inputs_sha256": inputs,
        "hardware": {"platform": platform.platform(), "cpu": platform.processor(), "cores": os.cpu_count()},
        "model_manifests_sha256": {name: digest(path / "SHA256SUMS") for name, path in
                                   (("ner", args.model_dir), ("nym", Path.home() / ".local/share/gaze/models/nym-small-int8"))},
        "command": ["uv", "run", "--project", "scripts/bench", "--python", "3.13", "python",
                    "scripts/bench/readme_byte_measurements.py", "--tag", args.tag,
                    "--dataset", args.dataset, "--runtime-root", "<tag-checkout>",
                    "--binary", "<tag-checkout>/target/release/examples/clean_for_bench",
                    "--policy", "<setup-policy>", "--input", "<pinned-dataset>",
                    "--output", "<private-record-directory>", "--build-receipts", "<build-receipts>"],
        "build_commands": [[part.replace(str(root), "<tag-checkout>")
                             .replace(str(args.policy), "<setup-policy>") for part in command]
                           for command in build["commands"]],
        "toolchain": "1.96.0", "cargo_build_jobs": 4,
        "records": references, "sets": rows, "totals": totals(rows),
    }
    (args.output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--runtime-root", type=Path, required=True)
    parser.add_argument("--tag", choices=("v0.16.0", "v0.17.0"), required=True)
    parser.add_argument("--build-receipts", type=Path, required=True)
    for name in ("binary", "policy", "input", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--dataset", choices=("own", "sealed", "presidio-research", "piibench-commercial", "pii-trace"), required=True)
    parser.add_argument("--model-dir", type=Path, default=Path.home() / ".local/share/gaze/models/davlan-mbert-ner-hrl")
    measure(parser.parse_args())


if __name__ == "__main__":
    main()
