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
import re
import subprocess
import tomllib
from collections import Counter
from pathlib import Path
from typing import Sequence
from unittest.mock import patch

import agentic_layers as agentic
import dataiku_en_de_gaze_bench as dataiku
import gaze_bench_score as score
import known_record_attribution as attribution
import known_record_cells as kind_cells
from iban_trailing_word_enumeration import LENGTHS as IBAN_COUNTRY_LENGTHS
import run_no_opf_benchmark as benchmark


LABEL_CLASS = {
    "EMAIL": "Email",
    "GIVENNAME": "Name",
    "FIRSTNAME": "Name",
    "MIDDLENAME": "Name",
    "MIDDLE": "Name",
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
INFERRED_KEYS = {"Email": "email", "Name": "name", "Location": "address", "custom:phone": "phone", "custom:iban": "iban"}
NAME_MULTI_KINDS = (
    "exact", "case_folded", "whitespace_flexible", "whitespace_case_folded"
)


def common_name_words(repo: Path) -> frozenset[str]:
    source = repo / "crates/gaze-recognizers/assets/record-common-names-v1.txt"
    return frozenset(line.casefold() for line in source.read_text(encoding="utf-8").splitlines() if line and not line.startswith("#"))


def common_name_counts(documents: list[score.Document], contexts: dict[str, str | None], words: frozenset[str]) -> tuple[int, int]:
    values = 0
    hits = 0
    for document in documents:
        context = contexts.get(document.uid)
        if context is None:
            continue
        names = {value.casefold() for cls, value in record_values(context) if cls == "Name" and value.casefold() in words}
        values += len(names)
        folded_text = document.text.casefold()
        for name in names:
            hits += len(re.findall(r"(?<!\w)" + re.escape(name) + r"(?!\w)", folded_text))
    return values, hits


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


def valid_iban(value: str) -> bool:
    canonical = "".join(value.split()).upper()
    if len(canonical) != IBAN_COUNTRY_LENGTHS.get(canonical[:2]):
        return False
    if not (canonical[:2].isalpha() and canonical[2:4].isdigit() and canonical.isascii() and canonical.isalnum()):
        return False
    remainder = 0
    for char in canonical[4:] + canonical[:4]:
        for digit in str(int(char, 36)):
            remainder = (remainder * 10 + int(digit)) % 97
    return remainder == 1


def safe_record_value(value: str, class_name: str) -> bool:
    if class_name == "custom:iban" and valid_iban(value):
        return True
    letters = sum(ch.isalpha() for ch in value)
    digits = sum(ch.isnumeric() for ch in value)
    return (
        (digits >= 4 if letters == 0 else letters >= 3)
    )


def full_name_gold_spans(document: score.Document) -> list[tuple[int, int]]:
    """Join adjacent given/middle/surname gold on the same line."""
    encoded = document.text.encode("utf-8")
    names = sorted(
        (span for span in document.spans if span.label in {"GIVENNAME", "FIRSTNAME", "MIDDLENAME", "MIDDLE", "SURNAME"}),
        key=lambda span: span.start,
    )

    def adjacent(left: score.Span, right: score.Span) -> bool:
        if left.end >= right.start:
            return False
        gap = encoded[left.end:right.start].decode("utf-8")
        return gap.isspace() and "\n" not in gap and "\r" not in gap

    full_names = []
    for index, first in enumerate(names):
        if first.label not in {"GIVENNAME", "FIRSTNAME"}:
            continue
        previous = first
        next_index = index + 1
        while next_index < len(names) and names[next_index].label in {"MIDDLENAME", "MIDDLE"}:
            if not adjacent(previous, names[next_index]):
                break
            previous = names[next_index]
            next_index += 1
        if next_index < len(names) and names[next_index].label == "SURNAME" and adjacent(previous, names[next_index]):
            full_names.append((first.start, names[next_index].end))
    return full_names


def record_for_document(document: score.Document, policy: dict) -> tuple[str | None, Counter[str]]:
    encoded = document.text.encode("utf-8")
    fields: dict[str, dict[str, str]] = {}
    mapping: dict[str, str] = {}
    eligible: Counter[str] = Counter()
    seen_values: set[tuple[str, str]] = set()
    for span in document.spans:
        class_name = LABEL_CLASS.get(span.label)
        if class_name is None:
            continue
        if class_action(policy, class_name) not in {"tokenize", "format_preserve"}:
            raise ValueError(f"policy has no reversible action for selected class {class_name}")
        value = encoded[span.start : span.end].decode("utf-8")
        if not safe_record_value(value, class_name) or len(value.encode("utf-8")) > MAX_VALUE_BYTES:
            continue
        canonical_key = (class_name, " ".join(value.split()))
        if canonical_key in seen_values:
            eligible[span.label] += span.end - span.start
            continue
        if len(fields) >= MAX_FIELDS:
            break
        seen_values.add(canonical_key)
        slot = f"v{len(fields):02d}"
        key = INFERRED_KEYS.get(class_name, "value")
        fields[slot] = {key: value}
        if class_name not in INFERRED_KEYS:
            mapping[f"/{slot}/{key}"] = class_name
        eligible[span.label] += span.end - span.start
    seen_full_names = {
        value.casefold() for class_name, value in seen_values
        if class_name == "Name" and len(value.split()) > 1
    }
    for start, end in full_name_gold_spans(document):
        value = encoded[start:end].decode("utf-8")
        canonical_key = ("Name", " ".join(value.split()))
        folded_name = canonical_key[1].casefold()
        if folded_name in seen_full_names or len(fields) >= MAX_FIELDS:
            continue
        if not safe_record_value(value, "Name") or len(value.encode("utf-8")) > MAX_VALUE_BYTES:
            continue
        seen_full_names.add(folded_name)
        seen_values.add(canonical_key)
        fields[f"v{len(fields):02d}"] = {"name": value}
    if not fields:
        return None, eligible
    context = {"record": fields}
    if mapping:
        context["field_map"] = mapping
    return json.dumps(context, ensure_ascii=False), eligible


def record_values(context: str) -> list[tuple[str, str]]:
    parsed = json.loads(context)
    values = []
    for slot, fields in parsed["record"].items():
        for key, value in fields.items():
            path = f"/{slot}/{key}"
            class_name = parsed.get("field_map", {}).get(path)
            if class_name is None:
                class_name = next(cls for cls, alias in INFERRED_KEYS.items() if alias == key)
            values.append((class_name, value))
    return values


def enable_name_multi_measurement(context: str | None) -> str | None:
    if context is None:
        return None
    if not any(class_name == "Name" and len(value.split()) > 1 for class_name, value in record_values(context)):
        return context
    parsed = json.loads(context)
    # The product default is OFF until these kinds have measured positive rows.
    parsed.setdefault("record_match_kinds", {})["name_multi"] = list(NAME_MULTI_KINDS)
    return json.dumps(parsed, ensure_ascii=False)


def kind_contexts_for_measurement(
    pairs: Sequence[kind_cells.Pair], *, shipping_defaults: bool = False
) -> dict[str, str]:
    return {
        cell.uid: (
            cell.context_json()
            if shipping_defaults else kind_cells.arm_context(cell, probes_on=True)
        )
        for cell in kind_cells.cells(pairs)
    }


def name_multi_positive_spans(
    documents: list[score.Document], contexts: dict[str, str | None],
    *, kind_pairs: Sequence[kind_cells.Pair] = (),
) -> dict[str, int]:
    targets: set[tuple[str, int, int, str]] = set()
    for document in documents:
        context = contexts.get(document.uid)
        if context is None:
            continue
        names = [
            value for class_name, value in record_values(context)
            if class_name == "Name" and len(value.split()) > 1
        ]
        encoded = document.text.encode("utf-8")
        spans = [
            (span.start, span.end) for span in document.spans
            if LABEL_CLASS.get(span.label) == "Name"
        ] + full_name_gold_spans(document)
        for start, end in spans:
            text = encoded[start:end].decode("utf-8")
            kinds = set()
            for value in names:
                group, kind = attribution.match_group_and_kind(text, "Name", value, frozenset())
                if group == "name_multi" and kind in NAME_MULTI_KINDS:
                    kinds.add(kind)
            if len(kinds) > 1:
                raise ValueError("one name gold span matches multiple record kinds")
            for kind in kinds:
                targets.add((document.uid, start, end, kind))
    by_uid = {document.uid: document for document in documents}
    for pair in kind_pairs:
        document = by_uid.get(pair.positive.uid)
        if document is None or contexts.get(document.uid) is None:
            continue
        for target in pair.positive.primary_targets():
            group, kind = target.attribution
            if group == "name_multi" and kind in NAME_MULTI_KINDS:
                if any(span.start < target.end and target.start < span.end for span in document.spans):
                    targets.add((document.uid, target.start, target.end, kind))
    counts = Counter(kind for _, _, _, kind in targets)
    return {kind: counts[kind] for kind in NAME_MULTI_KINDS}


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
    """Price collisions and nearby benign text with paired known values."""
    documents: list[score.Document] = []
    contexts: dict[str, str] = {}
    for language, pool in sorted(known_pool.items()):
        counts: Counter[str] = Counter()

        def add(kind: str, text: str, context: str) -> None:
            if counts[kind] >= 16:
                return
            uid = f"known-record-{language}-{kind}-{counts[kind]:02d}"
            documents.append(
                score.Document(
                    uid=uid,
                    text=text,
                    language=language,
                    region="",
                    source_dataset="known-record-oracle-counterweight",
                    spans=(),
                    negative_category=f"record_{kind}",
                    cell=f"D|record_{kind}|synthetic|benign",
                )
            )
            contexts[uid] = context
            counts[kind] += 1

        for context in pool:
            for class_name, value in record_values(context):
                if class_name == "Name":
                    add("homonym", f"The fictional product is called {value}.", context)
                    parts = value.split()
                    if len(parts) == 2:
                        add("surname", f"The unrelated author surname is {parts[-1]}.", context)
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
        values = {value for _, value in record_values(context)}
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
    parser.add_argument("--shipping-defaults", action="store_true", help="measure product defaults without oracle kind overrides")
    parser.add_argument("--baseline-only", action="store_true", help="score no-record requests with a separate binary")
    parser.add_argument("--binary-source-commit", help="source commit of --binary when it differs from --repo")
    args = parser.parse_args()
    if args.baseline_only and not args.binary_source_commit:
        parser.error("--baseline-only requires --binary-source-commit")
    repo = args.repo.resolve()
    policy_path = args.policy.resolve()
    policy = tomllib.loads(policy_path.read_text(encoding="utf-8"))
    layers = corpus(repo, args.contract, (repo / args.dataset).resolve())
    repo_sha = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=repo, text=True
    ).strip()
    known_pool: dict[str, list[str]] = {}
    common_words = common_name_words(repo)
    for document in [*layers["C"], *layers["A"]]:
        context, _ = record_for_document(document, policy)
        if context is not None:
            known_pool.setdefault(document.language, []).append(context)
    counterweight_documents, counterweight_contexts = explicit_counterweights(known_pool)
    layers["D"] = [*layers["D"], *counterweight_documents]
    # These records differ from the text by a match kind; they are supplied,
    # not derived from gold like the primary oracle records.
    kind_pairs = kind_cells.generate()
    layers["K"], _ = kind_cells.documents(repo, args.contract, kind_pairs)
    kind_contexts = kind_contexts_for_measurement(
        kind_pairs, shipping_defaults=args.shipping_defaults
    )
    output: dict[str, object] = {
        "arm": "known-record oracle (caller already knows the selected gold values)",
        "contract": args.contract,
        "full": args.max_documents is None,
        "source_commit": repo_sha,
        "binary_source_commit": args.binary_source_commit or repo_sha,
        "policy_sha256": hashlib.sha256(policy_path.read_bytes()).hexdigest(),
        "dataset_sha256": dataiku.DATASET_SHA256,
        "agentic_manifest": agentic.manifest(agentic.PUBLISHED_PARTITION, agentic.generate(agentic.PUBLISHED_PARTITION)),
        "kind_cells_manifest": kind_cells.manifest(kind_pairs),
        "record_match_kinds_mode": (
            "baseline_only" if args.baseline_only else
            "shipping_defaults" if args.shipping_defaults else "probe_all"
        ),
        "name_multi_measurement_kinds": [] if args.shipping_defaults else list(NAME_MULTI_KINDS),
        "prediction_registered_before_measurement": "90-100% of baseline leaked bytes within eligible exact-value spans; overall reduction unknown (Solo scratchpad 10781)",
        "layers": {},
    }
    for layer, all_documents in layers.items():
        documents = all_documents[: args.max_documents] if args.max_documents else all_documents
        if args.max_documents and layer == "D":
            one_per_cell = {document.cell: document for document in reversed(counterweight_documents)}
            documents = [*documents, *one_per_cell.values()]
        core_documents = [
            document for document in documents
            if document.uid not in counterweight_contexts and document.uid not in kind_contexts
        ]
        contexts, eligible = paired_records(
            core_documents, policy, known_pool if layer == "D" else None
        )
        selected_ids = {document.uid for document in documents}
        contexts.update({uid: context for uid, context in counterweight_contexts.items() if uid in selected_ids})
        contexts.update({uid: context for uid, context in kind_contexts.items() if uid in selected_ids})
        if not args.shipping_defaults:
            contexts = {uid: enable_name_multi_measurement(context) for uid, context in contexts.items()}
        name_multi_inputs = name_multi_positive_spans(
            documents, contexts, kind_pairs=kind_pairs if layer == "K" else ()
        )
        common_values, common_hits = common_name_counts(documents, contexts, common_words)
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
        record_eligible_leaks, record_record = eligible_leak_counter(contexts)
        recorder = attribution.AttributionRecorder.create(
            common_words, frozenset(LABEL_CLASS)
        )
        kind_tally = kind_cells.VariantTally.create(kind_pairs)

        def baseline_observer(
            config: str, document: score.Document, response: dict, validators: object
        ) -> None:
            baseline_record(config, document, response, validators)
            recorder.record_baseline(document, response)
            kind_tally.record_baseline(document, response)

        def record_observer(
            config: str, document: score.Document, response: dict, validators: object
        ) -> None:
            record_record(config, document, response, validators)
            context = contexts[document.uid]
            recorder.record_candidate(
                document,
                response,
                record_values(context) if context is not None else [],
                decoy=document.source_dataset == "known-record-oracle-counterweight",
            )
            kind_tally.record_candidate(document, response)

        clean_environment = dict(os.environ)
        clean_environment.pop("GAZE_BENCH_KNOWN_RECORD_ARM", None)
        baseline = score.run_config(
            **kwargs, base_environment=clean_environment, record_document=baseline_observer
        )
        baseline_result = {
            key: baseline[key]
            for key in ("metrics", "pipeline_contract", "pipeline_availability", "per_label_recall")
        }
        if args.baseline_only:
            output["layers"][layer] = {"documents": len(documents), "baseline": baseline_result}
            continue
        with_record = run_with_record_context(
            contexts,
            **kwargs,
            base_environment={**clean_environment, "GAZE_BENCH_KNOWN_RECORD_ARM": "1"},
            record_document=record_observer,
        )
        baseline_bytes = baseline["metrics"]["utf8_bytes"]
        record_bytes = with_record["metrics"]["utf8_bytes"]
        output["layers"][layer] = {
            "documents": len(documents),
            "record_documents": sum(value is not None for value in contexts.values()),
            "common_word_record_values": common_values,
            "common_word_text_hits": common_hits,
            "name_multi_positive_spans_by_kind": name_multi_inputs,
            "explicit_counterweight_documents": sum(
                document.uid in counterweight_contexts and document.negative_category is not None
                for document in documents
            ),
            "eligible_gold_bytes_by_label": dict(sorted(eligible.items())),
            "baseline_eligible_leaked_bytes_by_label": dict(sorted(baseline_eligible_leaks.items())),
            "with_record_eligible_leaked_bytes_by_label": dict(sorted(record_eligible_leaks.items())),
            "attribution": recorder.result(
                leaked_fall=baseline_bytes["leaked"] - record_bytes["leaked"],
                false_positive_rise=record_bytes["false_positive"] - baseline_bytes["false_positive"],
                eligible_leak_fall=sum(baseline_eligible_leaks.values())
                - sum(record_eligible_leaks.values()),
            ),
            **({"kind_cells": kind_tally.result()} if layer == "K" else {}),
            "baseline": baseline_result,
            "with_record": {
                key: with_record[key]
                for key in ("metrics", "pipeline_contract", "pipeline_availability", "per_label_recall")
            },
        }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
