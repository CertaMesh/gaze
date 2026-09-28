#!/usr/bin/env python3
"""Separate oracle arm: how much does a caller-known record help Gaze?

The record is derived from held-out gold before detection. This assumes an
adopter already knows those values and must never be reported as headline
context-free performance. Records stay in memory; output contains counts only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import tomllib
from collections import Counter
from pathlib import Path
from unittest.mock import patch

import agentic_layers as agentic
import dataiku_en_de_gaze_bench as dataiku
import gaze_bench_score as score
import run_no_opf_benchmark as benchmark


LABEL_CLASS = {
    "EMAIL": "Email",
    "GIVENNAME": "Name",
    "FIRSTNAME": "Name",
    "SURNAME": "Name",
    "PHONENUMBER": "custom:phone",
    "TELEPHONENUM": "custom:phone",
    "IBAN": "custom:iban",
    "ACCOUNTNUM": "custom:iban",
    "CREDITCARDNUMBER": "custom:credit_card",
    "PASSPORTID": "custom:passport",
    "PASSPORTNUM": "custom:passport",
    "NATIONALID": "custom:national_id",
    "IDCARDNUM": "custom:national_id",
    "TAXNUM": "custom:steuer_id",
    "STREET": "Location",
    "CITY": "Location",
    "BUILDINGNUM": "Location",
}
MAX_FIELDS = 32
MAX_VALUE_BYTES = 256


def run_with_record_context(
    contexts: dict[str, str | None], **kwargs: object
) -> dict[str, object]:
    """Inject oracle context at the transport boundary, leaving the scorer intact."""
    base_transport = score.BenchSubprocess

    class RecordTransport(base_transport):
        def exchange(self, request: dict[str, object]) -> dict[str, object]:
            enriched = dict(request)
            context = contexts[request["fixture_id"]]
            if context is not None:
                enriched["context_json"] = context
            return super().exchange(enriched)

    with patch.object(score, "BenchSubprocess", RecordTransport):
        return score.run_config(**kwargs)


def class_action(policy: dict, class_name: str) -> str | None:
    for rule in policy.get("rule", []):
        if rule.get("kind") == "class" and str(rule.get("class", "")).lower() == class_name.lower():
            return rule.get("action")
        if rule.get("kind") == "default":
            return rule.get("action")
    return None


def record_for_document(document: score.Document, policy: dict) -> tuple[str | None, Counter[str]]:
    encoded = document.text.encode("utf-8")
    fields: dict[str, str] = {}
    mapping: dict[str, str] = {}
    eligible: Counter[str] = Counter()
    for span in document.spans:
        class_name = LABEL_CLASS.get(span.label)
        if class_name is None:
            continue
        if class_action(policy, class_name) not in {"tokenize", "format_preserve"}:
            raise ValueError(f"policy has no reversible action for selected class {class_name}")
        value = encoded[span.start : span.end].decode("utf-8")
        if not value.strip() or len(value.encode("utf-8")) > MAX_VALUE_BYTES:
            continue
        if len(fields) >= MAX_FIELDS:
            break
        key = f"v{len(fields):02d}"
        fields[key] = value
        mapping[f"/{key}"] = class_name
        eligible[span.label] += span.end - span.start
    if not fields:
        return None, eligible
    return json.dumps({"record": fields, "field_map": mapping}, ensure_ascii=False), eligible


def paired_records(
    documents: list[score.Document], policy: dict,
    fallback_by_language: dict[str, list[str]] | None = None,
) -> tuple[dict[str, str | None], Counter[str]]:
    contexts: dict[str, str | None] = {}
    eligible: Counter[str] = Counter()
    positives_by_language: dict[str, list[str]] = {}
    for document in documents:
        context, bytes_by_label = record_for_document(document, policy)
        contexts[document.uid] = context
        eligible.update(bytes_by_label)
        if context is not None and document.spans:
            positives_by_language.setdefault(document.language, []).append(context)
    counters: Counter[str] = Counter()
    for document in documents:
        if contexts[document.uid] is not None:
            continue
        pool = positives_by_language.get(document.language, []) or (
            fallback_by_language or {}
        ).get(document.language, [])
        if document.negative_category is not None and pool:
            index = sum(document.uid.encode("utf-8")) % len(pool)
            contexts[document.uid] = pool[index]
            counters[document.negative_category] += 1
    return contexts, eligible


def explicit_counterweights(
    known_pool: dict[str, list[str]],
) -> tuple[list[score.Document], dict[str, str]]:
    """Price collisions and nearby benign text; include email/name variant positives."""
    documents: list[score.Document] = []
    contexts: dict[str, str] = {}
    for language, pool in sorted(known_pool.items()):
        counts: Counter[str] = Counter()

        def add(kind: str, text: str, context: str, label: str | None = None, value: str = "") -> None:
            if counts[kind] >= 16:
                return
            uid = f"known-record-{language}-{kind}-{counts[kind]:02d}"
            if label == "NAME_ORDER":
                surname, first_name = value.split()
                start = text.encode("utf-8").index(value.encode("utf-8"))
                surname_end = start + len(surname.encode("utf-8"))
                first_start = surname_end + 1
                spans = (
                    score.Span(start, surname_end, "SURNAME"),
                    score.Span(first_start, first_start + len(first_name.encode("utf-8")), "FIRSTNAME"),
                )
            elif label:
                start = text.encode("utf-8").index(value.encode("utf-8"))
                spans = (score.Span(start, start + len(value.encode("utf-8")), label),)
            else:
                spans = ()
            documents.append(
                score.Document(
                    uid=uid,
                    text=text,
                    language=language,
                    region="",
                    source_dataset="known-record-oracle-counterweight",
                    spans=spans,
                    negative_category=None if label else f"record_{kind}",
                    cell=f"D|record_{kind}|synthetic|{'positive' if label else 'benign'}",
                )
            )
            contexts[uid] = context
            counts[kind] += 1

        for context in pool:
            parsed = json.loads(context)
            for key, value in parsed["record"].items():
                class_name = parsed["field_map"][f"/{key}"]
                if class_name == "Name":
                    add("homonym", f"The fictional product is called {value}.", context)
                    parts = value.split()
                    if len(parts) == 2:
                        add("surname", f"The unrelated author surname is {parts[-1]}.", context)
                        if all(part.isalpha() and len(part) >= 3 for part in parts):
                            reversed_name = f"{parts[-1]} {parts[0]}"
                            add("name_order", f"Contact: {reversed_name}.", context, "NAME_ORDER", reversed_name)
                elif class_name == "Email" and value.isascii() and "@" in value:
                    mixed = value.swapcase()
                    if mixed != value:
                        add("mixed_case_email", f"Contact: {mixed}.", context, "EMAIL", mixed)
                elif class_name in {"custom:iban", "custom:credit_card", "custom:phone"}:
                    add("reference", f"The synthetic catalog reference is {value}.", context)
                    digit = next((i for i in range(len(value) - 1, -1, -1) if value[i].isdigit()), None)
                    if digit is not None:
                        nearby = value[:digit] + str((int(value[digit]) + 1) % 10) + value[digit + 1 :]
                        add("nearby_digits", f"The unrelated catalog reference is {nearby}.", context)
                elif class_name == "Location":
                    part = next((word for word in value.split() if len(word) >= 4), None)
                    if part:
                        add("partial_address", f"The film title contains {part}.", context)
                if class_name.startswith("custom:"):
                    letter = next((i for i, char in enumerate(value) if char.isascii() and char.isalpha()), None)
                    if letter is not None:
                        near = value[:letter] + ("Z" if value[letter] != "Z" else "Y") + value[letter + 1 :]
                        add("ocr_near_miss", f"The unrelated label reads {near}.", context)
        if counts["name_order"] == 0:
            # A fixed synthetic fixture exercises the approved two-word order
            # variant when the held-out gold contains no suitable full name.
            synthetic = json.dumps({
                "record": {"name": "Alice Smith"},
                "field_map": {"/name": "Name"},
            })
            add("name_order", "Contact: Smith Alice.", synthetic, "NAME_ORDER", "Smith Alice")
            add("surname", "The unrelated author surname is Smith.", synthetic)
    return documents, contexts


def eligible_leak_counter(contexts: dict[str, str | None]) -> tuple[Counter[str], object]:
    leaked: Counter[str] = Counter()

    def record_document(
        _config: str, document: score.Document, response: dict, _validators: object
    ) -> None:
        if "pipeline_error_code" in response:
            return
        context = contexts[document.uid]
        if context is None:
            return
        values = set(json.loads(context)["record"].values())
        predictions = score.final_trace_predictions(document, response)
        protected = score.merge_intervals((span.start, span.end) for span in predictions)
        encoded = document.text.encode("utf-8")
        for span in document.spans:
            if span.label not in LABEL_CLASS:
                continue
            if encoded[span.start : span.end].decode("utf-8") not in values:
                continue
            leaked[span.label] += score.interval_length(
                score.subtract_intervals([(span.start, span.end)], protected)
            )

    return leaked, record_document


def corpus(repo: Path, contract: str, dataset: Path) -> dict[str, list[score.Document]]:
    primary, _ = dataiku.load_documents(dataset)
    negatives, _ = benchmark.load_negative_documents(
        repo / "crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl"
    )
    documents = primary + negatives
    if contract == "v2":
        scoring = score.load_scored_label_contract(
            repo / "docs/reference/benchmarks/scored-labels-v2.json"
        )
        documents = score.apply_scored_label_contract(documents, scoring)
    prepared = agentic.prepare(repo)
    return {
        "C": documents,
        "A": prepared.identifiers,
        "D": prepared.lookalikes,
        "R": prepared.repeats,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--policy", required=True, type=Path)
    parser.add_argument("--model-dir", required=True, type=Path)
    parser.add_argument("--dataset", type=Path, default=Path("target/bench-data/dataiku-en-de/test.parquet"))
    parser.add_argument("--contract", required=True, choices=("v1", "v2"))
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--max-documents", type=int, help="development sample only; never publish as full arm")
    args = parser.parse_args()
    repo = args.repo.resolve()
    policy_path = args.policy.resolve()
    policy = tomllib.loads(policy_path.read_text(encoding="utf-8"))
    layers = corpus(repo, args.contract, (repo / args.dataset).resolve())
    repo_sha = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=repo, text=True
    ).strip()
    known_pool: dict[str, list[str]] = {}
    for document in [*layers["C"], *layers["A"]]:
        context, _ = record_for_document(document, policy)
        if context is not None:
            known_pool.setdefault(document.language, []).append(context)
    counterweight_documents, counterweight_contexts = explicit_counterweights(known_pool)
    layers["D"] = [*layers["D"], *counterweight_documents]
    output: dict[str, object] = {
        "arm": "known-record oracle (caller already knows the selected gold values)",
        "contract": args.contract,
        "full": args.max_documents is None,
        "source_commit": repo_sha,
        "policy_sha256": hashlib.sha256(policy_path.read_bytes()).hexdigest(),
        "dataset_sha256": dataiku.DATASET_SHA256,
        "agentic_manifest": agentic.manifest(agentic.PUBLISHED_PARTITION, agentic.generate(agentic.PUBLISHED_PARTITION)),
        "prediction_registered_before_measurement": "90-100% of baseline leaked bytes within eligible exact-value spans; overall reduction unknown (Solo scratchpad 10781)",
        "layers": {},
    }
    for layer, all_documents in layers.items():
        documents = all_documents[: args.max_documents] if args.max_documents else all_documents
        if args.max_documents and layer == "D":
            one_per_cell = {document.cell: document for document in reversed(counterweight_documents)}
            documents = [*documents, *one_per_cell.values()]
        core_documents = [document for document in documents if document.uid not in counterweight_contexts]
        contexts, eligible = paired_records(
            core_documents, policy, known_pool if layer == "D" else None
        )
        selected_ids = {document.uid for document in documents}
        contexts.update({uid: context for uid, context in counterweight_contexts.items() if uid in selected_ids})
        if not any(contexts.values()):
            output["layers"][layer] = {"skipped": "no eligible record contexts"}
            continue
        kwargs = dict(
            repo_root=repo,
            binary=args.binary.resolve(),
            config="policy-file",
            documents=documents,
            model_dir=args.model_dir.resolve(),
            opf_command=None,
            opf_checkpoint=None,
            opf_daemon_socket=None,
            threshold=0.3,
            diagnostics_dir=args.output.parent / f"known-record-{args.contract}-{layer}-logs",
            policy_path=policy_path,
        )
        baseline_eligible_leaks, baseline_record = eligible_leak_counter(contexts)
        exact_eligible_leaks, exact_record = eligible_leak_counter(contexts)
        record_eligible_leaks, record_record = eligible_leak_counter(contexts)
        clean_environment = dict(os.environ)
        clean_environment.pop("GAZE_BENCH_KNOWN_RECORD_ARM", None)
        clean_environment.pop("GAZE_BENCH_RECORD_EXACT_ONLY", None)
        baseline = score.run_config(
            **kwargs, base_environment=clean_environment, record_document=baseline_record
        )
        exact_record_run = run_with_record_context(
            contexts,
            **kwargs,
            base_environment={**clean_environment, "GAZE_BENCH_KNOWN_RECORD_ARM": "1", "GAZE_BENCH_RECORD_EXACT_ONLY": "1"},
            record_document=exact_record,
        )
        with_record = run_with_record_context(
            contexts,
            **kwargs,
            base_environment={**clean_environment, "GAZE_BENCH_KNOWN_RECORD_ARM": "1"},
            record_document=record_record,
        )
        output["layers"][layer] = {
            "documents": len(documents),
            "record_documents": sum(value is not None for value in contexts.values()),
            "explicit_counterweight_documents": sum(
                document.uid in counterweight_contexts and document.negative_category is not None
                for document in documents
            ),
            "explicit_variant_documents": sum(
                document.uid in counterweight_contexts and bool(document.spans)
                for document in documents
            ),
            "eligible_gold_bytes_by_label": dict(sorted(eligible.items())),
            "baseline_eligible_leaked_bytes_by_label": dict(sorted(baseline_eligible_leaks.items())),
            "exact_eligible_leaked_bytes_by_label": dict(sorted(exact_eligible_leaks.items())),
            "with_record_eligible_leaked_bytes_by_label": dict(sorted(record_eligible_leaks.items())),
            "baseline": {
                key: baseline[key]
                for key in ("metrics", "pipeline_contract", "pipeline_availability", "per_label_recall")
            },
            "with_record": {
                key: with_record[key]
                for key in ("metrics", "pipeline_contract", "pipeline_availability", "per_label_recall")
            },
            "exact_record": {
                key: exact_record_run[key]
                for key in ("metrics", "pipeline_contract", "pipeline_availability", "per_label_recall")
            },
        }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
