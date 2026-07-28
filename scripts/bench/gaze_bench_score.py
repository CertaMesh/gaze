#!/usr/bin/env python3
"""Shared fail-closed scoring for Gaze synthetic PII benchmarks.

The benchmark is deliberately label-agnostic at its primary boundary: a PII
byte is safe only when Gaze replaces it. Entity and per-label recall remain as
diagnostics, but conventional NER F1 is not allowed to hide partial leaks.
"""

from __future__ import annotations

import copy
import hashlib
import json
import math
import os
import re
import statistics
import subprocess
import sys
import time
import tomllib
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Mapping, Sequence


SCORECARD_SCHEMA_VERSION = 4
READABLE_SCORECARD_SCHEMA_VERSIONS = frozenset({3, SCORECARD_SCHEMA_VERSION})
DEFAULT_SAMPLE_SEED = 20_260_710
SAMPLING_STRATEGY = "deterministic-stratified-language-region-v1"
PIPELINE_FAILURE_STAGES = frozenset({"clean", "post_policy_scan"})
PIPELINE_FAILURE_REASONS = frozenset(
    {
        "invalid_regex",
        "unknown_token",
        "export_forbidden",
        "empty_document_integrity",
        "invalid_snapshot_version",
        "invalid_snapshot_signature",
        "blob_expired",
        "snapshot_decode",
        "invalid_snapshot_payload",
        "sqlite",
        "policy",
        "rulepack",
        "recognizer_detect",
        "redaction_log",
        "safety_net_unavailable",
        "safety_net_weights_missing",
        "safety_net_model_unavailable",
        "safety_net_model_integrity_mismatch",
        "safety_net_input_too_large",
        "safety_net_runtime_subprocess_timeout",
        "safety_net_runtime_subprocess_non_zero_exit",
        "safety_net_runtime_other",
        "safety_net_invalid_output_malformed_backend_output",
        "safety_net_invalid_output_label_decode_mismatch",
        "safety_net_invalid_output_clean_to_raw_mapping",
        "safety_net_invalid_output_trace_manifest_mismatch",
        "safety_net_invalid_output_other",
        "safety_net_fallback_overlap_conflict",
        "safety_net_fallback_validator_veto",
        "safety_net_fallback_anchor_missing",
        "safety_net_fallback_residual_suspect",
        "safety_net_span_invalid",
        "unsupported_capital_heuristic_locale",
        "unsupported_raw_document_variant",
        "unsupported_structured_value_variant",
        "unsupported_policy_action_variant",
    }
)

# These are direct or account-linked identifiers. The remaining observed
# labels (date, time, age, gender, sex, title, amount, and currency) are still
# included in the all-PII score, but are reported separately as contextual PII.
DIRECT_IDENTIFIER_LABELS = frozenset(
    {
        "ACCOUNTNUM",
        "BANKNAME",
        "BUILDINGNUM",
        "CITY",
        "CREDITCARDNUMBER",
        "DRIVERLICENSENUM",
        "EMAIL",
        "GIVENNAME",
        "IDCARDNUM",
        "ORGANISATION",
        "PASSPORTNUM",
        "SOCIALNUM",
        "STREET",
        "SURNAME",
        "TAXNUM",
        "TELEPHONENUM",
        "URL",
        "ZIPCODE",
    }
)

DEFAULT_CONFIGS = (
    "rule-floor-extended",
    "pass2-ner",
    "full-stack-kiji-resolve",
)
PRODUCTION_CONFIG = "full-stack-kiji-resolve"
VALIDATOR_PROBE_PROTOCOL_SCHEMA_VERSION = 1
VALIDATOR_PROBE_MANIFEST = Path("scripts/bench/validator_recall_probe/Cargo.toml")
VALIDATOR_PROBE_TARGET = Path("target/validator-recall-probe")

# Corpus labels describe source-dataset semantics, while validator-backed
# recognizers emit canonical Gaze classes. Keep this mapping narrow: an absent
# label is explicitly not applicable, never silently treated as zero recall.
VALIDATOR_CLASSES_BY_LABEL: dict[str, tuple[str, ...]] = {
    "ACCOUNTNUM": ("custom:iban",),
    "AADHAAR": ("custom:aadhaar",),
    "BSN": ("custom:bsn",),
    "CNPJ": ("custom:cnpj",),
    "CPF": ("custom:cpf",),
    "CREDITCARDNUMBER": ("custom:credit_card",),
    "EMAIL": ("email",),
    "ETHADDRESS": ("custom:eth_address",),
    "IBAN": ("custom:iban",),
    "IPADDRESS": ("custom:ip_address",),
    "IPV4": ("custom:ip_address",),
    "IPV6": ("custom:ip_address",),
    "NHSNUMBER": ("custom:nhs_number",),
    "NIR": ("custom:nir",),
    "PHONENUMBER": ("custom:phone",),
    "TAXNUM": ("custom:steuer_id",),
    "TELEPHONENUM": ("custom:phone",),
}


@dataclass(frozen=True)
class Span:
    start: int
    end: int
    label: str


@dataclass(frozen=True)
class Document:
    uid: str
    text: str
    language: str
    region: str
    source_dataset: str
    spans: tuple[Span, ...]
    negative_category: str | None = None

    @property
    def locale_chain(self) -> list[str]:
        locale = f"{self.language}-{self.region}" if self.region else self.language
        return [locale, "global"]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def document_ids_digest(document_ids: Sequence[str]) -> dict[str, str]:
    payload = json.dumps(
        list(document_ids), ensure_ascii=False, separators=(",", ":")
    ).encode("utf-8")
    return {"algorithm": "sha256", "value": hashlib.sha256(payload).hexdigest()}


def identified_document_population(document_ids: Iterable[str]) -> dict[str, object]:
    ids = sorted(document_ids)
    if not all(isinstance(uid, str) and uid for uid in ids):
        raise ValueError("document IDs must be non-empty strings")
    if len(set(ids)) != len(ids):
        raise ValueError("document IDs must be unique")
    return {
        "documents": len(ids),
        "document_ids": ids,
        "document_ids_digest": document_ids_digest(ids),
    }


def char_to_byte_offsets(text: str) -> list[int]:
    offsets = [0]
    byte_offset = 0
    for character in text:
        byte_offset += len(character.encode("utf-8"))
        offsets.append(byte_offset)
    return offsets


def population_summary(documents: Sequence[Document]) -> dict[str, object]:
    labels = Counter(span.label for document in documents for span in document.spans)
    languages = Counter(document.language for document in documents)
    regions = Counter(
        f"{document.language}-{document.region}"
        if document.region
        else document.language
        for document in documents
    )
    sources = Counter(document.source_dataset for document in documents)
    return {
        "documents": len(documents),
        "entities": sum(labels.values()),
        "labels": dict(sorted(labels.items())),
        "languages": dict(sorted(languages.items())),
        "regions": dict(sorted(regions.items())),
        "sources": dict(sorted(sources.items())),
        "negative_only_documents": sum(not document.spans for document in documents),
    }


def _stratum(document: Document) -> tuple[str, str]:
    return document.language, document.region


def _document_rank(seed: int, document: Document) -> bytes:
    payload = json.dumps(
        [seed, document.language, document.region, document.uid],
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(payload).digest()


def _proportional_allocation(
    capacities: Mapping[tuple[str, str], int], budget: int
) -> dict[tuple[str, str], int]:
    allocations = {key: 0 for key in capacities}
    if budget <= 0:
        return allocations
    capacity_total = sum(capacities.values())
    if budget > capacity_total:
        raise ValueError("sample allocation exceeds available population")
    quotas = {
        key: budget * capacity / capacity_total for key, capacity in capacities.items()
    }
    for key, quota in quotas.items():
        allocations[key] = min(capacities[key], math.floor(quota))
    remaining = budget - sum(allocations.values())
    order = sorted(
        capacities,
        key=lambda key: (
            -(quotas[key] - math.floor(quotas[key])),
            -capacities[key],
            key,
        ),
    )
    for key in order:
        if remaining == 0:
            break
        if allocations[key] < capacities[key]:
            allocations[key] += 1
            remaining -= 1
    if remaining:
        raise RuntimeError("deterministic stratum allocation left unassigned slots")
    return allocations


def _allocate_strata(
    strata: Mapping[tuple[str, str], Sequence[Document]], sample_size: int
) -> dict[tuple[str, str], int]:
    capacities = {key: len(value) for key, value in strata.items()}
    return _proportional_allocation(capacities, sample_size)


def stratified_sample(
    documents: Sequence[Document],
    max_documents: int | None,
    seed: int = DEFAULT_SAMPLE_SEED,
) -> tuple[list[Document], dict[str, object]]:
    if type(seed) is not int:
        raise TypeError("sampling seed must be an integer")
    if not documents:
        raise ValueError("available population is empty")
    if max_documents is not None and (
        type(max_documents) is not int or max_documents <= 0
    ):
        raise ValueError("--max-documents must be greater than zero")
    uids = [document.uid for document in documents]
    if len(set(uids)) != len(uids):
        raise ValueError("document IDs must be unique before sampling")

    sample_size = min(max_documents or len(documents), len(documents))
    strata: defaultdict[tuple[str, str], list[Document]] = defaultdict(list)
    for document in documents:
        strata[_stratum(document)].append(document)
    allocation = _allocate_strata(strata, sample_size)
    evaluated: list[Document] = []
    for key in sorted(strata):
        ranked = sorted(
            strata[key],
            key=lambda document: (_document_rank(seed, document), document.uid),
        )
        evaluated.extend(ranked[: allocation[key]])
    evaluated.sort(key=lambda document: (_document_rank(seed, document), document.uid))

    evaluated_ids = [document.uid for document in evaluated]
    report: dict[str, object] = {
        "strategy": SAMPLING_STRATEGY,
        "seed": seed,
        "requested_max_documents": max_documents,
        "available_population": population_summary(documents),
        "evaluated_population": population_summary(evaluated),
        "evaluated_document_ids": evaluated_ids,
        "evaluated_document_ids_digest": document_ids_digest(evaluated_ids),
    }
    return evaluated, report


class ResponseValidationError(RuntimeError):
    """The Rust benchmark producer violated its closed wire contract."""


class SourceIdVocabularyError(RuntimeError):
    """A committed source-ID vocabulary input could not be loaded safely."""


def _expect_object(value: object, context: str) -> dict[str, object]:
    if not isinstance(value, dict) or not all(isinstance(key, str) for key in value):
        raise ResponseValidationError(f"{context}: expected an object")
    return value


def _expect_exact_keys(
    value: object, required: frozenset[str], context: str
) -> dict[str, object]:
    result = _expect_object(value, context)
    missing = sorted(required - result.keys())
    unknown = sorted(result.keys() - required)
    if missing or unknown:
        parts = []
        if missing:
            parts.append(f"missing fields {missing}")
        if unknown:
            parts.append(f"unknown fields {unknown}")
        raise ResponseValidationError(f"{context}: {'; '.join(parts)}")
    return result


def _expect_string(value: object, context: str) -> str:
    if not isinstance(value, str):
        raise ResponseValidationError(f"{context}: expected a string")
    return value


def _expect_bool(value: object, context: str) -> bool:
    if type(value) is not bool:
        raise ResponseValidationError(f"{context}: expected a boolean")
    return value


def _expect_int(value: object, context: str, *, minimum: int = 0) -> int:
    if type(value) is not int or value < minimum:
        raise ResponseValidationError(
            f"{context}: expected an integer greater than or equal to {minimum}"
        )
    return value


def _expect_number(value: object, context: str) -> float:
    if type(value) not in {int, float} or not math.isfinite(float(value)):
        raise ResponseValidationError(f"{context}: expected a finite number")
    if float(value) < 0:
        raise ResponseValidationError(f"{context}: expected a non-negative number")
    return float(value)


def _expect_list(value: object, context: str) -> list[object]:
    if not isinstance(value, list):
        raise ResponseValidationError(f"{context}: expected an array")
    return value


MANIFEST_SPAN_FIELDS = frozenset(
    {"raw_start", "raw_end", "clean_start", "clean_end", "class"}
)
FINAL_PROTECTION_TRACE_FIELDS = frozenset(
    {"raw_start", "raw_end", "class", "action", "provenance"}
)
TRACE_PROVENANCE_FIELDS = frozenset({"stage", "decision", "source_ids"})
VALID_TRACE_COMBINATIONS = frozenset(
    {
        ("primary_pipeline", "policy", "tokenize"),
        ("safety_net", "resolve", "tokenize"),
        ("safety_net", "redact", "redact"),
        ("safety_net", "fallback_redact", "redact"),
    }
)
BUILTIN_CANONICAL_CLASSES = frozenset({"email", "name", "location", "organization"})
SOURCE_ID_PATTERN = re.compile(r"(?=.{1,128}\Z)[a-z][a-z0-9]*(?:[._:/-][a-z0-9]+)*\Z")
LEAK_SUSPECT_FIELDS = frozenset(
    {
        "clean_start",
        "clean_end",
        "action_start",
        "action_end",
        "class",
        "safety_net_id",
        "kind",
    }
)
SAFETY_NET_STATS_FIELDS = frozenset(
    {
        "suspect_count",
        "uncovered_count",
        "partial_bleed_count",
        "class_mismatch_count",
        "locale_skipped_count",
    }
)
RESTORE_FIELDS = frozenset(
    {
        "exact",
        "decision",
        "unknown_token_count",
        "manifest_bypass_count",
        "fresh_pii_detected_count",
        "phase_execution_mask",
    }
)
MANIFEST_INTEGRITY_FIELDS = frozenset(
    {
        "spans",
        "invalid_clean_bounds",
        "invalid_raw_bounds",
        "overlapping_clean_spans",
        "non_monotonic_raw_spans",
        "token_restore_failures",
        "raw_value_mismatches",
    }
)
SUCCESS_TIMING_FIELDS = frozenset({"clean_ms", "restore_ms", "post_policy_scan_ms"})
SUCCESS_RESPONSE_FIELDS = frozenset(
    {
        "fixture_id",
        "clean_text",
        "manifest_spans",
        "pre_safety_text_len",
        "pre_safety_manifest_spans",
        "leak_suspects",
        "safety_net_mode",
        "strict_would_reject",
        "initial_safety_net_stats",
        "post_policy_safety_net_stats",
        "restore",
        "manifest_integrity",
        "timing",
        "final_protection_trace",
    }
)
PIPELINE_ERROR_RESPONSE_FIELDS = frozenset(
    {"fixture_id", "pipeline_error_stage", "pipeline_error_code", "timing"}
)


def _validate_manifest_span(value: object, context: str) -> None:
    span = _expect_exact_keys(value, MANIFEST_SPAN_FIELDS, context)
    for field in ("raw_start", "raw_end", "clean_start", "clean_end"):
        _expect_int(span[field], f"{context}.{field}")
    _expect_string(span["class"], f"{context}.class")


def _validate_leak_suspect(value: object, context: str) -> None:
    suspect = _expect_exact_keys(value, LEAK_SUSPECT_FIELDS, context)
    for field in ("clean_start", "clean_end", "action_start", "action_end"):
        _expect_int(suspect[field], f"{context}.{field}")
    for field in ("class", "safety_net_id", "kind"):
        _expect_string(suspect[field], f"{context}.{field}")


def _validate_safety_net_stats(value: object, context: str) -> None:
    stats = _expect_exact_keys(value, SAFETY_NET_STATS_FIELDS, context)
    for field in SAFETY_NET_STATS_FIELDS:
        _expect_int(stats[field], f"{context}.{field}")


def _validate_restore(value: object, context: str) -> None:
    restore = _expect_exact_keys(value, RESTORE_FIELDS, context)
    _expect_bool(restore["exact"], f"{context}.exact")
    _expect_string(restore["decision"], f"{context}.decision")
    for field in RESTORE_FIELDS - {"exact", "decision"}:
        _expect_int(restore[field], f"{context}.{field}")


def _validate_manifest_integrity(value: object, context: str) -> None:
    integrity = _expect_exact_keys(value, MANIFEST_INTEGRITY_FIELDS, context)
    for field in MANIFEST_INTEGRITY_FIELDS:
        _expect_int(integrity[field], f"{context}.{field}")


def _validate_success_timing(value: object, context: str) -> None:
    timing = _expect_exact_keys(value, SUCCESS_TIMING_FIELDS, context)
    for field in SUCCESS_TIMING_FIELDS:
        if field == "post_policy_scan_ms" and timing[field] is None:
            continue
        _expect_number(timing[field], f"{context}.{field}")


def _validate_canonical_class(value: str, context: str) -> None:
    is_custom = value.startswith("custom:") and len(value) > len("custom:")
    if value not in BUILTIN_CANONICAL_CLASSES and not is_custom:
        raise ResponseValidationError(
            f"{context}: expected a canonical PiiClass representation"
        )


def _validate_source_id(value: str, context: str) -> None:
    if not SOURCE_ID_PATTERN.fullmatch(value):
        raise ResponseValidationError(
            f"{context}: expected a metadata-only stable identifier"
        )


def _load_committed_toml(path: Path) -> dict[str, object]:
    if not path.is_file():
        raise SourceIdVocabularyError(
            f"committed source-ID vocabulary input is missing: {path}"
        )
    try:
        with path.open("rb") as handle:
            value = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise SourceIdVocabularyError(
            f"cannot read committed source-ID vocabulary input {path}: {error}"
        ) from error
    if not isinstance(value, dict):
        raise SourceIdVocabularyError(
            f"committed source-ID vocabulary input must be a TOML table: {path}"
        )
    return value


def _committed_vocabulary_id(value: object, context: str) -> str:
    if not isinstance(value, str) or not SOURCE_ID_PATTERN.fullmatch(value):
        raise SourceIdVocabularyError(
            f"{context}: expected a metadata-only stable identifier"
        )
    return value


def _committed_builtin_source_ids(
    models: dict[str, object], model_path: Path
) -> tuple[str, ...]:
    section = models.get("builtin_source_ids")
    if not isinstance(section, dict):
        raise SourceIdVocabularyError(
            f"committed built-in source-ID declaration must contain a "
            f"[builtin_source_ids] table: {model_path}"
        )
    if set(section) != {"ids"}:
        raise SourceIdVocabularyError(
            f"{model_path}: [builtin_source_ids] must contain only an ids array"
        )
    values = section["ids"]
    if not isinstance(values, list) or not values:
        raise SourceIdVocabularyError(
            f"{model_path}: [builtin_source_ids].ids must be a non-empty array"
        )
    identifiers = tuple(
        _committed_vocabulary_id(
            value, f"{model_path}: builtin_source_ids.ids[{index}]"
        )
        for index, value in enumerate(values)
    )
    if len(set(identifiers)) != len(identifiers):
        raise SourceIdVocabularyError(
            f"{model_path}: [builtin_source_ids].ids must not contain duplicates"
        )
    return identifiers


def load_committed_source_id_vocabulary(
    repo_root: Path | None = None,
) -> frozenset[str]:
    root = repo_root or Path(__file__).resolve().parents[2]
    rulepack_dir = root / "crates/gaze-recognizers/embedded"
    if not rulepack_dir.is_dir():
        raise SourceIdVocabularyError(
            f"committed rulepack directory is missing: {rulepack_dir}"
        )
    rulepack_paths = sorted(rulepack_dir.glob("*.toml"))
    if not rulepack_paths:
        raise SourceIdVocabularyError(
            f"committed rulepack directory contains no TOML bundles: {rulepack_dir}"
        )

    identifiers: set[str] = set()
    recognizer_count = 0
    for path in rulepack_paths:
        rulepack = _load_committed_toml(path)
        recognizers = rulepack.get("recognizers", [])
        if not isinstance(recognizers, list):
            raise SourceIdVocabularyError(
                f"{path}: recognizers must be an array of tables"
            )
        for index, recognizer in enumerate(recognizers):
            if not isinstance(recognizer, dict) or "id" not in recognizer:
                raise SourceIdVocabularyError(
                    f"{path}: recognizers[{index}] must declare an id"
                )
            identifiers.add(
                _committed_vocabulary_id(
                    recognizer["id"], f"{path}: recognizers[{index}].id"
                )
            )
            collision = recognizer.get("collision")
            if collision is not None:
                collision_context = f"{path}: recognizers[{index}].collision"
                if not isinstance(collision, dict) or "family" not in collision:
                    raise SourceIdVocabularyError(
                        f"{collision_context} must be a table declaring a family"
                    )
                family = collision["family"]
                if not isinstance(family, str):
                    raise SourceIdVocabularyError(
                        f"{collision_context}.family: expected a metadata-only "
                        "stable identifier"
                    )
                identifiers.add(
                    _committed_vocabulary_id(
                        f"collision-family:{family}",
                        f"{collision_context}.family",
                    )
                )
            recognizer_count += 1
    if recognizer_count == 0:
        raise SourceIdVocabularyError(
            f"committed rulepacks declare no recognizer IDs: {rulepack_dir}"
        )

    model_path = root / "scripts/bench/no_opf_models.toml"
    models = _load_committed_toml(model_path)
    identifiers.update(_committed_builtin_source_ids(models, model_path))
    model_count = 0
    for section_name, section in models.items():
        if section_name == "builtin_source_ids":
            continue
        if not isinstance(section, dict) or "id" not in section:
            continue
        identifiers.add(
            _committed_vocabulary_id(section["id"], f"{model_path}: {section_name}.id")
        )
        model_count += 1
    if model_count == 0:
        raise SourceIdVocabularyError(
            f"committed model source declares no IDs: {model_path}"
        )
    if not identifiers:
        raise SourceIdVocabularyError("committed source-ID vocabulary is empty")
    return frozenset(identifiers)


COMMITTED_SOURCE_ID_VOCABULARY = load_committed_source_id_vocabulary()


def _validate_final_protection_trace(
    document: Document,
    value: object,
    manifest: Sequence[object],
) -> list[Span]:
    trace = _expect_list(value, "final_protection_trace")
    original_text = document.text.encode("utf-8")
    original_text_bytes = len(original_text)
    char_boundaries = frozenset(char_to_byte_offsets(document.text))
    previous_raw_end = 0
    predictions: list[Span] = []
    tokenize_items: Counter[tuple[int, int, str]] = Counter()
    protected_raw_values: list[str] = []
    source_identifiers: list[tuple[str, str]] = []

    for index, raw_item in enumerate(trace):
        context = f"final_protection_trace[{index}]"
        item = _expect_exact_keys(raw_item, FINAL_PROTECTION_TRACE_FIELDS, context)
        raw_start = _expect_int(item["raw_start"], f"{context}.raw_start")
        raw_end = _expect_int(item["raw_end"], f"{context}.raw_end")
        pii_class = _expect_string(item["class"], f"{context}.class")
        action = _expect_string(item["action"], f"{context}.action")
        provenance = _expect_exact_keys(
            item["provenance"], TRACE_PROVENANCE_FIELDS, f"{context}.provenance"
        )
        stage = _expect_string(provenance["stage"], f"{context}.provenance.stage")
        decision = _expect_string(
            provenance["decision"], f"{context}.provenance.decision"
        )
        source_values = _expect_list(
            provenance["source_ids"], f"{context}.provenance.source_ids"
        )
        source_ids = [
            _expect_string(
                source_id, f"{context}.provenance.source_ids[{source_index}]"
            )
            for source_index, source_id in enumerate(source_values)
        ]

        _validate_canonical_class(pii_class, f"{context}.class")
        if action not in {"tokenize", "redact"}:
            raise ResponseValidationError(
                f"{context}.action: unknown action {action!r}"
            )
        if (stage, decision, action) not in VALID_TRACE_COMBINATIONS:
            raise ResponseValidationError(
                f"{context}.provenance: invalid stage/decision/action combination"
            )
        if not source_ids:
            raise ResponseValidationError(
                f"{context}.provenance.source_ids: expected non-empty metadata IDs"
            )
        for source_index, source_id in enumerate(source_ids):
            source_context = f"{context}.provenance.source_ids[{source_index}]"
            _validate_source_id(source_id, source_context)
            source_identifiers.append((source_id, source_context))
        if source_ids != sorted(source_ids) or len(source_ids) != len(set(source_ids)):
            raise ResponseValidationError(
                f"{context}.provenance.source_ids: expected sorted duplicate-free IDs"
            )
        if raw_start >= raw_end or raw_end > original_text_bytes:
            raise ResponseValidationError(
                f"{context}: invalid original-text bounds {raw_start}:{raw_end}"
            )
        if raw_start not in char_boundaries or raw_end not in char_boundaries:
            raise ResponseValidationError(
                f"{context}: span endpoints are not original-text UTF-8 char boundaries"
            )
        if raw_start < previous_raw_end:
            raise ResponseValidationError(
                f"{context}: trace spans must be sorted and disjoint"
            )
        previous_raw_end = raw_end
        protected_raw_values.append(original_text[raw_start:raw_end].decode("utf-8"))
        predictions.append(Span(raw_start, raw_end, pii_class))
        if action == "tokenize":
            tokenize_items[(raw_start, raw_end, pii_class)] += 1

    manifest_items: Counter[tuple[int, int, str]] = Counter()
    for index, raw_span in enumerate(manifest):
        span = _expect_object(raw_span, f"manifest_spans[{index}]")
        manifest_items[
            (int(span["raw_start"]), int(span["raw_end"]), str(span["class"]))
        ] += 1
    if tokenize_items != manifest_items:
        raise ResponseValidationError(
            "final_protection_trace: tokenize items must agree 1:1 with the final manifest"
        )
    for source_id, context in source_identifiers:
        if source_id not in COMMITTED_SOURCE_ID_VOCABULARY and any(
            raw_value
            and re.search(
                rf"(?:^|(?<=[._:/-])){re.escape(raw_value)}(?:$|(?=[._:/-]))",
                source_id,
            )
            for raw_value in protected_raw_values
        ):
            raise ResponseValidationError(
                f"{context}: identifier reproduces protected request content"
            )
    return predictions


def validate_response(document: Document, value: object) -> dict[str, object]:
    response = _expect_object(value, f"{document.uid}: response")
    if "pipeline_error_code" in response:
        response = _expect_exact_keys(
            response,
            PIPELINE_ERROR_RESPONSE_FIELDS,
            f"{document.uid}: pipeline error response",
        )
        fixture_id = _expect_string(response["fixture_id"], "fixture_id")
        stage = _expect_string(response["pipeline_error_stage"], "pipeline_error_stage")
        reason = _expect_string(response["pipeline_error_code"], "pipeline_error_code")
        if stage not in PIPELINE_FAILURE_STAGES:
            raise ResponseValidationError(
                f"pipeline_error_stage: unknown closed stage {stage!r}"
            )
        if reason not in PIPELINE_FAILURE_REASONS:
            raise ResponseValidationError(
                f"pipeline_error_code: unknown closed reason {reason!r}"
            )
        timing = _expect_exact_keys(
            response["timing"], frozenset({"total_ms"}), "pipeline error timing"
        )
        _expect_number(timing["total_ms"], "pipeline error timing.total_ms")
    else:
        response = _expect_exact_keys(
            response, SUCCESS_RESPONSE_FIELDS, f"{document.uid}: success response"
        )
        fixture_id = _expect_string(response["fixture_id"], "fixture_id")
        _expect_string(response["clean_text"], "clean_text")
        manifest = _expect_list(response["manifest_spans"], "manifest_spans")
        for index, span in enumerate(manifest):
            _validate_manifest_span(span, f"manifest_spans[{index}]")
        pre_safety_length = response["pre_safety_text_len"]
        if pre_safety_length is not None:
            _expect_int(pre_safety_length, "pre_safety_text_len")
        pre_safety_manifest = response["pre_safety_manifest_spans"]
        if pre_safety_manifest is not None:
            for index, span in enumerate(
                _expect_list(pre_safety_manifest, "pre_safety_manifest_spans")
            ):
                _validate_manifest_span(span, f"pre_safety_manifest_spans[{index}]")
        suspects = _expect_list(response["leak_suspects"], "leak_suspects")
        for index, suspect in enumerate(suspects):
            _validate_leak_suspect(suspect, f"leak_suspects[{index}]")
        _expect_string(response["safety_net_mode"], "safety_net_mode")
        _expect_bool(response["strict_would_reject"], "strict_would_reject")
        _validate_safety_net_stats(
            response["initial_safety_net_stats"], "initial_safety_net_stats"
        )
        initial_stats = _expect_object(
            response["initial_safety_net_stats"], "initial_safety_net_stats"
        )
        suspect_kinds = Counter(
            _expect_object(suspect, "leak_suspect")["kind"] for suspect in suspects
        )
        expected_suspect_counts = {
            "suspect_count": len(suspects),
            "uncovered_count": suspect_kinds["uncovered"],
            "partial_bleed_count": suspect_kinds["partial_bleed"],
            "class_mismatch_count": suspect_kinds["class_mismatch"],
        }
        for field, expected in expected_suspect_counts.items():
            if initial_stats[field] != expected:
                raise ResponseValidationError(
                    f"initial_safety_net_stats.{field}: telemetry/output disagreement"
                )
        expected_strict_rejection = (
            expected_suspect_counts["uncovered_count"]
            + expected_suspect_counts["partial_bleed_count"]
            > 0
        )
        if response["strict_would_reject"] is not expected_strict_rejection:
            raise ResponseValidationError(
                "strict_would_reject: telemetry/output disagreement"
            )
        post_policy = response["post_policy_safety_net_stats"]
        if post_policy is not None:
            _validate_safety_net_stats(post_policy, "post_policy_safety_net_stats")
        _validate_restore(response["restore"], "restore")
        _validate_manifest_integrity(
            response["manifest_integrity"], "manifest_integrity"
        )
        integrity = _expect_object(response["manifest_integrity"], "manifest_integrity")
        if integrity["spans"] != len(manifest):
            raise ResponseValidationError(
                "manifest_integrity.spans: telemetry/output disagreement"
            )
        _validate_success_timing(response["timing"], "timing")
        _validate_final_protection_trace(
            document, response["final_protection_trace"], manifest
        )
        if (
            any(
                _expect_object(item, "final_protection_trace item")["action"]
                == "redact"
                for item in _expect_list(
                    response["final_protection_trace"], "final_protection_trace"
                )
            )
            and _expect_object(response["restore"], "restore")["exact"] is True
        ):
            raise ResponseValidationError(
                "restore.exact: redact protection cannot be exactly reversible"
            )

    if fixture_id != document.uid:
        raise ResponseValidationError(
            f"runner response mismatch: expected {document.uid}, received {fixture_id}"
        )
    return response


def merge_intervals(intervals: Iterable[tuple[int, int]]) -> list[tuple[int, int]]:
    merged: list[tuple[int, int]] = []
    for start, end in sorted(intervals):
        if end <= start:
            continue
        if merged and start <= merged[-1][1]:
            previous_start, previous_end = merged[-1]
            merged[-1] = (previous_start, max(previous_end, end))
        else:
            merged.append((start, end))
    return merged


def interval_length(intervals: Sequence[tuple[int, int]]) -> int:
    return sum(end - start for start, end in intervals)


def intersection_length(
    left: Sequence[tuple[int, int]], right: Sequence[tuple[int, int]]
) -> int:
    total = 0
    left_index = 0
    right_index = 0
    while left_index < len(left) and right_index < len(right):
        left_start, left_end = left[left_index]
        right_start, right_end = right[right_index]
        total += max(0, min(left_end, right_end) - max(left_start, right_start))
        if left_end <= right_end:
            left_index += 1
        else:
            right_index += 1
    return total


def interval_is_covered(
    interval: tuple[int, int], covering: Sequence[tuple[int, int]]
) -> bool:
    start, end = interval
    return any(
        cover_start <= start and cover_end >= end for cover_start, cover_end in covering
    )


def interval_overlaps(
    interval: tuple[int, int], candidates: Sequence[tuple[int, int]]
) -> bool:
    start, end = interval
    return any(
        candidate_start < end and start < candidate_end
        for candidate_start, candidate_end in candidates
    )


def safe_ratio(numerator: int | float, denominator: int | float) -> float:
    if denominator == 0:
        return 1.0 if numerator == 0 else 0.0
    return numerator / denominator


class MetricAccumulator:
    def __init__(self) -> None:
        self.documents = 0
        self.documents_without_leaks = 0
        self.documents_with_false_positives = 0
        self.pii_bytes = 0
        self.predicted_bytes = 0
        self.true_positive_bytes = 0
        self.non_pii_bytes = 0
        self.false_positive_bytes = 0
        self.entities = 0
        self.entities_fully_covered = 0
        self.entities_overlapped = 0
        self.entities_exact = 0
        self.prediction_spans = 0
        self.prediction_spans_overlapped = 0

    def add(self, document: Document, predictions: Sequence[Span]) -> None:
        gold = merge_intervals((span.start, span.end) for span in document.spans)
        predicted = merge_intervals((span.start, span.end) for span in predictions)
        text_bytes = len(document.text.encode("utf-8"))
        gold_bytes = interval_length(gold)
        predicted_bytes = interval_length(predicted)
        true_positive_bytes = intersection_length(gold, predicted)
        false_positive_bytes = predicted_bytes - true_positive_bytes

        self.documents += 1
        self.documents_without_leaks += true_positive_bytes == gold_bytes
        self.documents_with_false_positives += false_positive_bytes > 0
        self.pii_bytes += gold_bytes
        self.predicted_bytes += predicted_bytes
        self.true_positive_bytes += true_positive_bytes
        self.non_pii_bytes += text_bytes - gold_bytes
        self.false_positive_bytes += false_positive_bytes
        self.entities += len(document.spans)
        self.entities_fully_covered += sum(
            interval_is_covered((span.start, span.end), predicted)
            for span in document.spans
        )
        self.entities_overlapped += sum(
            interval_overlaps((span.start, span.end), predicted)
            for span in document.spans
        )
        exact = {(span.start, span.end) for span in predictions}
        self.entities_exact += sum(
            (span.start, span.end) in exact for span in document.spans
        )
        self.prediction_spans += len(predictions)
        self.prediction_spans_overlapped += sum(
            interval_overlaps((span.start, span.end), gold) for span in predictions
        )

    def result(self) -> dict[str, object]:
        precision = safe_ratio(self.true_positive_bytes, self.predicted_bytes)
        recall = safe_ratio(self.true_positive_bytes, self.pii_bytes)
        f1 = safe_ratio(2 * precision * recall, precision + recall)
        f2 = safe_ratio(5 * precision * recall, 4 * precision + recall)
        return {
            "documents": self.documents,
            "zero_leak_document_rate": safe_ratio(
                self.documents_without_leaks, self.documents
            ),
            "documents_without_leaks": self.documents_without_leaks,
            "documents_with_false_positives": self.documents_with_false_positives,
            "utf8_bytes": {
                "pii": self.pii_bytes,
                "predicted": self.predicted_bytes,
                "true_positive": self.true_positive_bytes,
                "false_positive": self.false_positive_bytes,
                "leaked": self.pii_bytes - self.true_positive_bytes,
                "precision": precision,
                "recall": recall,
                "leak_rate": 1.0 - recall,
                "f1": f1,
                "f2": f2,
                "false_positive_rate": safe_ratio(
                    self.false_positive_bytes, self.non_pii_bytes
                ),
            },
            "entities": {
                "gold": self.entities,
                "fully_covered": self.entities_fully_covered,
                "overlapped": self.entities_overlapped,
                "exact_boundary": self.entities_exact,
                "full_coverage_recall": safe_ratio(
                    self.entities_fully_covered, self.entities
                ),
                "overlap_recall": safe_ratio(self.entities_overlapped, self.entities),
                "exact_boundary_recall": safe_ratio(self.entities_exact, self.entities),
            },
            "prediction_spans": {
                "total": self.prediction_spans,
                "overlapped_gold": self.prediction_spans_overlapped,
                "overlap_precision": safe_ratio(
                    self.prediction_spans_overlapped, self.prediction_spans
                ),
            },
        }


class RecallAccumulator:
    def __init__(self) -> None:
        self.bytes = 0
        self.covered_bytes = 0
        self.entities = 0
        self.fully_covered = 0
        self.overlapped = 0

    def add(self, spans: Sequence[Span], predictions: Sequence[Span]) -> None:
        predicted = merge_intervals((span.start, span.end) for span in predictions)
        for span in spans:
            interval = (span.start, span.end)
            span_bytes = span.end - span.start
            self.bytes += span_bytes
            self.covered_bytes += intersection_length([interval], predicted)
            self.entities += 1
            self.fully_covered += interval_is_covered(interval, predicted)
            self.overlapped += interval_overlaps(interval, predicted)

    def result(self) -> dict[str, object]:
        return {
            "entities": self.entities,
            "fully_covered": self.fully_covered,
            "overlapped": self.overlapped,
            "full_coverage_recall": safe_ratio(self.fully_covered, self.entities),
            "overlap_recall": safe_ratio(self.overlapped, self.entities),
            "utf8_bytes": self.bytes,
            "covered_utf8_bytes": self.covered_bytes,
            "byte_recall": safe_ratio(self.covered_bytes, self.bytes),
            "leaked_utf8_bytes": self.bytes - self.covered_bytes,
        }


class ContractAccumulator:
    INTEGRITY_ERROR_FIELDS = (
        "invalid_clean_bounds",
        "invalid_raw_bounds",
        "overlapping_clean_spans",
        "non_monotonic_raw_spans",
        "token_restore_failures",
        "raw_value_mismatches",
    )

    def __init__(self) -> None:
        self.documents = 0
        self.restore_exact_documents = 0
        self.restore_success_decisions = 0
        self.manifest_valid_documents = 0
        self.manifest_spans = 0
        self.integrity_errors: Counter[str] = Counter()
        self.initial_suspects = 0
        self.initial_actionable_suspects = 0
        self.initial_class_mismatches = 0
        self.strict_would_reject_documents = 0
        self.post_policy_scanned_documents = 0
        self.post_policy_zero_suspect_documents = 0
        self.post_policy_suspects = 0
        self.protection_trace_items = 0
        self.tokenize_actions = 0
        self.redact_actions = 0
        self.documents_with_redact = 0

    def add(self, response: dict[str, object]) -> None:
        self.documents += 1
        restore = response["restore"]
        integrity = response["manifest_integrity"]
        initial = response["initial_safety_net_stats"]
        assert isinstance(restore, dict)
        assert isinstance(integrity, dict)
        assert isinstance(initial, dict)

        self.restore_exact_documents += bool(restore["exact"])
        self.restore_success_decisions += restore["decision"] == "success"
        self.manifest_spans += int(integrity["spans"])
        document_integrity_errors = 0
        for field in self.INTEGRITY_ERROR_FIELDS:
            count = int(integrity[field])
            self.integrity_errors[field] += count
            document_integrity_errors += count
        self.manifest_valid_documents += document_integrity_errors == 0

        self.initial_suspects += int(initial["suspect_count"])
        self.initial_actionable_suspects += int(initial["uncovered_count"]) + int(
            initial["partial_bleed_count"]
        )
        self.initial_class_mismatches += int(initial["class_mismatch_count"])
        self.strict_would_reject_documents += bool(response["strict_would_reject"])
        trace = response.get("final_protection_trace", [])
        assert isinstance(trace, list)
        actions = [item["action"] for item in trace if isinstance(item, dict)]
        self.protection_trace_items += len(actions)
        self.tokenize_actions += actions.count("tokenize")
        self.redact_actions += actions.count("redact")
        self.documents_with_redact += "redact" in actions

        post_policy = response["post_policy_safety_net_stats"]
        if post_policy is not None:
            assert isinstance(post_policy, dict)
            self.post_policy_scanned_documents += 1
            suspects = int(post_policy["suspect_count"])
            self.post_policy_suspects += suspects
            self.post_policy_zero_suspect_documents += suspects == 0

    def result(self) -> dict[str, object]:
        return {
            "documents": self.documents,
            "restore_exact_documents": self.restore_exact_documents,
            "restore_exact_rate": safe_ratio(
                self.restore_exact_documents, self.documents
            ),
            "restore_success_decisions": self.restore_success_decisions,
            "restore_success_decision_rate": safe_ratio(
                self.restore_success_decisions, self.documents
            ),
            "manifest_spans": self.manifest_spans,
            "manifest_valid_documents": self.manifest_valid_documents,
            "manifest_valid_document_rate": safe_ratio(
                self.manifest_valid_documents, self.documents
            ),
            "manifest_integrity_errors": dict(sorted(self.integrity_errors.items())),
            "initial_safety_net_suspects": self.initial_suspects,
            "initial_actionable_suspects": self.initial_actionable_suspects,
            "initial_class_mismatches": self.initial_class_mismatches,
            "strict_would_reject_documents": self.strict_would_reject_documents,
            "strict_acceptance_rate": 1.0
            - safe_ratio(self.strict_would_reject_documents, self.documents),
            "post_policy_scanned_documents": self.post_policy_scanned_documents,
            "post_policy_suspects": self.post_policy_suspects,
            "final_protection_trace_items": self.protection_trace_items,
            "tokenize_actions": self.tokenize_actions,
            "redact_actions": self.redact_actions,
            "documents_with_redact": self.documents_with_redact,
            "post_policy_zero_suspect_documents": self.post_policy_zero_suspect_documents,
            "post_policy_zero_suspect_rate": (
                safe_ratio(
                    self.post_policy_zero_suspect_documents,
                    self.post_policy_scanned_documents,
                )
                if self.post_policy_scanned_documents
                else None
            ),
        }


def percentile(values: Sequence[float], quantile: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    position = (len(ordered) - 1) * quantile
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def timing_summary(values: Sequence[float]) -> dict[str, float]:
    return {
        "mean": statistics.fmean(values) if values else 0.0,
        "median": statistics.median(values) if values else 0.0,
        "p95": percentile(values, 0.95),
        "p99": percentile(values, 0.99),
        "max": max(values, default=0.0),
    }


def validate_prediction(document: Document, raw: dict[str, object]) -> Span:
    start = raw["raw_start"]
    end = raw["raw_end"]
    label = raw["class"]
    if (
        not isinstance(start, int)
        or not isinstance(end, int)
        or not isinstance(label, str)
    ):
        raise RuntimeError(f"{document.uid}: invalid prediction shape")
    text_bytes = len(document.text.encode("utf-8"))
    if start < 0 or end <= start or end > text_bytes:
        raise RuntimeError(f"{document.uid}: invalid prediction bounds {start}:{end}")
    return Span(start, end, label)


def final_trace_predictions(
    document: Document, response: dict[str, object]
) -> list[Span]:
    trace = response["final_protection_trace"]
    assert isinstance(trace, list)
    return [validate_prediction(document, item) for item in trace]


def validator_probe_binary(repo_root: Path) -> Path:
    return (
        repo_root
        / VALIDATOR_PROBE_TARGET
        / "debug"
        / "validator-recall-probe"
    )


def build_validator_probe(repo_root: Path) -> Path:
    manifest = repo_root / VALIDATOR_PROBE_MANIFEST
    target_dir = repo_root / VALIDATOR_PROBE_TARGET
    subprocess.run(
        [
            "rustup",
            "run",
            "1.96.0",
            "cargo",
            "build",
            "--locked",
            "--manifest-path",
            str(manifest),
            "--target-dir",
            str(target_dir),
        ],
        cwd=repo_root,
        check=True,
    )
    binary = validator_probe_binary(repo_root)
    if not binary.is_file():
        raise FileNotFoundError(f"validator recall probe is missing: {binary}")
    return binary


def _validate_validator_probe_handshake(value: object) -> dict[str, object]:
    payload = _expect_exact_keys(
        value,
        frozenset(
            {
                "schema_version",
                "validator_kinds_by_class",
                "validator_recognizers",
            }
        ),
        "validator probe handshake",
    )
    schema_version = _expect_int(
        payload["schema_version"], "validator probe handshake.schema_version", minimum=1
    )
    if schema_version != VALIDATOR_PROBE_PROTOCOL_SCHEMA_VERSION:
        raise ResponseValidationError(
            "validator probe handshake.schema_version must be "
            f"{VALIDATOR_PROBE_PROTOCOL_SCHEMA_VERSION}, got {schema_version}"
        )
    raw_inventory = _expect_object(
        payload["validator_kinds_by_class"],
        "validator probe handshake.validator_kinds_by_class",
    )
    inventory: dict[str, list[str]] = {}
    for class_name, raw_kinds in raw_inventory.items():
        canonical_class = _expect_string(
            class_name, "validator probe handshake validator class"
        )
        kinds = [
            _expect_string(
                item,
                f"validator probe handshake.validator_kinds_by_class[{canonical_class}]",
            )
            for item in _expect_list(
                raw_kinds,
                f"validator probe handshake.validator_kinds_by_class[{canonical_class}]",
            )
        ]
        if not kinds or kinds != sorted(set(kinds)):
            raise ResponseValidationError(
                "validator probe handshake validator kinds must be non-empty, "
                f"sorted, and unique for {canonical_class}"
            )
        inventory[canonical_class] = kinds
    recognizers: list[dict[str, str]] = []
    for index, raw_recognizer in enumerate(
        _expect_list(
            payload["validator_recognizers"],
            "validator probe handshake.validator_recognizers",
        )
    ):
        context = f"validator probe handshake.validator_recognizers[{index}]"
        recognizer = _expect_exact_keys(
            raw_recognizer,
            frozenset({"id", "class", "validator_kind"}),
            context,
        )
        item = {
            key: _expect_string(recognizer[key], f"{context}.{key}")
            for key in ("id", "class", "validator_kind")
        }
        if item["validator_kind"] not in inventory.get(item["class"], []):
            raise ResponseValidationError(
                f"{context} is inconsistent with validator_kinds_by_class"
            )
        recognizers.append(item)
    recognizer_ids = [item["id"] for item in recognizers]
    if not recognizers or recognizer_ids != sorted(set(recognizer_ids)):
        raise ResponseValidationError(
            "validator probe recognizers must be non-empty, sorted, and unique"
        )
    return {
        "schema_version": schema_version,
        "validator_kinds_by_class": inventory,
        "validator_recognizers": recognizers,
    }


def _validate_probe_prediction(
    document: Document, value: object, context: str
) -> dict[str, object]:
    prediction = _expect_exact_keys(
        value,
        frozenset({"start", "end", "class", "source_ids"}),
        context,
    )
    start = _expect_int(prediction["start"], f"{context}.start")
    end = _expect_int(prediction["end"], f"{context}.end")
    text_bytes = len(document.text.encode("utf-8"))
    if end <= start or end > text_bytes:
        raise ResponseValidationError(f"{context} has invalid raw bounds")
    class_name = _expect_string(prediction["class"], f"{context}.class")
    source_ids = [
        _expect_string(item, f"{context}.source_ids")
        for item in _expect_list(prediction["source_ids"], f"{context}.source_ids")
    ]
    if not source_ids or source_ids != sorted(set(source_ids)):
        raise ResponseValidationError(
            f"{context}.source_ids must be non-empty, sorted, and unique"
        )
    return {
        "start": start,
        "end": end,
        "class": class_name,
        "source_ids": source_ids,
    }


def _validate_validator_probe_response(
    document: Document, value: object, *, predictions_expected: bool
) -> dict[str, object]:
    payload = _expect_exact_keys(
        value,
        frozenset({"fixture_id", "gold_validation", "predictions"}),
        f"validator probe response for {document.uid}",
    )
    fixture_id = _expect_string(
        payload["fixture_id"], f"validator probe response for {document.uid}.fixture_id"
    )
    if fixture_id != document.uid:
        raise ResponseValidationError(
            f"validator probe fixture mismatch: expected {document.uid}, got {fixture_id}"
        )
    gold_validation: list[dict[str, object]] = []
    for index, raw_validation in enumerate(
        _expect_list(
            payload["gold_validation"],
            f"validator probe response for {document.uid}.gold_validation",
        )
    ):
        context = (
            f"validator probe response for {document.uid}.gold_validation[{index}]"
        )
        validation = _expect_exact_keys(
            raw_validation,
            frozenset(
                {
                    "start",
                    "end",
                    "label",
                    "applicable",
                    "validator_passed",
                }
            ),
            context,
        )
        applicable = _expect_bool(validation["applicable"], f"{context}.applicable")
        raw_passed = validation["validator_passed"]
        if applicable:
            validator_passed: bool | None = _expect_bool(
                raw_passed, f"{context}.validator_passed"
            )
        elif raw_passed is None:
            validator_passed = None
        else:
            raise ResponseValidationError(
                f"{context}.validator_passed must be null when not applicable"
            )
        gold_validation.append(
            {
                "start": _expect_int(validation["start"], f"{context}.start"),
                "end": _expect_int(validation["end"], f"{context}.end"),
                "label": _expect_string(validation["label"], f"{context}.label"),
                "applicable": applicable,
                "validator_passed": validator_passed,
            }
        )
    expected_gold = [
        {"start": span.start, "end": span.end, "label": span.label}
        for span in document.spans
    ]
    observed_gold = [
        {key: item[key] for key in ("start", "end", "label")}
        for item in gold_validation
    ]
    if observed_gold != expected_gold:
        raise ResponseValidationError(
            f"validator probe gold span mismatch for {document.uid}"
        )

    raw_predictions = payload["predictions"]
    predictions: dict[str, list[dict[str, object]]] | None
    if predictions_expected:
        prediction_pair = _expect_exact_keys(
            raw_predictions,
            frozenset({"validator_backed", "shape_only"}),
            f"validator probe response for {document.uid}.predictions",
        )
        predictions = {}
        for mode in ("validator_backed", "shape_only"):
            predictions[mode] = [
                _validate_probe_prediction(
                    document,
                    item,
                    (
                        f"validator probe response for {document.uid}."
                        f"predictions.{mode}[{index}]"
                    ),
                )
                for index, item in enumerate(
                    _expect_list(
                        prediction_pair[mode],
                        (
                            f"validator probe response for {document.uid}."
                            f"predictions.{mode}"
                        ),
                    )
                )
            ]
    elif raw_predictions is None:
        predictions = None
    else:
        raise ResponseValidationError(
            f"validator probe response for {document.uid}.predictions must be null"
        )
    return {
        "fixture_id": fixture_id,
        "gold_validation": gold_validation,
        "predictions": predictions,
    }


def collect_validator_measurements(
    probe_binary: Path,
    documents: Sequence[Document],
    measured_document_ids: Iterable[str],
) -> dict[str, object]:
    measured_ids = frozenset(measured_document_ids)
    document_ids = {document.uid for document in documents}
    unknown_ids = measured_ids - document_ids
    if unknown_ids:
        raise ValueError(
            "validator measurement IDs are outside the document population: "
            + ", ".join(sorted(unknown_ids))
        )
    process = subprocess.Popen(
        [str(probe_binary)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        bufsize=1,
    )
    assert process.stdin is not None
    assert process.stdout is not None
    assert process.stderr is not None
    handshake_line = process.stdout.readline()
    if not handshake_line:
        stderr = process.stderr.read()
        raise RuntimeError(
            "validator recall probe exited before its handshake"
            + (f": {stderr.strip()}" if stderr.strip() else "")
        )
    handshake = _validate_validator_probe_handshake(json.loads(handshake_line))
    responses: dict[str, dict[str, object]] = {}
    try:
        for document in documents:
            request = {
                "fixture_id": document.uid,
                "locale_chain": document.locale_chain,
                "text": document.text,
                "gold_spans": [
                    {
                        "start": span.start,
                        "end": span.end,
                        "label": span.label,
                        "applicable_classes": list(
                            VALIDATOR_CLASSES_BY_LABEL.get(span.label, ())
                        ),
                    }
                    for span in document.spans
                ],
                "measure_predictions": document.uid in measured_ids,
            }
            process.stdin.write(json.dumps(request, ensure_ascii=False) + "\n")
            process.stdin.flush()
            response_line = process.stdout.readline()
            if not response_line:
                stderr = process.stderr.read()
                raise RuntimeError(
                    f"validator recall probe exited before {document.uid}"
                    + (f": {stderr.strip()}" if stderr.strip() else "")
                )
            responses[document.uid] = _validate_validator_probe_response(
                document,
                json.loads(response_line),
                predictions_expected=document.uid in measured_ids,
            )
    finally:
        process.stdin.close()
        return_code = process.wait(timeout=30)
        stderr = process.stderr.read()
        process.stdout.close()
        process.stderr.close()
        if return_code != 0:
            raise RuntimeError(
                f"validator recall probe failed with status {return_code}"
                + (f": {stderr.strip()}" if stderr.strip() else "")
            )
    return {**handshake, "documents": responses}


def _validator_applicability(
    label: str, validator_measurements: Mapping[str, object]
) -> tuple[str, list[str], tuple[str, ...]]:
    classes = VALIDATOR_CLASSES_BY_LABEL.get(label)
    if classes is None:
        return "not_applicable", [], ()
    inventory = validator_measurements["validator_kinds_by_class"]
    assert isinstance(inventory, dict)
    validator_kinds = sorted(
        {
            kind
            for class_name in classes
            for kind in inventory.get(class_name, [])
            if isinstance(kind, str)
        }
    )
    if not validator_kinds:
        return "validator_not_configured", [], classes
    return "applicable", validator_kinds, classes


def validator_gold_census(
    documents: Sequence[Document], validator_measurements: Mapping[str, object]
) -> dict[str, dict[str, object]]:
    responses = validator_measurements["documents"]
    assert isinstance(responses, dict)
    labels = sorted({span.label for document in documents for span in document.spans})
    census: dict[str, dict[str, object]] = {}
    for label in labels:
        status, validator_kinds, _ = _validator_applicability(
            label, validator_measurements
        )
        validations = [
            validation
            for document in documents
            for validation in responses[document.uid]["gold_validation"]
            if validation["label"] == label
        ]
        expected_applicable = status == "applicable"
        if any(
            validation["applicable"] is not expected_applicable
            for validation in validations
        ):
            raise ResponseValidationError(
                f"validator applicability mismatch for corpus label {label}"
            )
        if status == "applicable":
            validator_passed = sum(
                validation["validator_passed"] is True for validation in validations
            )
            validator_failed = len(validations) - validator_passed
        else:
            validator_passed = None
            validator_failed = None
        census[label] = {
            "applicability": status,
            "validator_kinds": validator_kinds,
            "gold_spans": len(validations),
            "validator_passed_gold_spans": validator_passed,
            "validator_failed_gold_spans": validator_failed,
        }
    return census


def validator_recall_by_label(
    documents: Sequence[Document],
    scored_document_ids: Iterable[str],
    validator_measurements: Mapping[str, object],
) -> dict[str, dict[str, object]]:
    scored_ids = frozenset(scored_document_ids)
    responses = validator_measurements["documents"]
    assert isinstance(responses, dict)
    scored_documents = [document for document in documents if document.uid in scored_ids]
    labels = sorted(
        {span.label for document in scored_documents for span in document.spans}
    )
    result: dict[str, dict[str, object]] = {}
    for label in labels:
        status, validator_kinds, applicable_classes = _validator_applicability(
            label, validator_measurements
        )
        label_spans = [
            span
            for document in scored_documents
            for span in document.spans
            if span.label == label
        ]
        validations = [
            validation
            for document in scored_documents
            for validation in responses[document.uid]["gold_validation"]
            if validation["label"] == label
        ]
        expected_applicable = status == "applicable"
        if any(
            validation["applicable"] is not expected_applicable
            for validation in validations
        ):
            raise ResponseValidationError(
                f"validator applicability mismatch for scored label {label}"
            )
        if status == "applicable":
            validator_passed = sum(
                validation["validator_passed"] is True for validation in validations
            )
            validator_failed = len(validations) - validator_passed
            accumulators = {
                "validator_backed": RecallAccumulator(),
                "shape_only": RecallAccumulator(),
            }
            for document in scored_documents:
                spans = [span for span in document.spans if span.label == label]
                if not spans:
                    continue
                predictions = responses[document.uid]["predictions"]
                if not isinstance(predictions, dict):
                    raise ResponseValidationError(
                        f"validator predictions missing for {document.uid}"
                    )
                for mode, accumulator in accumulators.items():
                    relevant = [
                        Span(
                            start=int(prediction["start"]),
                            end=int(prediction["end"]),
                            label=str(prediction["class"]),
                        )
                        for prediction in predictions[mode]
                        if prediction["class"] in applicable_classes
                    ]
                    accumulator.add(spans, relevant)
            validator_backed_recall: dict[str, object] | None = accumulators[
                "validator_backed"
            ].result()
            shape_only_recall: dict[str, object] | None = accumulators[
                "shape_only"
            ].result()
        else:
            validator_passed = None
            validator_failed = None
            validator_backed_recall = None
            shape_only_recall = None
        result[label] = {
            "applicability": status,
            "validator_kinds": validator_kinds,
            "gold_spans": len(label_spans),
            "validator_passed_gold_spans": validator_passed,
            "validator_failed_gold_spans": validator_failed,
            "validator_backed_recall": validator_backed_recall,
            "shape_only_recall": shape_only_recall,
        }
    return result


def run_config(
    repo_root: Path,
    binary: Path,
    config: str,
    documents: Sequence[Document],
    model_dir: Path,
    kiji_model_dir: Path,
    opf_command: Path | None,
    opf_checkpoint: Path | None,
    opf_daemon_socket: Path | None,
    threshold: float,
    diagnostics_dir: Path,
    base_environment: Mapping[str, str] | None = None,
    warmup_count: int = 0,
    validator_measurements: Mapping[str, object] | None = None,
) -> dict[str, object]:
    if not documents:
        raise ValueError(f"{config}: cannot run an empty document cell")
    attempted_document_ids = [document.uid for document in documents]
    if len(set(attempted_document_ids)) != len(attempted_document_ids):
        raise ValueError(f"{config}: document IDs must be unique within a run")
    if warmup_count < 0:
        raise ValueError("warmup_count must be non-negative")
    environment = (
        dict(base_environment) if base_environment is not None else os.environ.copy()
    )
    environment["GAZE_NER_MODEL_DIR"] = str(model_dir)
    environment["GAZE_NER_THRESHOLD"] = str(threshold)
    environment["GAZE_KIJI_DISTILBERT_MODEL_DIR"] = str(kiji_model_dir)
    if opf_command is not None:
        environment["GAZE_OPENAI_FILTER_OPF"] = str(opf_command)
    if opf_checkpoint is not None:
        environment["OPF_CHECKPOINT"] = str(opf_checkpoint)
    if opf_daemon_socket is not None:
        environment["GAZE_OPF_DAEMON_SOCKET"] = str(opf_daemon_socket)
    command = [str(binary), "--config", config]
    diagnostics_dir.mkdir(parents=True, exist_ok=True)
    stderr_path = diagnostics_dir / f"{config}.stderr.log"
    stderr_handle = stderr_path.open("w", encoding="utf-8")
    started = time.perf_counter()
    process = subprocess.Popen(
        command,
        cwd=repo_root,
        env=environment,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=stderr_handle,
        text=True,
        encoding="utf-8",
        bufsize=1,
    )
    assert process.stdin is not None
    assert process.stdout is not None

    overall = MetricAccumulator()
    per_language: defaultdict[str, MetricAccumulator] = defaultdict(MetricAccumulator)
    per_label: defaultdict[str, RecallAccumulator] = defaultdict(RecallAccumulator)
    per_negative_category: defaultdict[str, MetricAccumulator] = defaultdict(
        MetricAccumulator
    )
    direct = RecallAccumulator()
    contextual = RecallAccumulator()
    contract = ContractAccumulator()
    scored_document_ids: list[str] = []
    failed_closed_documents: list[dict[str, str]] = []
    success_timing: defaultdict[str, list[float]] = defaultdict(list)
    first_response_ms: float | None = None
    discarded_warmup_samples: list[dict[str, object]] = []

    def exchange(document: Document) -> tuple[dict[str, object], float]:
        nonlocal first_response_ms
        request = {
            "fixture_id": document.uid,
            "locale_chain": document.locale_chain,
            "text": document.text,
        }
        request_started = time.perf_counter()
        process.stdin.write(json.dumps(request, ensure_ascii=False) + "\n")
        process.stdin.flush()
        response_line = process.stdout.readline()
        if not response_line:
            return_code = process.poll()
            raise RuntimeError(
                f"benchmark runner exited before {document.uid}; status={return_code}"
            )
        response = validate_response(document, json.loads(response_line))
        validated_at = time.perf_counter()
        if first_response_ms is None:
            first_response_ms = (validated_at - started) * 1000.0
        return response, (validated_at - request_started) * 1000.0

    try:
        for warmup_index in range(warmup_count):
            warmup_document = documents[warmup_index % len(documents)]
            response, round_trip_ms = exchange(warmup_document)
            if "pipeline_error_code" in response:
                timing = response["timing"]
                assert isinstance(timing, dict)
                discarded_warmup_samples.append(
                    {
                        "sample": warmup_index + 1,
                        "round_trip_ms": round_trip_ms,
                        "pipeline_error_code": response["pipeline_error_code"],
                        "pipeline_error_stage": response["pipeline_error_stage"],
                        "total_ms": timing["total_ms"],
                    }
                )
                continue
            timing = response["timing"]
            assert isinstance(timing, dict)
            warmup_metrics = MetricAccumulator()
            warmup_metrics.add(
                warmup_document,
                final_trace_predictions(warmup_document, response),
            )
            warmup_contract = ContractAccumulator()
            warmup_contract.add(response)
            metric_result = warmup_metrics.result()
            contract_result = warmup_contract.result()
            warmup_correctness = {
                "leaked_labeled_utf8_bytes": metric_result["utf8_bytes"]["leaked"],
                "uncovered_entities": (
                    metric_result["entities"]["gold"]
                    - metric_result["entities"]["fully_covered"]
                ),
                "restore_failures": 1 - contract_result["restore_exact_documents"],
                "manifest_invalid_documents": 1
                - contract_result["manifest_valid_documents"],
                "strict_rejections": contract_result["strict_would_reject_documents"],
                "residual_suspects": contract_result["post_policy_suspects"],
                "redact_actions": contract_result["redact_actions"],
            }
            discarded_warmup_samples.append(
                {
                    "sample": warmup_index + 1,
                    "round_trip_ms": round_trip_ms,
                    "clean_ms": timing["clean_ms"],
                    "restore_ms": timing["restore_ms"],
                    "post_policy_scan_ms": timing["post_policy_scan_ms"],
                    "correctness": warmup_correctness,
                }
            )
        for index, document in enumerate(documents):
            response, _ = exchange(document)
            if "pipeline_error_code" in response:
                failed_closed_documents.append(
                    {
                        "document_id": document.uid,
                        "reason": str(response["pipeline_error_code"]),
                        "stage": str(response["pipeline_error_stage"]),
                    }
                )
                continue
            scored_document_ids.append(document.uid)
            predictions = final_trace_predictions(document, response)
            overall.add(document, predictions)
            per_language[document.language].add(document, predictions)
            if document.negative_category is not None:
                per_negative_category[document.negative_category].add(
                    document, predictions
                )
            direct_spans = [
                span
                for span in document.spans
                if span.label in DIRECT_IDENTIFIER_LABELS
            ]
            contextual_spans = [
                span
                for span in document.spans
                if span.label not in DIRECT_IDENTIFIER_LABELS
            ]
            direct.add(direct_spans, predictions)
            contextual.add(contextual_spans, predictions)
            contract.add(response)
            for label in {span.label for span in document.spans}:
                per_label[label].add(
                    [span for span in document.spans if span.label == label],
                    predictions,
                )
            for key, value in response["timing"].items():
                if value is not None:
                    success_timing[key].append(float(value))
            if (index + 1) % 500 == 0:
                print(
                    f"{config}: scored {index + 1}/{len(documents)} documents",
                    file=sys.stderr,
                )
    finally:
        process.stdin.close()
        return_code = process.wait(timeout=30)
        stderr_handle.close()
        if return_code != 0:
            raise RuntimeError(f"benchmark runner failed with status {return_code}")

    wall_seconds = time.perf_counter() - started
    clean_ms = success_timing.get("clean_ms", [])
    measured_clean_seconds = sum(clean_ms) / 1000.0
    production_documents_per_second = (
        len(clean_ms) / measured_clean_seconds
        if clean_ms and measured_clean_seconds > 0.0
        else None
    )
    failed_closed_documents.sort(key=lambda item: item["document_id"])
    failed_closed_document_ids = [
        item["document_id"] for item in failed_closed_documents
    ]
    pipeline_errors = Counter(item["reason"] for item in failed_closed_documents)
    pipeline_error_stages = Counter(item["stage"] for item in failed_closed_documents)
    scored_population = identified_document_population(scored_document_ids)
    failed_closed_population = {
        **identified_document_population(failed_closed_document_ids),
        "excluded_documents": failed_closed_documents,
        "reason_counts": dict(sorted(pipeline_errors.items())),
        "stage_counts": dict(sorted(pipeline_error_stages.items())),
    }
    failed_closed_count = len(failed_closed_documents)
    return {
        "config": config,
        "scored_population": scored_population,
        "failed_closed_population": failed_closed_population,
        "metrics": overall.result(),
        "direct_identifier_recall": direct.result(),
        "contextual_pii_recall": contextual.result(),
        "pipeline_contract": contract.result(),
        "pipeline_availability": {
            "attempted_documents": len(documents),
            "completed_documents": len(documents) - failed_closed_count,
            "completion_rate": safe_ratio(
                len(documents) - failed_closed_count, len(documents)
            ),
            "failed_closed_documents": failed_closed_count,
            "errors": dict(sorted(pipeline_errors.items())),
            "error_stages": dict(sorted(pipeline_error_stages.items())),
        },
        "per_language": {
            key: value.result() for key, value in sorted(per_language.items())
        },
        "per_label_recall": {
            key: value.result() for key, value in sorted(per_label.items())
        },
        **(
            {
                "validator_recall_by_label": validator_recall_by_label(
                    documents,
                    scored_document_ids,
                    validator_measurements,
                )
            }
            if validator_measurements is not None
            else {}
        ),
        "per_negative_category": {
            key: value.result() for key, value in sorted(per_negative_category.items())
        },
        "latency_ms": {
            key: timing_summary(value) for key, value in sorted(success_timing.items())
        },
        "warm_latency_ms": {
            key: timing_summary(value[1:])
            for key, value in sorted(success_timing.items())
            if len(value) > 1
        },
        "process": {
            "wall_seconds": wall_seconds,
            "documents_per_second": production_documents_per_second,
            "start_to_first_response_ms": first_response_ms,
            "cold_start_to_first_validated_response_ms": first_response_ms,
            "warmup_count": warmup_count,
            "discarded_warmup_samples": discarded_warmup_samples,
            "stderr_log": str(stderr_path.relative_to(repo_root)),
            "stderr_bytes": stderr_path.stat().st_size,
        },
    }


def git_metadata(repo_root: Path) -> dict[str, object]:
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo_root,
        check=True,
        text=True,
        capture_output=True,
    ).stdout.strip()
    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=repo_root,
        check=True,
        text=True,
        capture_output=True,
    ).stdout
    return {"revision": revision, "dirty": bool(status.strip())}


SCORING_METADATA: dict[str, object] = {
    "primary_unit": "UTF-8 bytes",
    "primary_scope": "label-agnostic whole-pipeline PII pseudonymization",
    "adjacent_and_overlapping_spans_are_merged": True,
    "full_entity_coverage_requires_every_byte": True,
    "direct_identifier_labels": sorted(DIRECT_IDENTIFIER_LABELS),
    "non_compensating_axes": [
        "safety",
        "reversibility",
        "manifest_integrity",
        "strict_availability",
        "precision",
        "latency",
    ],
    "notes": [
        "All labeled PII bytes are included in the primary safety score.",
        "A protected byte counts for safety even when its action is not reversible.",
        "Restore, manifest, pipeline, and telemetry failures remain separate hard failures.",
        "Final protection evidence contains offsets, classes, counts, and stable IDs only; never PII values.",
        "Per-cell scored and failed-closed populations are sorted synthetic document-ID sets with SHA-256 digests.",
        "Failed-closed reason and stage counts reconcile exactly to the identified excluded documents.",
        "Validator-backed and validator-bypassed shape-only recall are additive diagnostics and do not alter existing counters.",
        "Labels without an applicable validator are explicitly not_applicable; null pass/fail and recall fields are never numeric zero.",
        "Latency is considered only after correctness gates pass.",
    ],
    "comparison_gates": {
        "correctness": "zero-tolerance integer-count ratchets; ratios are diagnostics only",
        "release_readiness": "production cell must have zero correctness failures",
        "population": (
            "candidate and baseline dataset provenance plus per-cell scored and "
            "failed-closed document sets must match"
        ),
        "performance": (
            "clean_ms p95 uses a separately configured tolerance and is "
            "informational by default"
        ),
    },
}


def assemble_scorecard(
    *,
    repo_root: Path,
    dataset_metadata: Mapping[str, object],
    dataset_report: Mapping[str, object],
    sampling_report: Mapping[str, object],
    parameters: Mapping[str, object],
    runs: Sequence[Mapping[str, object]],
) -> dict[str, object]:
    available_population = copy.deepcopy(sampling_report["available_population"])
    evaluated_population = copy.deepcopy(sampling_report["evaluated_population"])
    dataset = copy.deepcopy(dict(dataset_metadata))
    dataset.update(
        {
            key: copy.deepcopy(value)
            for key, value in dataset_report.items()
            if key != "selection"
        }
    )
    dataset.update(
        {
            "available_population": available_population,
            "evaluated_population": evaluated_population,
            "selection": copy.deepcopy(evaluated_population),
            "sampling": {
                key: copy.deepcopy(value)
                for key, value in sampling_report.items()
                if key not in {"available_population", "evaluated_population"}
            },
        }
    )
    return {
        "schema_version": SCORECARD_SCHEMA_VERSION,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "gaze": git_metadata(repo_root),
        "dataset": dataset,
        "scoring": copy.deepcopy(SCORING_METADATA),
        "parameters": copy.deepcopy(dict(parameters)),
        "runs": [copy.deepcopy(dict(run)) for run in runs],
    }


def _scorecard_runs(scorecard: Mapping[str, object]) -> dict[str, Mapping[str, object]]:
    if type(scorecard.get("schema_version")) is not int:
        raise ValueError("scorecard schema_version must be an integer")
    if scorecard["schema_version"] not in READABLE_SCORECARD_SCHEMA_VERSIONS:
        raise ValueError(
            f"unsupported scorecard schema_version {scorecard['schema_version']}"
        )
    raw_runs = scorecard.get("runs")
    if not isinstance(raw_runs, list):
        raise ValueError("scorecard runs must be an array")
    result: dict[str, Mapping[str, object]] = {}
    for raw_run in raw_runs:
        if not isinstance(raw_run, dict) or not isinstance(raw_run.get("config"), str):
            raise ValueError("scorecard run must be an object with a string config")
        config = raw_run["config"]
        if config in result:
            raise ValueError(f"duplicate scorecard config {config}")
        result[config] = raw_run
    return result


def _evaluated_population_provenance(
    scorecard: Mapping[str, object],
) -> dict[str, object]:
    dataset = scorecard.get("dataset")
    if not isinstance(dataset, dict):
        raise ValueError("scorecard dataset must be an object")
    evaluated = dataset.get("evaluated_population")
    sampling = dataset.get("sampling")
    integrity = dataset.get("integrity")
    if not isinstance(evaluated, dict) or not isinstance(sampling, dict):
        raise ValueError("scorecard evaluated-population provenance is missing")
    if not isinstance(integrity, dict):
        raise ValueError("scorecard dataset integrity must be an object")

    document_count = evaluated.get("documents")
    evaluated_ids = sampling.get("evaluated_document_ids")
    digest = sampling.get("evaluated_document_ids_digest")
    if type(document_count) is not int or document_count <= 0:
        raise ValueError("evaluated population must contain documents")
    if not isinstance(evaluated_ids, list) or not all(
        isinstance(uid, str) and uid for uid in evaluated_ids
    ):
        raise ValueError("evaluated document IDs must be a non-empty string array")
    if len(evaluated_ids) != document_count or len(set(evaluated_ids)) != len(
        evaluated_ids
    ):
        raise ValueError("evaluated document IDs disagree with the population")
    if not isinstance(digest, dict) or set(digest) != {"algorithm", "value"}:
        raise ValueError("evaluated document digest has an invalid shape")
    if digest.get("algorithm") != "sha256" or not isinstance(digest.get("value"), str):
        raise ValueError("evaluated document digest must use sha256")
    expected_digest = hashlib.sha256(
        json.dumps(evaluated_ids, ensure_ascii=False, separators=(",", ":")).encode(
            "utf-8"
        )
    ).hexdigest()
    if digest["value"] != expected_digest:
        raise ValueError("evaluated document digest does not match the IDs")

    identity_fields = ("repository", "revision", "file")
    if not all(isinstance(dataset.get(field), str) for field in identity_fields):
        raise ValueError("scorecard dataset identity is incomplete")
    dataset_sha256 = integrity.get("sha256")
    if not isinstance(dataset_sha256, str) or not re.fullmatch(
        r"[0-9a-f]{64}", dataset_sha256
    ):
        raise ValueError("scorecard dataset sha256 is invalid")
    strategy = sampling.get("strategy")
    seed = sampling.get("seed")
    if strategy != SAMPLING_STRATEGY or type(seed) is not int:
        raise ValueError("scorecard sampling strategy or seed is invalid")

    return {
        "repository": dataset["repository"],
        "revision": dataset["revision"],
        "file": dataset["file"],
        "sha256": dataset_sha256,
        "evaluated_population": evaluated,
        "sampling_strategy": strategy,
        "sampling_seed": seed,
        "evaluated_document_ids": evaluated_ids,
        "evaluated_document_ids_digest": digest,
    }


def _count(value: object, context: str) -> int:
    if type(value) is not int or value < 0:
        raise ValueError(f"{context} must be a non-negative integer")
    return value


def _count_map(value: object, context: str) -> dict[str, int]:
    if not isinstance(value, dict):
        raise ValueError(f"{context} must be an object")
    return {str(key): _count(item, f"{context}.{key}") for key, item in value.items()}


def _exact_object(
    value: object, required: frozenset[str], context: str
) -> dict[str, object]:
    if not isinstance(value, dict) or not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} must be an object")
    if set(value) != required:
        raise ValueError(
            f"{context} fields disagree: "
            f"missing={sorted(required - value.keys())}, "
            f"unknown={sorted(value.keys() - required)}"
        )
    return value


def _identified_population_ids(value: object, context: str) -> frozenset[str]:
    population = _exact_object(
        value,
        frozenset({"documents", "document_ids", "document_ids_digest"}),
        context,
    )
    documents = _count(population["documents"], f"{context}.documents")
    raw_ids = population["document_ids"]
    if not isinstance(raw_ids, list) or not all(
        isinstance(uid, str) and uid for uid in raw_ids
    ):
        raise ValueError(f"{context}.document_ids must be a string array")
    if raw_ids != sorted(raw_ids) or len(set(raw_ids)) != len(raw_ids):
        raise ValueError(f"{context}.document_ids must be a sorted duplicate-free set")
    if len(raw_ids) != documents:
        raise ValueError(f"{context}.document_ids disagree with documents")
    digest = population["document_ids_digest"]
    if not isinstance(digest, dict) or set(digest) != {"algorithm", "value"}:
        raise ValueError(f"{context}.document_ids_digest has an invalid shape")
    if digest != document_ids_digest(raw_ids):
        raise ValueError(f"{context}.document_ids_digest does not match the IDs")
    return frozenset(raw_ids)


def _run_population_provenance(
    run: Mapping[str, object],
    *,
    context: str,
    expected_document_ids: Iterable[str] | None = None,
) -> dict[str, frozenset[str]]:
    availability = run.get("pipeline_availability")
    if not isinstance(availability, dict):
        raise ValueError(f"{context}.pipeline_availability must be an object")
    completed = _count(
        availability.get("completed_documents"),
        f"{context}.pipeline_availability.completed_documents",
    )
    failed = _count(
        availability.get("failed_closed_documents"),
        f"{context}.pipeline_availability.failed_closed_documents",
    )
    attempted = _count(
        availability.get("attempted_documents"),
        f"{context}.pipeline_availability.attempted_documents",
    )
    scored_ids = _identified_population_ids(
        run.get("scored_population"), f"{context}.scored_population"
    )
    raw_failed_population = run.get("failed_closed_population")
    if not isinstance(raw_failed_population, dict):
        raise ValueError(f"{context}.failed_closed_population must be an object")
    failed_population = _exact_object(
        raw_failed_population,
        frozenset(
            {
                "documents",
                "document_ids",
                "document_ids_digest",
                "excluded_documents",
                "reason_counts",
                "stage_counts",
            }
        ),
        f"{context}.failed_closed_population",
    )
    failed_ids = _identified_population_ids(
        {
            key: failed_population[key]
            for key in ("documents", "document_ids", "document_ids_digest")
        },
        f"{context}.failed_closed_population",
    )
    exclusions = failed_population["excluded_documents"]
    if not isinstance(exclusions, list):
        raise ValueError(
            f"{context}.failed_closed_population.excluded_documents must be an array"
        )
    excluded_ids: list[str] = []
    observed_reasons: Counter[str] = Counter()
    observed_stages: Counter[str] = Counter()
    for index, raw_exclusion in enumerate(exclusions):
        exclusion_context = (
            f"{context}.failed_closed_population.excluded_documents[{index}]"
        )
        exclusion = _exact_object(
            raw_exclusion,
            frozenset({"document_id", "reason", "stage"}),
            exclusion_context,
        )
        document_id = exclusion["document_id"]
        reason = exclusion["reason"]
        stage = exclusion["stage"]
        if not isinstance(document_id, str) or not document_id:
            raise ValueError(f"{exclusion_context}.document_id must be a string")
        if not isinstance(reason, str):
            raise ValueError(f"{exclusion_context}.reason must be a string")
        if not isinstance(stage, str):
            raise ValueError(f"{exclusion_context}.stage must be a string")
        if reason not in PIPELINE_FAILURE_REASONS:
            raise ValueError(f"{exclusion_context}.reason is not a closed reason")
        if stage not in PIPELINE_FAILURE_STAGES:
            raise ValueError(f"{exclusion_context}.stage is not a closed stage")
        excluded_ids.append(document_id)
        observed_reasons[reason] += 1
        observed_stages[stage] += 1
    if excluded_ids != sorted(excluded_ids) or len(set(excluded_ids)) != len(
        excluded_ids
    ):
        raise ValueError(
            f"{context}.failed_closed_population exclusions must be sorted and unique"
        )
    if frozenset(excluded_ids) != failed_ids:
        raise ValueError(
            f"{context}.failed_closed_population exclusions disagree with document IDs"
        )
    reason_counts = _count_map(
        failed_population["reason_counts"],
        f"{context}.failed_closed_population.reason_counts",
    )
    stage_counts = _count_map(
        failed_population["stage_counts"],
        f"{context}.failed_closed_population.stage_counts",
    )
    if set(reason_counts) - PIPELINE_FAILURE_REASONS:
        raise ValueError(
            f"{context}.failed_closed_population.reason_counts contains an unknown reason"
        )
    if set(stage_counts) - PIPELINE_FAILURE_STAGES:
        raise ValueError(
            f"{context}.failed_closed_population.stage_counts contains an unknown stage"
        )
    if reason_counts != dict(observed_reasons):
        raise ValueError(
            f"{context}.failed-closed reason counts do not reconcile to excluded documents"
        )
    if stage_counts != dict(observed_stages):
        raise ValueError(
            f"{context}.failed-closed stage counts do not reconcile to excluded documents"
        )
    availability_reasons = _count_map(
        availability.get("errors", {}), f"{context}.pipeline_availability.errors"
    )
    availability_stages = _count_map(
        availability.get("error_stages", {}),
        f"{context}.pipeline_availability.error_stages",
    )
    if reason_counts != availability_reasons or stage_counts != availability_stages:
        raise ValueError(
            f"{context}.failed-closed breakdown disagrees with pipeline availability"
        )
    if len(scored_ids) != completed or len(failed_ids) != failed:
        raise ValueError(f"{context}.population IDs disagree with availability counts")
    if scored_ids & failed_ids:
        raise ValueError(f"{context}.scored and failed-closed populations overlap")
    attempted_ids = scored_ids | failed_ids
    if len(attempted_ids) != attempted:
        raise ValueError(
            f"{context}.population IDs do not reconcile to attempted total"
        )
    if expected_document_ids is not None:
        expected_ids = frozenset(expected_document_ids)
        if attempted_ids != expected_ids:
            raise ValueError(
                f"{context}.attempted population disagrees with dataset evaluated IDs"
            )
    return {
        "scored_document_ids": scored_ids,
        "failed_closed_document_ids": failed_ids,
    }


def _run_correctness_counts(run: Mapping[str, object]) -> dict[str, int]:
    metrics = run["metrics"]
    contract = run["pipeline_contract"]
    availability = run["pipeline_availability"]
    if not all(isinstance(value, dict) for value in (metrics, contract, availability)):
        raise ValueError("scorecard run correctness sections must be objects")
    utf8_bytes = metrics["utf8_bytes"]
    entities = metrics["entities"]
    if not isinstance(utf8_bytes, dict) or not isinstance(entities, dict):
        raise ValueError("scorecard metric sections must be objects")
    attempted = _count(availability["attempted_documents"], "attempted_documents")
    completed = _count(availability["completed_documents"], "completed_documents")
    failed = _count(availability["failed_closed_documents"], "failed_closed_documents")
    errors = _count_map(availability.get("errors", {}), "pipeline errors")
    error_stages = _count_map(
        availability.get("error_stages", {}), "pipeline error stages"
    )
    if completed + failed != attempted:
        raise ValueError("pipeline attempted/completed/failed counts disagree")
    if sum(errors.values()) != failed or sum(error_stages.values()) != failed:
        raise ValueError("pipeline error telemetry disagrees with failed count")

    documents = _count(metrics["documents"], "metrics.documents")
    without_leaks = _count(
        metrics["documents_without_leaks"], "documents_without_leaks"
    )
    if documents != completed or without_leaks > documents:
        raise ValueError("metric document counts disagree with pipeline completion")
    contract_documents = _count(contract["documents"], "contract.documents")
    if contract_documents != completed:
        raise ValueError("contract document count disagrees with pipeline completion")

    gold_entities = _count(entities["gold"], "entities.gold")
    fully_covered = _count(entities["fully_covered"], "entities.fully_covered")
    if fully_covered > gold_entities:
        raise ValueError("fully covered entities exceed gold entities")
    pii_bytes = _count(utf8_bytes["pii"], "utf8_bytes.pii")
    leaked_bytes = _count(utf8_bytes["leaked"], "utf8_bytes.leaked")
    if leaked_bytes > pii_bytes:
        raise ValueError("leaked bytes exceed labeled PII bytes")

    restore_exact = _count(
        contract["restore_exact_documents"], "restore_exact_documents"
    )
    restore_success = _count(
        contract["restore_success_decisions"], "restore_success_decisions"
    )
    manifest_valid = _count(
        contract["manifest_valid_documents"], "manifest_valid_documents"
    )
    post_policy_scanned = _count(
        contract.get("post_policy_scanned_documents", 0),
        "post_policy_scanned_documents",
    )
    for name, value in (
        ("restore_exact_documents", restore_exact),
        ("restore_success_decisions", restore_success),
        ("manifest_valid_documents", manifest_valid),
        ("post_policy_scanned_documents", post_policy_scanned),
    ):
        if value > contract_documents:
            raise ValueError(f"{name} exceeds contract documents")
    integrity_errors = sum(
        _count_map(
            contract.get("manifest_integrity_errors", {}),
            "manifest_integrity_errors",
        ).values()
    )

    return {
        "attempted_documents": attempted,
        "completed_documents": completed,
        "failed_closed_documents": failed,
        "scored_documents": documents,
        "documents_without_leaks": without_leaks,
        "documents_with_leaks": documents - without_leaks,
        "pii_utf8_bytes": pii_bytes,
        "leaked_labeled_utf8_bytes": leaked_bytes,
        "gold_entities": gold_entities,
        "fully_covered_entities": fully_covered,
        "uncovered_entities": gold_entities - fully_covered,
        "false_positive_utf8_bytes": _count(
            utf8_bytes["false_positive"], "utf8_bytes.false_positive"
        ),
        "documents_with_false_positives": _count(
            metrics["documents_with_false_positives"],
            "documents_with_false_positives",
        ),
        "contract_documents": contract_documents,
        "restore_exact_documents": restore_exact,
        "restore_failures": contract_documents - restore_exact,
        "restore_success_decisions": restore_success,
        "restore_decision_failures": contract_documents - restore_success,
        "manifest_valid_documents": manifest_valid,
        "manifest_invalid_documents": contract_documents - manifest_valid,
        "manifest_integrity_errors": integrity_errors,
        "strict_rejections": _count(
            contract["strict_would_reject_documents"],
            "strict_would_reject_documents",
        ),
        "post_policy_scanned_documents": post_policy_scanned,
        "post_policy_unscanned_documents": contract_documents - post_policy_scanned,
        "residual_suspects": _count(
            contract.get("post_policy_suspects", 0), "post_policy_suspects"
        ),
        "redact_actions": _count(contract.get("redact_actions", 0), "redact_actions"),
    }


def evaluate_release_readiness(candidate: Mapping[str, object]) -> dict[str, object]:
    failures: list[dict[str, object]] = []
    evaluated_document_ids: list[str] | None = None
    try:
        candidate_runs = _scorecard_runs(candidate)
    except (KeyError, TypeError, ValueError) as error:
        return {
            "passed": False,
            "failures": [{"gate": "candidate_runs", "reason": str(error)}],
        }
    expected_configs = frozenset(DEFAULT_CONFIGS)
    if frozenset(candidate_runs) != expected_configs:
        failures.append(
            {
                "gate": "candidate_config_set",
                "expected": sorted(expected_configs),
                "actual": sorted(candidate_runs),
            }
        )
    try:
        population_provenance = _evaluated_population_provenance(candidate)
        evaluated_document_ids = list(population_provenance["evaluated_document_ids"])
    except (KeyError, TypeError, ValueError) as error:
        failures.append(
            {
                "gate": "candidate_evaluated_population_provenance",
                "reason": str(error),
            }
        )
    cell_counts: dict[str, dict[str, int]] = {}
    for config in sorted(expected_configs.intersection(candidate_runs)):
        try:
            counts = _run_correctness_counts(candidate_runs[config])
        except (KeyError, TypeError, ValueError) as error:
            failures.append(
                {
                    "config": config,
                    "gate": "candidate_integer_counts",
                    "reason": str(error),
                }
            )
            continue
        if evaluated_document_ids is not None:
            try:
                _run_population_provenance(
                    candidate_runs[config],
                    context=f"candidate {config}",
                    expected_document_ids=evaluated_document_ids,
                )
            except (KeyError, TypeError, ValueError) as error:
                failures.append(
                    {
                        "config": config,
                        "gate": "candidate_run_population_provenance",
                        "reason": str(error),
                    }
                )
        cell_counts[config] = counts
        for gate in (
            "failed_closed_documents",
            "restore_failures",
            "restore_decision_failures",
            "manifest_invalid_documents",
            "manifest_integrity_errors",
            "strict_rejections",
        ):
            if counts[gate] != 0:
                failures.append(
                    {
                        "config": config,
                        "gate": gate,
                        "actual": counts[gate],
                        "required": 0,
                    }
                )
        if counts["attempted_documents"] <= 0:
            failures.append({"config": config, "gate": "non_empty_run"})
    if PRODUCTION_CONFIG in candidate_runs:
        counts = cell_counts.get(PRODUCTION_CONFIG)
        if counts is not None:
            zero_targets = (
                "documents_with_leaks",
                "leaked_labeled_utf8_bytes",
                "uncovered_entities",
                "post_policy_unscanned_documents",
                "residual_suspects",
                "redact_actions",
            )
            for gate in zero_targets:
                if counts[gate] != 0:
                    failures.append(
                        {
                            "config": PRODUCTION_CONFIG,
                            "gate": gate,
                            "actual": counts[gate],
                            "required": 0,
                        }
                    )
    return {"passed": not failures, "failures": failures}


CLASS_COVERAGE_LOSS_GATE = "class_coverage_loss"
CLASS_POPULATION_EVALUABILITY_GATE = "class_population_evaluability"


def _label_coverage(
    run: Mapping[str, object], context: str
) -> dict[str, tuple[int, int, int, int]] | None:
    """Returns {label: (entities, fully_covered, overlapped, leaked_utf8_bytes)}.

    Returns None when the block is ABSENT, so that two scorecards which both lack
    it stay comparable; a PRESENT but malformed block still fails closed. Whether
    an absent block on only one side is acceptable is decided by the caller, which
    treats that asymmetry as a non-pass.
    """
    labels = run.get("per_label_recall")
    if labels is None:
        return None
    if not isinstance(labels, Mapping):
        raise ValueError(f"{context}: per_label_recall is not a table")
    out: dict[str, tuple[int, int, int, int]] = {}
    for name, payload in labels.items():
        if not isinstance(payload, Mapping):
            raise ValueError(f"{context}.per_label_recall[{name}]: not a table")
        values = []
        for field in ("entities", "fully_covered", "overlapped", "leaked_utf8_bytes"):
            value = payload.get(field)
            if not isinstance(value, int) or isinstance(value, bool):
                raise ValueError(
                    f"{context}.per_label_recall[{name}].{field}: expected an integer"
                )
            values.append(value)
        out[str(name)] = (values[0], values[1], values[2], values[3])
    return out


def _optional_scored_document_ids(
    run: Mapping[str, object], context: str
) -> frozenset[str] | None:
    if "scored_population" not in run:
        return None
    return _identified_population_ids(
        run["scored_population"], f"{context}.scored_population"
    )


def _class_coverage_failures(
    config: str,
    candidate_run: Mapping[str, object],
    baseline_run: Mapping[str, object],
) -> list[dict[str, object]]:
    """Flags any class that lost coverage on an identical scored-document set."""
    failures: list[dict[str, object]] = []
    candidate_labels = _label_coverage(candidate_run, f"candidate {config}")
    baseline_labels = _label_coverage(baseline_run, f"baseline {config}")
    if candidate_labels is None and baseline_labels is None:
        # Neither side carries per-label telemetry: nothing to compare, and the
        # symmetry means a candidate cannot dodge the gate by dropping the block.
        return failures
    if candidate_labels is None or baseline_labels is None:
        # Asymmetric telemetry IS a dodge, so it is a non-pass rather than a skip.
        failures.append(
            {
                "config": config,
                "gate": CLASS_COVERAGE_LOSS_GATE,
                "reason": "per_label_recall present on only one side, cannot evaluate",
                "candidate_has_per_label_recall": candidate_labels is not None,
                "baseline_has_per_label_recall": baseline_labels is not None,
            }
        )
        return failures
    candidate_population = _optional_scored_document_ids(
        candidate_run, f"candidate {config}"
    )
    baseline_population = _optional_scored_document_ids(
        baseline_run, f"baseline {config}"
    )
    labels = sorted(set(candidate_labels) | set(baseline_labels))
    if candidate_population is None or baseline_population is None:
        for label in labels:
            failures.append(
                {
                    "config": config,
                    "gate": CLASS_POPULATION_EVALUABILITY_GATE,
                    "label": label,
                    "reason": "scored_population_unidentified, cannot evaluate",
                    "candidate_entities": candidate_labels.get(label, (0, 0, 0, 0))[0],
                    "baseline_entities": baseline_labels.get(label, (0, 0, 0, 0))[0],
                }
            )
        return failures
    if candidate_population != baseline_population:
        added_document_ids = sorted(candidate_population - baseline_population)
        removed_document_ids = sorted(baseline_population - candidate_population)
        for label in labels:
            failures.append(
                {
                    "config": config,
                    "gate": CLASS_POPULATION_EVALUABILITY_GATE,
                    "label": label,
                    "reason": "population_moved, cannot evaluate",
                    "candidate_entities": candidate_labels.get(label, (0, 0, 0, 0))[0],
                    "baseline_entities": baseline_labels.get(label, (0, 0, 0, 0))[0],
                    "added_document_ids": added_document_ids,
                    "removed_document_ids": removed_document_ids,
                }
            )
        return failures
    for label in labels:
        if label not in candidate_labels or label not in baseline_labels:
            failures.append(
                {
                    "config": config,
                    "gate": CLASS_POPULATION_EVALUABILITY_GATE,
                    "label": label,
                    "reason": "label_missing_on_identical_scored_population",
                    "candidate_entities": candidate_labels.get(label, (0, 0, 0, 0))[0],
                    "baseline_entities": baseline_labels.get(label, (0, 0, 0, 0))[0],
                }
            )
            continue
        c_entities, c_full, c_overlap, c_leaked = candidate_labels[label]
        b_entities, b_full, b_overlap, b_leaked = baseline_labels[label]
        if c_entities != b_entities:
            failures.append(
                {
                    "config": config,
                    "gate": CLASS_POPULATION_EVALUABILITY_GATE,
                    "label": label,
                    "reason": "gold_count_mismatch_on_identical_scored_population",
                    "candidate_entities": c_entities,
                    "baseline_entities": b_entities,
                }
            )
            continue
        if c_full < b_full or c_overlap < b_overlap or c_leaked > b_leaked:
            failures.append(
                {
                    "config": config,
                    "gate": CLASS_COVERAGE_LOSS_GATE,
                    "label": label,
                    "entities": c_entities,
                    "baseline_fully_covered": b_full,
                    "candidate_fully_covered": c_full,
                    "fully_covered_delta": c_full - b_full,
                    "baseline_overlapped": b_overlap,
                    "candidate_overlapped": c_overlap,
                    "overlapped_delta": c_overlap - b_overlap,
                    "leaked_utf8_bytes_delta": c_leaked - b_leaked,
                }
            )
    return failures


def compare_scorecards(
    candidate: Mapping[str, object], baseline: Mapping[str, object]
) -> dict[str, object]:
    regression_failures: list[dict[str, object]] = []
    release_readiness = evaluate_release_readiness(candidate)
    try:
        candidate_runs = _scorecard_runs(candidate)
    except (KeyError, TypeError, ValueError) as error:
        candidate_runs = {}
        failure = {"gate": "candidate_runs", "reason": str(error)}
        regression_failures.append(failure)
    try:
        baseline_runs = _scorecard_runs(baseline)
    except (KeyError, TypeError, ValueError) as error:
        baseline_runs = {}
        failure = {"gate": "baseline_runs", "reason": str(error)}
        regression_failures.append(failure)
    expected_configs = frozenset(DEFAULT_CONFIGS)
    candidate_configs = frozenset(candidate_runs)
    baseline_configs = frozenset(baseline_runs)
    invariant_counts = (
        "attempted_documents",
        "pii_utf8_bytes",
        "gold_entities",
    )
    higher_is_better = (
        "completed_documents",
        "scored_documents",
        "documents_without_leaks",
        "fully_covered_entities",
        "restore_exact_documents",
        "restore_success_decisions",
        "manifest_valid_documents",
        "post_policy_scanned_documents",
    )
    lower_is_better = (
        "failed_closed_documents",
        "documents_with_leaks",
        "leaked_labeled_utf8_bytes",
        "uncovered_entities",
        "false_positive_utf8_bytes",
        "documents_with_false_positives",
        "restore_failures",
        "restore_decision_failures",
        "manifest_invalid_documents",
        "manifest_integrity_errors",
        "strict_rejections",
        "post_policy_unscanned_documents",
        "residual_suspects",
        "redact_actions",
    )

    if candidate_configs != expected_configs:
        failure = {
            "gate": "candidate_config_set",
            "expected": sorted(expected_configs),
            "actual": sorted(candidate_configs),
        }
        regression_failures.append(failure)
    if baseline_configs != expected_configs:
        failure = {
            "gate": "baseline_config_set",
            "expected": sorted(expected_configs),
            "actual": sorted(baseline_configs),
        }
        regression_failures.append(failure)

    candidate_provenance: dict[str, object] | None = None
    baseline_provenance: dict[str, object] | None = None
    try:
        candidate_provenance = _evaluated_population_provenance(candidate)
    except (KeyError, TypeError, ValueError) as error:
        failure = {
            "gate": "candidate_evaluated_population_provenance",
            "reason": str(error),
        }
        regression_failures.append(failure)
    try:
        baseline_provenance = _evaluated_population_provenance(baseline)
    except (KeyError, TypeError, ValueError) as error:
        failure = {
            "gate": "baseline_evaluated_population_provenance",
            "reason": str(error),
        }
        regression_failures.append(failure)
    if (
        candidate_provenance is not None
        and baseline_provenance is not None
        and candidate_provenance != baseline_provenance
    ):
        candidate_ids = frozenset(candidate_provenance["evaluated_document_ids"])
        baseline_ids = frozenset(baseline_provenance["evaluated_document_ids"])
        failure = {
            "gate": "evaluated_population_provenance_match",
            "reason": "candidate and baseline provenance differ",
            "added_document_ids": sorted(candidate_ids - baseline_ids),
            "removed_document_ids": sorted(baseline_ids - candidate_ids),
        }
        regression_failures.append(failure)

    prerequisites_match = (
        candidate_configs == expected_configs
        and baseline_configs == expected_configs
        and candidate_provenance is not None
        and candidate_provenance == baseline_provenance
    )
    if prerequisites_match:
        expected_document_ids = candidate_provenance["evaluated_document_ids"]
        for config in sorted(expected_configs):
            candidate_values: dict[str, int] | None = None
            baseline_values: dict[str, int] | None = None
            try:
                candidate_values = _run_correctness_counts(candidate_runs[config])
                baseline_values = _run_correctness_counts(baseline_runs[config])
            except (KeyError, TypeError, ValueError) as error:
                regression_failures.append(
                    {
                        "config": config,
                        "gate": "integer_counts",
                        "reason": str(error),
                    }
                )
            candidate_population: dict[str, frozenset[str]] | None = None
            baseline_population: dict[str, frozenset[str]] | None = None
            try:
                candidate_population = _run_population_provenance(
                    candidate_runs[config],
                    context=f"candidate {config}",
                    expected_document_ids=expected_document_ids,
                )
            except (KeyError, TypeError, ValueError) as error:
                regression_failures.append(
                    {
                        "config": config,
                        "gate": "candidate_run_population_provenance",
                        "reason": str(error),
                    }
                )
            try:
                baseline_population = _run_population_provenance(
                    baseline_runs[config],
                    context=f"baseline {config}",
                    expected_document_ids=expected_document_ids,
                )
            except (KeyError, TypeError, ValueError) as error:
                regression_failures.append(
                    {
                        "config": config,
                        "gate": "baseline_run_population_provenance",
                        "reason": str(error),
                    }
                )
            populations_match = (
                candidate_population is not None
                and baseline_population is not None
                and candidate_population == baseline_population
            )
            if candidate_population is not None and baseline_population is not None:
                for population_name in (
                    "scored_document_ids",
                    "failed_closed_document_ids",
                ):
                    candidate_ids = candidate_population[population_name]
                    baseline_ids = baseline_population[population_name]
                    if candidate_ids == baseline_ids:
                        continue
                    regression_failures.append(
                        {
                            "config": config,
                            "gate": (
                                "scored_population_identity_match"
                                if population_name == "scored_document_ids"
                                else "failed_closed_population_identity_match"
                            ),
                            "reason": "candidate and baseline document sets differ",
                            "added_document_ids": sorted(candidate_ids - baseline_ids),
                            "removed_document_ids": sorted(
                                baseline_ids - candidate_ids
                            ),
                        }
                    )
            if (
                candidate_values is not None
                and baseline_values is not None
                and populations_match
            ):
                for gate in invariant_counts:
                    if candidate_values[gate] != baseline_values[gate]:
                        regression_failures.append(
                            {
                                "config": config,
                                "gate": f"{gate}_match",
                                "candidate": candidate_values[gate],
                                "baseline": baseline_values[gate],
                            }
                        )
                for gate in higher_is_better:
                    if candidate_values[gate] < baseline_values[gate]:
                        regression_failures.append(
                            {
                                "config": config,
                                "gate": gate,
                                "candidate": candidate_values[gate],
                                "baseline": baseline_values[gate],
                            }
                        )
                for gate in lower_is_better:
                    if candidate_values[gate] > baseline_values[gate]:
                        regression_failures.append(
                            {
                                "config": config,
                                "gate": gate,
                                "candidate": candidate_values[gate],
                                "baseline": baseline_values[gate],
                            }
                        )
            try:
                regression_failures.extend(
                    _class_coverage_failures(
                        config, candidate_runs[config], baseline_runs[config]
                    )
                )
            except (KeyError, TypeError, ValueError) as error:
                regression_failures.append(
                    {
                        "config": config,
                        "gate": CLASS_COVERAGE_LOSS_GATE,
                        "reason": str(error),
                    }
                )

    return {
        "schema_version": SCORECARD_SCHEMA_VERSION,
        "regression": {
            "passed": not regression_failures,
            "failures": regression_failures,
        },
        "release_readiness": release_readiness,
    }


def compare_performance(
    candidate: Mapping[str, object],
    baseline: Mapping[str, object],
    *,
    tolerance_percent: float,
    gating: bool = False,
) -> dict[str, object]:
    if tolerance_percent < 0.0:
        raise ValueError("performance tolerance must be non-negative")
    failures: list[dict[str, object]] = []
    comparisons: list[dict[str, object]] = []
    try:
        candidate_runs = _scorecard_runs(candidate)
        baseline_runs = _scorecard_runs(baseline)
        for config in sorted(DEFAULT_CONFIGS):
            candidate_p95 = float(
                candidate_runs[config]["latency_ms"]["clean_ms"]["p95"]
            )
            baseline_p95 = float(baseline_runs[config]["latency_ms"]["clean_ms"]["p95"])
            limit = baseline_p95 * (1.0 + tolerance_percent / 100.0)
            comparison = {
                "config": config,
                "metric": "clean_ms.p95",
                "candidate": candidate_p95,
                "baseline": baseline_p95,
                "limit": limit,
            }
            comparisons.append(comparison)
            if candidate_p95 > limit:
                failures.append(copy.deepcopy(comparison))
    except (KeyError, TypeError, ValueError) as error:
        failures.append({"gate": "performance_data", "reason": str(error)})
    return {
        "passed": not failures,
        "gating": gating,
        "disposition": "gating" if gating else "informational",
        "tolerance_percent": tolerance_percent,
        "comparisons": comparisons,
        "failures": failures,
    }
