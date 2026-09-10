#!/usr/bin/env python3
"""Frozen local DEV256 only. Diagnostics never replace paired output scoring."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import dataiku_en_de_gaze_bench as dataiku
import gaze_bench_score as score
import run_no_opf_benchmark as runner
from bench_subprocess import BenchSubprocess

IDS_SHA = "3993b14aeded90b056854d29172de148424b6e7b0eb967395e6b78de2fbbce36"
FILE_SHA = "5a37e640fa43c3808a8d5aae2965a5c2e72eb815a0b7363fa82f89e3ff943427"
SOURCE_GOLD_SHA = "2ac74d55ca1612f3247929a7a5e61e654cabc75d25fa96330b0690da13bc5379"
BRIDGE_SHA = "273968f5638cac53c9528b61bc974230811acc700b3bc5742765b60873c6a403"
ARMS = ("pass2-ner", "pass2-ner-redact", "pass2-ner-redact-semantic-candidate")
MODEL_MANIFEST_SHA = "f6d3c6ff73b7070ceba87e1afbbf97c5552b79f7e1fd1e048396ac6d409b2d7d"


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_new(path, value):
    with path.open("x") as handle:
        json.dump(value, handle, indent=2)
        handle.write("\n")


def verify_private_model():
    private = Path.home() / ".local/share/gaze-private/redact-7370"
    manifest = private / "patched-bundle-manifest.json"
    assert digest(manifest) == MODEL_MANIFEST_SHA
    model = private / "model-patched-coreml"
    assert Path(os.environ["GAZE_REDACT_MODEL_DIR"]).resolve() == model
    entries = json.loads(manifest.read_text())
    assert {str(p.relative_to(model)) for p in model.rglob("*") if p.is_file()} == {
        entry["path"] for entry in entries}
    for entry in entries:
        path = model / entry["path"]
        assert path.stat().st_size == entry["size"] and digest(path) == entry["sha256"]
    return MODEL_MANIFEST_SHA


def selected_documents(repo, frozen, dataset):
    # Reuses diagnosis7381 physical-row selection before Python text construction.
    # Arrow reads encoded storage; JSONL streaming transiently decodes other rows.
    import pyarrow.parquet as pq
    ids = json.loads(frozen.read_text())
    assert digest(frozen) == FILE_SHA
    assert len(ids) == len(set(ids)) == 256
    assert score.document_ids_digest(ids)["value"] == IDS_SHA
    selected = set(ids)
    indices = sorted(int(i.removeprefix("dataiku-test-"))
                     for i in ids if i.startswith("dataiku-test-"))
    dataiku.verify_dataset(dataset)
    table = pq.read_table(dataset)
    assert table.num_rows == dataiku.DATASET_ROWS
    rows = table.take(indices).to_pylist()
    del table
    documents = {}
    for index, row in zip(indices, rows):
        uid = f"dataiku-test-{index}"
        text = row["text"]
        offsets = score.char_to_byte_offsets(text)
        spans = []
        for entity in row["privacy_mask"]:
            start, end = entity["start"], entity["end"]
            assert isinstance(start, int) and isinstance(end, int)
            assert 0 <= start < end <= len(text)
            assert text[start:end] == entity["value"]
            spans.append(score.Span(offsets[start], offsets[end], entity["label"]))
        documents[uid] = score.Document(
            uid, text, {"English": "en", "German": "de"}[row["language"]],
            dataiku.COUNTRY_REGIONS.get(row["country"], ""), dataiku.DATASET_REPO,
            tuple(spans))
    negative = repo / runner.NEGATIVE_CORPUS
    with negative.open() as handle:
        for line in handle:
            row = json.loads(line)
            if row["id"] not in selected:
                continue
            assert row["id"] not in documents
            assert row["oracle_spans"] == []
            assert row["language"] in {"en", "de"}
            assert all(isinstance(row[k], str) and row[k]
                       for k in ("id", "category", "text"))
            documents[row["id"]] = score.Document(
                row["id"], row["text"], row["language"], "",
                "gaze-a4-negative-corpus", (), row["category"])
    assert set(documents) == selected
    result = [documents[uid] for uid in ids]
    assert score.output_source_contract(result)["source_gold_sha256"] == SOURCE_GOLD_SHA
    return result


def joined_audit(path, ids):
    """Join full planned request order, including failed requests, without guessing."""
    rows = []
    current = None
    for line in path.open():
        record = json.loads(line)
        status = record.get("status")
        if status == "request_begin":
            assert current is None
            ordinal = record["request"]
            assert ordinal == len(rows) + 1 and ordinal <= len(ids)
            current = {"request": ordinal, "document_id": ids[ordinal - 1],
                       "terminal": "unknown", "records": [record]}
        elif status in {"request_success", "request_refusal", "request_error"}:
            assert current is not None and record["request"] == current["request"]
            current["records"].append(record)
            current["terminal"] = status
            rows.append(current)
            current = None
        elif current is not None:
            # Keep complete normalized batch spans/dispositions, including unknowns.
            current["records"].append(record)
        else:
            raise ValueError("unbound audit record")
    if current is not None:
        rows.append(current)
    for ordinal in range(len(rows) + 1, len(ids) + 1):
        rows.append({"request": ordinal, "document_id": ids[ordinal - 1],
                     "terminal": "unknown", "records": []})
    return {"coordinates": "detector_input_utf8", "diagnostic_only": True,
            "rows": rows}


def run(args):
    os.environ.pop("GAZE_NER_LOCALE", None)
    repo = Path(__file__).resolve().parents[2]
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    binary = repo / "target/debug/examples/clean_for_bench"
    validator = score.validator_probe_binary(repo)
    bridge = Path(os.environ["GAZE_REDACT_BRIDGE"])
    assert digest(bridge) == BRIDGE_SHA
    docs = selected_documents(repo, args.frozen, args.dataset)
    ids = [doc.uid for doc in docs]
    davlan = Path.home() / ".local/share/gaze/models/davlan-mbert-ner-hrl"
    kiji = Path.home() / ".local/share/gaze/models/kiji-distilbert"
    models = runner.validate_required_models(repo, davlan, kiji)
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=repo)
    for name in ("workspace-bootstrap", "producer-build", "validator-build",
                 "python-tests", "runner-tests", "observer-tests"):
        receipt = json.loads((out.parent / (name + ".json")).read_text())
        assert receipt["exit_code"] == 0 and receipt["source_head"] == head
    pins = {"source_head": head, "producer_sha256": digest(binary),
            "validator_sha256": digest(validator), "bridge_sha256": digest(bridge),
            "private_model_manifest_sha256": verify_private_model(),
            "models": models, "ordered_ids": ids, "ordered_ids_sha256": IDS_SHA,
            "source_contract": score.output_source_contract(docs), "arms": ARMS,
            "threshold": 0.3, "redact_threshold": 0.6, "org_enabled": True,
            "warmups": 0, "repetitions": 1,
            "effective_environment": {
                "GAZE_NER_LOCALE": None, "GAZE_NER_THRESHOLD": "0.3",
                "GAZE_NER_MODEL_DIR": str(davlan),
                "GAZE_REDACT_BRIDGE": str(bridge),
                "GAZE_REDACT_MODEL_DIR": os.environ["GAZE_REDACT_MODEL_DIR"],
                "DAL_APP_ID": "gaze-local-redact-primary",
                "DAL_COREML_COMPUTE_UNITS": "all"},
            "admission_audit": str(out / "admission.jsonl")}
    if args.phase == "freeze":
        write_new(out / "freeze.json", pins)
        return 0
    frozen = json.loads((out / "freeze.json").read_text())
    assert json.loads(json.dumps(pins)) == frozen
    environment = dict(os.environ)
    if args.phase == "smoke":
        smoke = score.Document("synthetic-fullwidth-smoke", "😀 plain\n\tＳｃｈｍｉｄｔ  end",
                               "en", "US", "synthetic", (score.Span(12, 33, "LASTNAME"),))
        results = {}
        for arm in ARMS[1:]:
            environment["GAZE_REDACT_ADMISSION_AUDIT_FILE"] = str(out / "smoke-admission.jsonl")
            environment.update(GAZE_NER_MODEL_DIR=str(davlan), GAZE_NER_THRESHOLD="0.3")
            with BenchSubprocess([str(binary), "--config", arm], cwd=repo,
                                 env=environment) as process:
                response = score.validate_response(smoke, process.exchange({
                    "fixture_id": smoke.uid, "locale_chain": smoke.locale_chain,
                    "text": smoke.text}))
            row = score.observe_output(smoke, response)
            trace = response.get("final_protection_trace", [])
            row["actual_redact_exact_raw_interval"] = any(
                item["raw_start"] == 12 and item["raw_end"] == 33
                and any(source.startswith("redact-patched-coreml-v1:")
                        for source in item["provenance"]["source_ids"])
                for item in trace)
            results[arm] = [row]
        write_new(out / "smoke.json", results)
        assert all(rows[0]["outcome"] == "completed_reversible"
                   and rows[0]["surviving_bytes"] == 0
                   and rows[0]["actual_redact_exact_raw_interval"] for rows in results.values())
        return 0
    smoke = json.loads((out / "smoke.json").read_text())
    assert all(rows[0]["outcome"] == "completed_reversible"
               and rows[0]["surviving_bytes"] == 0
               and rows[0]["actual_redact_exact_raw_interval"] for rows in smoke.values())
    environment["GAZE_REDACT_ADMISSION_AUDIT_FILE"] = pins["admission_audit"]
    write_new(out / "dev-started.json", {"source_head": head, "planned": 256})
    measurements = score.collect_validator_measurements(validator, docs, ids)
    try:
        runs, repetitions = runner.execute_measurements(
            repo_root=repo, binary=binary, documents=docs, davlan_model=davlan,
            kiji_model=kiji, threshold=0.3, diagnostics_dir=out / "logs",
            warmup_count=0, measured_repetitions=1, validator_measurements=measurements,
            source_environment=environment, configs=ARMS, output_proof_dir=out / "output-proof-v1")
        write_new(out / "runs.json", {"runs": runs, "repetitions": repetitions})
    finally:
        audit = out / "admission.jsonl"
        if audit.exists():
            write_new(out / "joined-admission.json", joined_audit(audit, ids))
    sidecars = out / "output-proof-v1/repetition-1"
    baseline = json.loads((sidecars / (ARMS[0] + ".json")).read_text())
    passed = []
    for arm in ARMS[1:]:
        candidate = json.loads((sidecars / (arm + ".json")).read_text())
        result = score.compare_output_proofs(candidate, baseline)
        write_new(out / ("comparison-" + arm + ".json"), result)
        passed.append(result["passed"])
    return 0 if all(passed) else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("freeze", "smoke", "dev"))
    for name in ("frozen", "dataset", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    try:
        exit_code = run(parser.parse_args())
    except BaseException as error:
        # Preserve a sanitized failure receipt; no exception text or corpus context.
        print(json.dumps({"error_type": type(error).__name__}), file=sys.stderr)
        exit_code = 2
    sys.exit(exit_code)
