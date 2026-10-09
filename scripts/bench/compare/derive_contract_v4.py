#!/usr/bin/env python3
"""Derive immutable comparison-contract v4 aggregates without replaying predictions.

Contract v4 has v1's scored-label and prediction-class scope plus v3's
gold-gap semantics. Historical native predictions are unavailable, so this
migration copies each v1 aggregate and applies the already-measured v3
gold-gap credit. ``--write`` additionally audits credential repeats in the
pinned corpus and replays only those documents through the historical OPF,
proving v4 introduces no credential-specific gold-gap credit.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent
REPO = BENCH.parents[1]
sys.path.insert(0, str(BENCH))
sys.path.insert(0, str(HERE))

import agentic_layers as agentic  # noqa: E402
import compare  # noqa: E402
import gaze_bench_score as score  # noqa: E402

REPORT = REPO / "docs/reference/benchmarks/comparison.json"
TUNED = REPO / "docs/reference/benchmarks/presidio-tuned.json"
PACK_DIR = REPO / "docs/reference/benchmarks/variant-packs"
CREDENTIAL_LABELS = frozenset({"PASSWORD", "SECURITYTOKEN"})


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sync_contract_hashes(report: dict) -> None:
    """Record the current immutable contract files, including metadata-only edits."""
    for version, path in compare.CONTRACTS.items():
        report["contracts"][version] = digest(REPO / path) if path is not None else None


def derive_cell(v1: dict, v3: dict) -> dict:
    """v4 is v1 accounting plus the pre-existing non-credential v3 credit."""
    result = copy.deepcopy(v1)
    if "gold_gap_protected_bytes" in result:
        credit = v3["gold_gap_protected_bytes"]
        raw = result["false_positive_bytes"]
        require(isinstance(credit, int) and 0 <= credit <= raw, "invalid v3 gold-gap credit")
        result["gold_gap_protected_bytes"] = credit
        result["false_positive_bytes_after_gold_gap"] = raw - credit
    return result


def add_v4_contracts(value: object) -> None:
    """Add v4 beside every measured v1/v2/v3 contract table in the report."""
    if isinstance(value, dict):
        if {"v1", "v2", "v3"}.issubset(value) and isinstance(value["v1"], dict):
            value["v4"] = derive_cell(value["v1"], value["v3"])
        for child in list(value.values()):
            add_v4_contracts(child)
    elif isinstance(value, list):
        for child in value:
            add_v4_contracts(child)


def derive_tuned(report: dict) -> dict:
    """Derive tuned-Presidio v4 cells; its mapping emits no credential class."""
    report = copy.deepcopy(report)
    for name, row in report["rows"].items():
        for layer in row["contracts"]["v1"]:
            v1 = row["contracts"]["v1"][layer]
            v3 = row["contracts"]["v3"][layer]
            require(
                v1["false_positive_bytes"] == v3["false_positive_bytes"],
                f"{name}/{layer}: credential-neutral predictions prevent aggregate derivation",
            )
    add_v4_contracts(report)
    sync_contract_hashes(report)
    report["comparison_sha256"] = digest(REPORT)
    report["compare_sha256"] = digest(Path(compare.__file__))
    report["scorer_sha256"] = digest(BENCH / "gaze_bench_score.py")
    report["contract_v4_derivation"] = {
        "method": "v1 aggregates plus v3 gold-gap credit",
        "prediction_replay": False,
        "credential_prediction_classes": False,
    }
    return report


def outside_gold_repeats(documents: list[score.Document]) -> list[dict[str, object]]:
    """Credential gold values repeated on a word boundary outside all gold spans."""
    findings: list[dict[str, object]] = []
    for document in documents:
        text = document.text.encode("utf-8")
        blocked = score.merge_intervals((span.start, span.end) for span in document.spans)
        for span in document.spans:
            if span.label not in CREDENTIAL_LABELS:
                continue
            needle = text[span.start:span.end]
            start = 0
            while True:
                start = text.find(needle, start)
                if start < 0:
                    break
                end = start + len(needle)
                if not score.interval_overlaps((start, end), blocked) and score.gold_gap_on_word_boundary(text, start, end):
                    findings.append({"document_id": document.uid, "label": span.label, "start": start, "end": end})
                start += max(1, len(needle))
    return findings


def audit_opf_repeat_credit(
    documents: list[score.Document], findings: list[dict[str, object]],
    python: Path, checkpoint: Path,
) -> dict[str, object]:
    """Replay only documents with credential repeats through the historical OPF."""
    expected_checkpoint = "7962be95ebe85491799c225cadaed7d1ac5aef82b530d4d3f02d0ec7e14239e5"
    require(compare.digest_tree(checkpoint) == expected_checkpoint, "OPF checkpoint differs from comparison provenance")
    source = Path(subprocess.check_output(
        [str(python), "-c", "import pathlib, opf; print(pathlib.Path(opf.__file__).resolve().parents[1])"],
        text=True,
    ).strip())
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip()
    require(revision == "f7f00ca7fb869683eb732c010299d901457f19c3", "OPF source differs from comparison provenance")
    wanted = {item["document_id"] for item in findings}
    selected = [document for document in documents if document.uid in wanted]
    contract = compare.runner.load_scored_label_contract(REPO, compare.CONTRACTS["v4"])
    applied = {document.uid: document for document in score.apply_scored_label_contract(selected, contract)}
    backend = compare.Opf(python, checkpoint, Path(tempfile.mkdtemp(prefix="gaze-v4-opf-")))
    evidence = []
    try:
        for document in selected:
            predictions = backend.predict(document)
            mapped = compare.mapped_document(applied[document.uid], compare.load_mapping()["opf"])
            gold, ignored, _ = score.contract_scoring_view(mapped, predictions)
            credits = score.gold_gap_credits(mapped, predictions, gold, ignored)
            credential_credits = [credit for credit in credits if credit.label in CREDENTIAL_LABELS]
            require(not credential_credits, f"OPF adds credential gold-gap credit: {credential_credits}")
            for item in (row for row in findings if row["document_id"] == document.uid):
                start, end = item["start"], item["end"]
                evidence.append({
                    **item,
                    "opf_overlaps": [
                        {"start": span.start, "end": span.end, "label": span.label}
                        for span in predictions
                        if score.interval_overlaps((span.start, span.end), [(start, end)])
                    ],
                })
    finally:
        backend.close()
    return {
        "checkpoint_sha256": expected_checkpoint,
        "source_revision": revision,
        "credential_gold_gap_protected_bytes": 0,
        "repeat_ranges": evidence,
    }


def comparison_documents(dataset: Path, report: dict) -> list[score.Document]:
    positives, _ = compare.dataiku.load_documents(dataset)
    negatives, _ = compare.runner.load_negative_documents(
        REPO / "crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl"
    )
    main, _ = score.stratified_sample(positives + negatives, None, score.DEFAULT_SAMPLE_SEED)
    version = report["corpus"]["agentic"]["generator_version"]
    generated = [record.to_document() for record in agentic.records_as_of(version, agentic.generate(agentic.PUBLISHED_PARTITION))]
    packs: list[score.Document] = []
    for path in sorted(PACK_DIR.glob("*.jsonl")):
        by_layer, _ = compare.load_pack(path)
        packs.extend(document for documents in by_layer.values() for document in documents)
    documents = [*main, *generated, *packs]
    expected = sum(layer["documents"] for layer in report["corpus"]["layers"].values())
    require(len(documents) == expected, f"rebuilt {len(documents)} documents, expected {expected}")
    return documents


def derive(
    report: dict, *, dataset: Path | None,
    opf_python: Path | None = None, opf_checkpoint: Path | None = None,
) -> dict:
    original = copy.deepcopy(report)
    report = copy.deepcopy(report)
    receipt = report.get("contract_v4_derivation")
    if dataset is not None:
        documents = comparison_documents(dataset, original)
        findings = outside_gold_repeats(documents)
        require(opf_python is not None and opf_checkpoint is not None, "credential repeats require OPF replay arguments")
        opf = audit_opf_repeat_credit(documents, findings, opf_python, opf_checkpoint)
        receipt = {
            "method": "v1 aggregates plus v3 gold-gap credit",
            "prediction_replay": False,
            "credential_gold_repeats_outside_gold": len(findings),
            "main_dataset_sha256": compare.dataiku.DATASET_SHA256,
            "agentic_corpus_sha256": original["corpus"]["agentic"]["corpus_sha256"],
            "variant_packs": original["corpus"]["packs"],
            "targeted_opf_replay": opf,
        }
    require(isinstance(receipt, dict), "comparison lacks contract_v4_derivation receipt; run --write with --dataset")
    require(receipt.get("targeted_opf_replay", {}).get("credential_gold_gap_protected_bytes") == 0,
            "credential repeat audit did not establish zero additional credit")
    require(receipt.get("main_dataset_sha256") == compare.dataiku.DATASET_SHA256, "v4 derivation dataset changed")
    require(receipt.get("agentic_corpus_sha256") == original["corpus"]["agentic"]["corpus_sha256"], "v4 derivation agentic corpus changed")
    require(receipt.get("variant_packs") == original["corpus"]["packs"], "v4 derivation variant packs changed")
    add_v4_contracts(report)
    sync_contract_hashes(report)
    report["compare_sha256"] = digest(Path(compare.__file__))
    report["scorer_sha256"] = digest(BENCH / "gaze_bench_score.py")
    report["contract_v4_derivation"] = receipt
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--write", action="store_true")
    action.add_argument("--check", action="store_true")
    parser.add_argument("--dataset", type=Path)
    parser.add_argument("--opf-python", type=Path)
    parser.add_argument("--opf-checkpoint", type=Path)
    args = parser.parse_args()
    if args.write and args.dataset is None:
        parser.error("--write requires --dataset for the credential-repeat audit")
    if args.write and (args.opf_python is None or args.opf_checkpoint is None):
        parser.error("--write requires --opf-python and --opf-checkpoint for targeted repeat replay")
    current = json.loads(REPORT.read_text(encoding="utf-8"))
    result = derive(
        current, dataset=args.dataset,
        opf_python=args.opf_python, opf_checkpoint=args.opf_checkpoint,
    )
    if args.write:
        REPORT.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    else:
        require(current == result, "comparison v4 aggregates need --write")
    tuned = json.loads(TUNED.read_text(encoding="utf-8"))
    tuned_result = derive_tuned(tuned)
    if args.write:
        TUNED.write_text(json.dumps(tuned_result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    else:
        require(tuned == tuned_result, "tuned Presidio v4 aggregates need --write")
    print("Contract v4 derivation audited; only credential-repeat documents were replayed through OPF.")


if __name__ == "__main__":
    main()
