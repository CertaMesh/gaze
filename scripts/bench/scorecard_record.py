#!/usr/bin/env python3
"""Value-free benchmark observations and contract-independent scorecard replay."""

from __future__ import annotations

import argparse
import copy
import gzip
import hashlib
import json
import sys
from collections import defaultdict
from pathlib import Path
from typing import Mapping, Sequence

import gaze_bench_score as score
import agentic_layers as agentic


# v2: every trace item's provenance also carries typed lineage (`settlement`
# plus `contributions`) and each observation carries `candidate_events`, one
# per detected candidate (component ledger S1). v1 records stay readable.
SCHEMA_VERSION = 2
READABLE_SCHEMA_VERSIONS = frozenset({1, SCHEMA_VERSION})
# Closed vocabulary; a clean_for_bench test pins it to the Rust enums.
VOCABULARY = json.loads(
    (Path(__file__).resolve().parent / "trace-lineage-vocabulary.json").read_text(encoding="utf-8")
)
CONTRIBUTION_ROLES = frozenset(VOCABULARY["roles"])
# Roles whose recognizer found the item's bytes itself; the rest are lineage.
DETECTING_ROLES = frozenset(VOCABULARY["detecting_roles"])
SETTLEMENTS = frozenset(VOCABULARY["settlements"])
DEFEAT_KINDS = frozenset(VOCABULARY["defeat_kinds"])
OUTCOMES = frozenset(VOCABULARY["outcomes"])
CONFLICT_TIERS = frozenset(VOCABULARY["conflict_tiers"])
VALIDATOR_FAIL_REASONS = frozenset(VOCABULARY["validator_fail_reasons"])
CONTRIBUTION_FIELDS = frozenset({
    "recognizer_id", "role", "raw_start", "raw_end", "tier", "defeat_kind", "defeated_by",
})
EVENT_FIELDS = frozenset({
    "original", "recognizer_id", "class", "raw_start", "raw_end", "outcome",
    "selection_start", "selection_end", "settlement", "role", "tier", "defeat_kind",
    "winner", "veto_reason",
})
DEFEAT_FIELDS = ("tier", "defeat_kind", "defeated_by")


class RecordError(ValueError):
    pass


def has_lineage(item: Mapping[str, object]) -> bool:
    provenance = item["provenance"]
    present = {"settlement", "contributions"} & set(provenance)
    if present and present != {"settlement", "contributions"}:
        raise RecordError("trace provenance carries half a lineage")
    return bool(present)


def response_lineage(response: Mapping[str, object]) -> bool | None:
    """Whether a response carries v2 lineage; `None` for a pipeline error."""
    if response.get("refused") or "pipeline_error_code" in response:
        return None
    events = "candidate_events" in response
    for item in response.get("final_protection_trace") or ():
        if has_lineage(item) != events:
            raise RecordError(
                f"{response.get('fixture_id')}: some trace items carry lineage and some do not"
            )
    return events


def _span(start: object, end: object, utf8_bytes: int) -> bool:
    return type(start) is int and type(end) is int and 0 <= start < end <= utf8_bytes


def validate_lineage(item: Mapping[str, object], utf8_bytes: int, where: str) -> None:
    """One trace item's typed lineage is well formed. Metadata only: no values."""
    provenance = item["provenance"]
    settlement = provenance["settlement"]
    if settlement not in SETTLEMENTS:
        raise RecordError(f"{where}: unknown settlement {settlement!r}")
    if (settlement == "safety_net") != (provenance["stage"] == "safety_net"):
        raise RecordError(f"{where}: settlement {settlement!r} disagrees with stage")
    contributions = provenance["contributions"]
    if not isinstance(contributions, list) or not contributions:
        raise RecordError(f"{where}: trace item without contributions")
    for entry in contributions:
        if set(entry) != CONTRIBUTION_FIELDS:
            raise RecordError(f"{where}: contribution fields differ from the schema")
        role = entry["role"]
        if role not in CONTRIBUTION_ROLES:
            raise RecordError(f"{where}: unknown contribution role {role!r}")
        if not isinstance(entry["recognizer_id"], str) or not entry["recognizer_id"]:
            raise RecordError(f"{where}: contribution without a recognizer ID")
        start, end = entry["raw_start"], entry["raw_end"]
        if role == "derived_dependency":
            if start is not None or end is not None:
                raise RecordError(f"{where}: a derived dependency detected no span")
        elif not _span(start, end, utf8_bytes):
            raise RecordError(f"{where}: {role} contribution without its own span in the document")
        if role in DETECTING_ROLES and not (start < item["raw_end"] and item["raw_start"] < end):
            raise RecordError(f"{where}: {role} contribution does not overlap its trace item")
        defeat = [entry[key] for key in DEFEAT_FIELDS]
        if role == "defeated":
            if entry["tier"] not in CONFLICT_TIERS:
                raise RecordError(f"{where}: unknown conflict tier {entry['tier']!r}")
            if entry["defeat_kind"] not in DEFEAT_KINDS:
                raise RecordError(f"{where}: unknown defeat kind {entry['defeat_kind']!r}")
            if not isinstance(entry["defeated_by"], str) or not entry["defeated_by"]:
                raise RecordError(f"{where}: a defeat names no winner")
        elif any(value is not None for value in defeat):
            raise RecordError(f"{where}: defeat fields belong to exactly the defeated role")
    if not any(entry["role"] in DETECTING_ROLES for entry in contributions):
        raise RecordError(f"{where}: trace item without a detecting contribution")


def validate_candidate_events(events: object, utf8_bytes: int, where: str) -> None:
    """One document's candidate events are well formed and self-consistent.

    A pool candidate appears once per selection tree that reached it (it can
    lose in one and be recovered in another), is selected at most once, and is
    `unlinked` only when no tree reached it.
    """
    if not isinstance(events, list):
        raise RecordError(f"{where}: candidate events must be a list")
    originals: dict[int, list[Mapping[str, object]]] = {}
    for event in events:
        if set(event) != EVENT_FIELDS:
            raise RecordError(f"{where}: candidate event fields differ from the schema")
        outcome = event["outcome"]
        if outcome not in OUTCOMES:
            raise RecordError(f"{where}: unknown candidate outcome {outcome!r}")
        if not isinstance(event["recognizer_id"], str) or not event["recognizer_id"]:
            raise RecordError(f"{where}: candidate event without a recognizer ID")
        if not isinstance(event["class"], str) or not event["class"]:
            raise RecordError(f"{where}: candidate event without a class")
        if not _span(event["raw_start"], event["raw_end"], utf8_bytes):
            raise RecordError(f"{where}: candidate span outside the document")
        original = event["original"]
        if (original is None) != (outcome == "vetoed"):
            raise RecordError(f"{where}: exactly a vetoed candidate has no pool index")
        if original is not None:
            if type(original) is not int or original < 0:
                raise RecordError(f"{where}: candidate pool index malformed")
            originals.setdefault(original, []).append(event)
        placed = outcome in {"selected", "defeated"}
        expected = {
            "selection_start": placed, "selection_end": placed, "settlement": placed,
            "role": outcome == "selected", "tier": outcome == "defeated",
            "defeat_kind": outcome == "defeated", "winner": outcome == "defeated",
            "veto_reason": outcome == "vetoed",
        }
        for key, present in expected.items():
            if (event[key] is not None) != present:
                raise RecordError(f"{where}: {key} does not belong to a {outcome} candidate")
        if placed:
            if not _span(event["selection_start"], event["selection_end"], utf8_bytes):
                raise RecordError(f"{where}: selection span outside the document")
            if event["settlement"] not in SETTLEMENTS - {"residual", "safety_net"}:
                raise RecordError(f"{where}: unknown selection settlement {event['settlement']!r}")
        if outcome == "selected" and event["role"] not in DETECTING_ROLES:
            raise RecordError(f"{where}: a selected candidate has a detecting role")
        if outcome == "defeated":
            if event["tier"] not in CONFLICT_TIERS:
                raise RecordError(f"{where}: unknown conflict tier {event['tier']!r}")
            if event["defeat_kind"] not in DEFEAT_KINDS:
                raise RecordError(f"{where}: unknown defeat kind {event['defeat_kind']!r}")
        if outcome == "vetoed" and event["veto_reason"] not in VALIDATOR_FAIL_REASONS:
            raise RecordError(f"{where}: unknown veto reason {event['veto_reason']!r}")
    if sorted(originals) != list(range(len(originals))):
        raise RecordError(f"{where}: candidate pool indexes are not one per candidate")
    for placements in originals.values():
        outcomes = [event["outcome"] for event in placements]
        if "unlinked" in outcomes and len(outcomes) > 1:
            raise RecordError(f"{where}: an unlinked candidate has a placement")
        if outcomes.count("selected") > 1:
            raise RecordError(f"{where}: candidate selected twice")
        selections = [(event["selection_start"], event["selection_end"]) for event in placements]
        if len(set(selections)) != len(selections):
            raise RecordError(f"{where}: candidate placed twice in one selection")
    for event in events:
        if event["outcome"] == "defeated":
            winner = originals.get(event["winner"], ())
            if (event["winner"] == event["original"]
                    or not any(other["outcome"] in {"selected", "defeated"} for other in winner)):
                raise RecordError(f"{where}: a defeat's winner is not a placed candidate")


def validate_response_lineage(response: Mapping[str, object], utf8_bytes: int, where: str) -> None:
    for item in response.get("final_protection_trace") or ():
        validate_lineage(item, utf8_bytes, where)
    validate_candidate_events(response["candidate_events"], utf8_bytes, where)


def _write_rows(path: Path, rows: Sequence[Mapping[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0, compresslevel=9) as zipped:
            for row in rows:
                zipped.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")).encode("utf-8") + b"\n")


def _contract_row(contract: score.ScoredLabelContract | None) -> dict[str, object] | None:
    if contract is None:
        return None
    return {
        "id": contract.contract_id, "version": contract.version,
        "path": contract.path, "sha256": contract.sha256,
        "scored": sorted(contract.scored_labels) if contract.scored_labels is not None else None,
        "excluded": sorted(contract.excluded_labels),
        "neutral": sorted(contract.neutral_prediction_classes),
        "gold_gap": sorted([list(pair) for pair in contract.gold_gap.compatible])
        if contract.gold_gap else None,
    }


def _contract_from_row(row: object) -> score.ScoredLabelContract:
    if not isinstance(row, Mapping):
        raise RecordError("missing layer contract")
    try:
        return score.ScoredLabelContract(
            contract_id=row["id"], version=row["version"], path=row["path"],
            sha256=row["sha256"],
            scored_labels=frozenset(row["scored"]) if row["scored"] is not None else None,
            excluded_labels=frozenset(row["excluded"]),
            neutral_prediction_classes=frozenset(row["neutral"]),
            gold_gap=score.GoldGapRule(frozenset(tuple(pair) for pair in row["gold_gap"]))
            if row["gold_gap"] is not None else None,
        )
    except (KeyError, TypeError, ValueError) as error:
        raise RecordError("invalid layer contract") from error


def _spans(spans: Sequence[score.Span]) -> list[list[object]]:
    return [[span.start, span.end, span.label] for span in spans]


def _original_spans(document: score.Document) -> tuple[score.Span, ...]:
    return (*document.spans, *document.excluded_spans)


def _document_row(document: score.Document) -> dict[str, object]:
    return {
        "id": document.uid,
        "utf8_bytes": len(document.text.encode("utf-8")),
        "language": document.language,
        "region": document.region,
        "source_dataset": document.source_dataset,
        "negative_category": document.negative_category,
        "cell": document.cell,
        "gold": _spans(_original_spans(document)),
    }


def _layer_identity(
    generator: Mapping[str, object], documents: Sequence[Mapping[str, object]]
) -> dict[str, object]:
    try:
        corpus = generator["corpus_sha256"]
        version = generator["generator_version"]
        population = generator["documents_by_layer"]
        layer_rows = sorted(
            ([row["layer"], row["id"], row["gold"]]
             for row in documents if row["layer"] in {"A", "D", "R"}),
            key=lambda item: (item[0], item[1]),
        )
        counts = {layer: sum(row[0] == layer for row in layer_rows) for layer in ("A", "D", "R")}
        if counts != population or len(layer_rows) != generator["documents"]:
            raise RecordError("layer documents disagree with generator population")
        payload = {"generator_corpus_sha256": corpus, "generator_version": version,
                   "documents": layer_rows}
        digest = hashlib.sha256(json.dumps(
            payload, sort_keys=True, separators=(",", ":")
        ).encode("utf-8")).hexdigest()
        return {"generator_corpus_sha256": corpus, "generator_version": version,
                "documents": len(layer_rows), "sha256": digest}
    except (KeyError, TypeError, ValueError) as error:
        raise RecordError("invalid generator or layer document identity") from error


def _gap_evidence(document: score.Document, predictions: Sequence[score.Span]) -> list[list[object]]:
    text = document.text.encode("utf-8")
    gold = sorted(_original_spans(document), key=lambda span: (span.start, span.end, span.label))
    result: list[list[object]] = []
    for prediction in predictions:
        start, end = prediction.start, prediction.end
        while start < end and text[start] in score.GOLD_GAP_TRIM_BYTES:
            start += 1
        while end > start and text[end - 1] in score.GOLD_GAP_TRIM_BYTES:
            end -= 1
        matches = [
            [span.start, span.end, span.label]
            for span in gold
            if text[span.start:span.end] == text[start:end]
        ] if start < end else []
        result.append([
            prediction.start, prediction.end, prediction.label,
            start, end, score.gold_gap_on_word_boundary(text, start, end), matches,
        ])
    return result


def _compact_response(response: Mapping[str, object]) -> dict[str, object]:
    if "pipeline_error_code" in response:
        return {
            "fixture_id": response["fixture_id"],
            "pipeline_error_code": response["pipeline_error_code"],
            "pipeline_error_stage": response["pipeline_error_stage"],
            "refused": True,
            "fallback_redact": False,
        }
    return {
        "fixture_id": response["fixture_id"],
        "refused": False,
        "safety_net_mode": response.get("safety_net_mode"),
        "fallback_redact": any(
            item["provenance"]["decision"] == "fallback_redact"
            for item in response["final_protection_trace"]
        ),
        "final_protection_trace": [
            {key: item[key] for key in ("raw_start", "raw_end", "class", "action", "provenance")}
            for item in response["final_protection_trace"]
        ],
        **({"candidate_events": response["candidate_events"]}
           if "candidate_events" in response else {}),
        "restore": response["restore"],
        "manifest_integrity": response["manifest_integrity"],
        "initial_safety_net_stats": response["initial_safety_net_stats"],
        "strict_would_reject": response["strict_would_reject"],
        "post_policy_safety_net_stats": response["post_policy_safety_net_stats"],
    }


class RecordWriter:
    def __init__(
        self,
        available: Sequence[score.Document],
        measurements: Mapping[str, object],
        *,
        corpus_sha256: str,
        extra_documents: Sequence[score.Document] = (),
        layer_contract: score.ScoredLabelContract | None = None,
    ) -> None:
        self.documents = {document.uid: document for document in (*available, *extra_documents)}
        if len(self.documents) != len(available) + len(extra_documents):
            raise RecordError("available document IDs are not unique")
        self.document_layers = {document.uid: "C" for document in available}
        self.document_validators = {
            document.uid: measurements["documents"][document.uid] for document in available
        }
        self.measurements = measurements
        self.corpus_sha256 = corpus_sha256
        self.layer_contract = layer_contract
        self.layer_measurements: dict[str, Mapping[str, object]] = {}
        self.rows: list[dict[str, object]] = []
        # None until the first trace item: whether the producer emits lineage.
        self.lineage: bool | None = None

    @property
    def schema_version(self) -> int:
        # A producer without typed lineage (an older tag) still writes a v1 record.
        return SCHEMA_VERSION if self.lineage else 1

    def add(
        self,
        layer: str,
        config: str,
        document: score.Document,
        response: Mapping[str, object],
        validator_measurements: Mapping[str, object] | None = None,
    ) -> None:
        original = self.documents.get(document.uid)
        if original is None:
            raise RecordError(f"unknown observation document {document.uid}")
        prior_layer = self.document_layers.setdefault(document.uid, layer)
        if prior_layer != layer:
            raise RecordError(f"{document.uid}: observation layer changed")
        measurement = self.layer_measurements.get(layer, validator_measurements)
        if layer != "C" and measurement is not None:
            validator = measurement["documents"][document.uid]
            previous = self.document_validators.setdefault(document.uid, validator)
            if previous != validator:
                raise RecordError(f"{document.uid}: validator evidence changed")
        compact = _compact_response(response)
        lineage = response_lineage(compact)
        if lineage is not None:
            if self.lineage is not None and lineage != self.lineage:
                raise RecordError(f"{document.uid}: some responses carry lineage and some do not")
            self.lineage = lineage
            if lineage:
                validate_response_lineage(compact, len(original.text.encode("utf-8")), document.uid)
        if "pipeline_error_code" not in response:
            compact["gold_gap_evidence"] = _gap_evidence(
                original, score.final_trace_predictions(original, dict(response))
            )
        row = {
            "kind": "observation",
            "layer": layer,
            "config": config,
            "document_id": document.uid,
            "response": compact,
        }
        self.rows.append(row)

    def write(self, path: Path, scorecard: Mapping[str, object], *, add_reference: bool) -> dict[str, object]:
        if set(self.document_layers) != set(self.documents):
            raise RecordError("some generated documents have no observation layer")
        documents = [
            {
                **_document_row(document),
                "layer": self.document_layers[document.uid],
                "validator": self.document_validators.get(document.uid),
            }
            for document in sorted(self.documents.values(), key=lambda item: item.uid)
        ]
        header = {
            "kind": "header",
            "schema_version": self.schema_version,
            "corpus_sha256": self.corpus_sha256,
            "scorecard": scorecard,
            "add_reference": add_reference,
            "validator": {
                key: value for key, value in self.measurements.items() if key != "documents"
            },
            "layer_contract": _contract_row(self.layer_contract),
            "documents": documents,
            "layer_identity": _layer_identity(scorecard["layers"]["generator"], documents)
            if "layers" in scorecard else None,
        }
        rows = [header, *self.rows]
        _write_rows(path, rows)
        digest = score.sha256_file(path)
        return {
            "file": path.name,
            "sha256": digest,
            "format": "gzip-jsonl",
            "schema_version": self.schema_version,
            "corpus_sha256": self.corpus_sha256,
            "observations": len(self.rows),
            "bytes": path.stat().st_size,
        }


def _document_from_row(row: Mapping[str, object]) -> score.Document:
    length = row["utf8_bytes"]
    if type(length) is not int or length < 0:
        raise RecordError("invalid document byte length")
    spans = tuple(score.Span(*item) for item in row["gold"])
    if any(type(span.start) is not int or type(span.end) is not int or not isinstance(span.label, str)
           or span.start < 0 or span.end <= span.start or span.end > length for span in spans):
        raise RecordError(f"{row['id']}: invalid gold bounds")
    return score.Document(
        uid=row["id"], text=" " * length, language=row["language"],
        region=row["region"], source_dataset=row["source_dataset"], spans=spans,
        negative_category=row["negative_category"], cell=row["cell"],
    )


def _filtered_measurements(
    inventory: Mapping[str, object], rows: Mapping[str, Mapping[str, object]],
    documents: Sequence[score.Document],
) -> dict[str, object]:
    responses: dict[str, object] = {}
    for document in documents:
        original = rows[document.uid]
        validator = copy.deepcopy(original["validator"])
        scored = {(span.start, span.end, span.label) for span in document.spans}
        validator["gold_validation"] = [
            item for item in validator["gold_validation"]
            if (item["start"], item["end"], item["label"]) in scored
        ]
        responses[document.uid] = validator
    return {**inventory, "documents": responses}


def filter_measurements(
    measurements: Mapping[str, object], documents: Sequence[score.Document]
) -> dict[str, object]:
    rows = {
        document.uid: {"validator": measurements["documents"][document.uid]}
        for document in documents
    }
    inventory = {key: value for key, value in measurements.items() if key != "documents"}
    return _filtered_measurements(inventory, rows, documents)


def _contract_documents(
    rows: Mapping[str, Mapping[str, object]], contract: score.ScoredLabelContract
) -> list[score.Document]:
    return score.apply_scored_label_contract(
        [_document_from_row(row) for row in rows.values()], contract
    )


def _evidence(
    recorded_response: Mapping[str, object], document: score.Document, response: Mapping[str, object]
) -> dict[tuple[int, int, str], tuple[int, int, bool, tuple[tuple[int, int, str], ...]]]:
    result = {}
    gold = {(span.start, span.end, span.label) for span in _original_spans(document)}
    for start, end, label, trimmed_start, trimmed_end, boundary, matches in recorded_response.get("gold_gap_evidence", []):
        key = (start, end, label)
        if (key in result or type(trimmed_start) is not int or type(trimmed_end) is not int
                or not start <= trimmed_start <= trimmed_end <= end
                or type(boundary) is not bool):
            raise RecordError(f"{document.uid}: invalid gold-gap evidence bounds")
        identities = tuple(tuple(match) for match in matches)
        if any(identity not in gold for identity in identities):
            raise RecordError(f"{document.uid}: gold-gap evidence names unknown gold")
        result[key] = (trimmed_start, trimmed_end, boundary, identities)
    trace_keys = {
        (item["raw_start"], item["raw_end"], item["class"])
        for item in response["final_protection_trace"]
    }
    if set(result) != trace_keys or len(trace_keys) != len(response["final_protection_trace"]):
        raise RecordError(f"{document.uid}: gold-gap evidence and trace disagree")
    return result


def _check_lineage(header: Mapping[str, object], observations: Sequence[Mapping[str, object]]) -> None:
    """A v2 record carries lineage on every trace item; a v1 record on none."""
    sizes = {row["id"]: row["utf8_bytes"] for row in header.get("documents") or ()}
    want = header["schema_version"] >= 2
    for row in observations:
        lineage = response_lineage(row["response"])
        if lineage is None:
            continue
        if lineage != want:
            raise RecordError(
                f"{row['document_id']}: record schema v{header['schema_version']} "
                "disagrees with its trace lineage"
            )
        if want:
            validate_response_lineage(row["response"], sizes[row["document_id"]], row["document_id"])


def _read(path: Path) -> tuple[dict[str, object], list[dict[str, object]]]:
    with gzip.open(path, "rt", encoding="utf-8") as stream:
        rows = [json.loads(line) for line in stream]
    if (not rows or rows[0].get("kind") != "header"
            or rows[0].get("schema_version") not in READABLE_SCHEMA_VERSIONS):
        raise RecordError("unknown or missing record header")
    header, observations = rows[0], rows[1:]
    if any(row.get("kind") != "observation" for row in observations):
        raise RecordError("unknown record row")
    _check_lineage(header, observations)
    card = header["scorecard"]
    recorded_corpus = (
        card["dataset"]["integrity"]["sha256"]
        if card.get("runs") else card["layers"]["generator"]["corpus_sha256"]
    )
    if recorded_corpus != header["corpus_sha256"]:
        raise RecordError("record corpus digest disagrees with scorecard")
    documents = header.get("documents")
    if not isinstance(documents, list):
        raise RecordError("missing document inventory")
    if len({row["id"] for row in documents}) != len(documents):
        raise RecordError("duplicate document ID")
    if "layers" in card:
        expected = _layer_identity(card["layers"]["generator"], documents)
        if header.get("layer_identity") != expected:
            raise RecordError("layer identity disagrees with generator or gold spans")
        _contract_from_row(header.get("layer_contract"))
    elif header.get("layer_identity") is not None:
        raise RecordError("layer identity without generated layers")
    return header, observations


def pin_template(
    source: Path, template: Path, output: Path, *, strip_layers: bool = False
) -> None:
    """Attach a committed scorecard only after its correctness matches the capture."""
    header, observations = _read(source)
    expected = json.loads(template.read_text(encoding="utf-8"))
    captured = header["scorecard"]
    if captured["gaze"] != expected["gaze"] or captured["dataset"]["integrity"] != expected["dataset"]["integrity"]:
        raise RecordError("template release or corpus identity differs from capture")
    if captured["dataset"]["validator_gold_census"] != expected["dataset"]["validator_gold_census"]:
        raise RecordError("template validator census differs from capture")
    def correctness(card):
        return [
            {key: value for key, value in run.items() if key not in {"latency_ms", "warm_latency_ms", "process"}}
            for run in card["runs"]
        ]
    if correctness(captured) != correctness(expected):
        raise RecordError("template run correctness differs from capture")
    capture = copy.deepcopy(captured)
    if strip_layers:
        capture.pop("layers", None)
        observations = [row for row in observations if row["layer"] == "C"]
        header["documents"] = [row for row in header["documents"] if row["layer"] == "C"]
        header["layer_identity"] = None
        header["layer_contract"] = None
    header["capture_scorecard"] = capture
    header["scorecard"] = expected
    header["add_reference"] = False
    _write_rows(output, [header, *observations])


def rescore(
    path: Path,
    contract: score.ScoredLabelContract,
    layer_contract: score.ScoredLabelContract | None = None,
) -> dict[str, object]:
    header, observations = _read(path)
    result = copy.deepcopy(header["scorecard"])
    document_rows = {row["id"]: row for row in header["documents"]}
    available_rows = {uid: row for uid, row in document_rows.items() if row["layer"] == "C"}
    available = _contract_documents(available_rows, contract)
    groups: dict[tuple[str, str], list[dict[str, object]]] = defaultdict(list)
    for row in observations:
        descriptor = document_rows.get(row["document_id"])
        if descriptor is None or descriptor["layer"] != row["layer"]:
            raise RecordError("observation document or layer disagrees with inventory")
        groups[(row["layer"], row["config"])].append(row)
    if not groups:
        raise RecordError("record has no observations")
    expected_groups = {("C", run["config"]) for run in result["runs"]}
    if "layers" in result:
        expected_groups.update(
            (layer, run["config"])
            for layer in ("A", "D", "R")
            for run in result["layers"][layer]["runs"]
        )
    if set(groups) != expected_groups:
        raise RecordError("observation layer/config population mismatch")
    if "layers" in result and layer_contract is not None:
        generated = [
            _document_from_row(document_rows[row["document_id"]])
            for layer in ("A", "D", "R")
            for row in groups[(layer, result["layers"][layer]["runs"][0]["config"])]
        ]
        agentic.apply_contract(generated, layer_contract)

    def replay(layer: str, config: str, selected_contract: score.ScoredLabelContract) -> dict[str, object]:
        rows = groups[(layer, config)]
        raw_documents = [_document_from_row(document_rows[row["document_id"]]) for row in rows]
        documents = score.apply_scored_label_contract(raw_documents, selected_contract)
        if len({document.uid for document in documents}) != len(documents):
            raise RecordError(f"{layer}/{config}: duplicate document ID")
        responses: dict[str, dict[str, object]] = {}
        with_evidence = []
        for document, row in zip(documents, rows, strict=True):
            recorded_response = row["response"]
            response = {
                **{key: value for key, value in recorded_response.items()
                   if key != "gold_gap_evidence"},
                "timing": {"clean_ms": 0.0, "restore_ms": 0.0, "post_policy_scan_ms": None},
            }
            if response["fixture_id"] != document.uid:
                raise RecordError(f"{document.uid}: response ID mismatch")
            if response.get("refused") is not ("pipeline_error_code" in response):
                raise RecordError(f"{document.uid}: refusal flag mismatch")
            if "pipeline_error_code" not in response:
                fallback = any(
                    item["provenance"]["decision"] == "fallback_redact"
                    for item in response["final_protection_trace"]
                )
                if response.get("fallback_redact") is not fallback:
                    raise RecordError(f"{document.uid}: fallback flag mismatch")
                for item in response["final_protection_trace"]:
                    score.validate_prediction(document, item)
                    if item["action"] not in {"tokenize", "redact"}:
                        raise RecordError("invalid protection action")
            responses[document.uid] = response
            with_evidence.append(score.Document(**{
                **document.__dict__, "gap_evidence": _evidence(recorded_response, document, response)
                if "pipeline_error_code" not in response else None,
            }))
        measured_rows = {document.uid: document_rows[document.uid] for document in documents}
        validator = None
        if all(measured_rows[document.uid].get("validator") is not None for document in documents):
            validator = _filtered_measurements(header["validator"], measured_rows, documents)
        run = score.run_config(
            Path("."), Path("."), config, with_evidence, Path("."), None, None, None,
            result["parameters"]["ner_threshold"], Path("."),
            validator_measurements=validator, replay_responses=responses,
        )
        old = next(
            (item for item in (result["runs"] if layer == "C" else result["layers"][layer]["runs"])
             if item["config"] == config), None
        )
        if old is None:
            raise RecordError(f"unexpected {layer}/{config} observation")
        expected_ids = set(old["scored_population"]["document_ids"]) | set(
            old["failed_closed_population"]["document_ids"]
        )
        if {document.uid for document in documents} != expected_ids:
            raise RecordError(f"{layer}/{config}: observed document IDs differ from scorecard")
        for timing_key in ("latency_ms", "warm_latency_ms", "process"):
            run[timing_key] = old[timing_key]
        return run

    selected: list[score.Document] = []
    if result["runs"]:
        result["runs"] = [replay("C", run["config"], contract) for run in result["runs"]]
        result["dataset"]["validator_gold_census"] = score.validator_gold_census(
            available, _filtered_measurements(header["validator"], available_rows, available)
        )
        selected = [document for document in available if document.uid in {
            row["document_id"] for row in groups[("C", result["runs"][0]["config"])]
        }]
        result["scoring"]["scored_label_contract"] = score.scored_label_contract_report(contract, selected)
    if "layers" in result:
        layer_contract = layer_contract or _contract_from_row(header.get("layer_contract"))
        for layer in ("A", "D", "R"):
            result["layers"][layer]["runs"] = [
                replay(layer, run["config"], layer_contract)
                for run in result["layers"][layer]["runs"]
            ]
            first_config = result["layers"][layer]["runs"][0]["config"]
            layer_rows = groups[(layer, first_config)]
            layer_docs = score.apply_scored_label_contract(
                [_document_from_row(document_rows[row["document_id"]]) for row in layer_rows],
                layer_contract,
            )
            result["layers"][layer]["population"] = score.population_summary(layer_docs)
            if "validator_gold_census" in result["layers"][layer]:
                row_index = {row["document_id"]: document_rows[row["document_id"]]
                             for row in layer_rows}
                result["layers"][layer]["validator_gold_census"] = score.validator_gold_census(
                    layer_docs, _filtered_measurements(header["validator"], row_index, layer_docs)
                )
        layer_documents = [
            _document_from_row(document_rows[row["document_id"]])
            for (layer, config), rows in groups.items() if layer in {"A", "D", "R"}
            for row in rows if config == result["layers"][layer]["runs"][0]["config"]
        ]
        layer_documents = score.apply_scored_label_contract(layer_documents, layer_contract)
        result["layers"]["scored_label_contract"] = score.scored_label_contract_report(layer_contract, layer_documents)
        if "gold_validity" in result["layers"]:
            result["layers"]["gold_validity"]["C"] = _gold_validity_digest(selected, available_rows)
    if header["add_reference"]:
        result["observation_record"] = {
            "file": path.name, "sha256": score.sha256_file(path),
            "format": "gzip-jsonl", "schema_version": header["schema_version"],
            "corpus_sha256": header["corpus_sha256"],
            "observations": len(observations), "bytes": path.stat().st_size,
        }
    return result


def _gold_validity_digest(
    documents: Sequence[score.Document], rows: Mapping[str, Mapping[str, object]]
) -> dict[str, object]:
    entries = sorted(
        [document.uid, span.start, span.end, span.label,
         bool(validation["applicable"]), validation["validator_passed"] is True]
        for document in documents
        for span, validation in zip(document.spans,
            _filtered_measurements({}, rows, [document])["documents"][document.uid]["gold_validation"], strict=True)
    )
    payload = json.dumps(entries, separators=(",", ":")).encode("utf-8")
    return {"algorithm": "sha256", "entities": len(entries), "value": hashlib.sha256(payload).hexdigest()}


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("record", type=Path)
    parser.add_argument("--scored-labels", type=Path)
    parser.add_argument("--agentic-scored-labels", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--expected-sha256", help="verify the published record digest")
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[2]
    def load(path: Path) -> score.ScoredLabelContract:
        resolved = path if path.is_absolute() else root / path
        try:
            display = resolved.resolve().relative_to(root).as_posix()
        except ValueError:
            display = resolved.as_posix()
        return score.load_scored_label_contract(resolved, display_path=display)
    try:
        if args.expected_sha256 is not None and score.sha256_file(args.record) != args.expected_sha256:
            raise RecordError("observation record SHA-256 mismatch")
        contract = load(args.scored_labels) if args.scored_labels else score.SCORED_LABEL_CONTRACT_V1
        layer_contract = (
            agentic.load_contract(root, args.agentic_scored_labels)
            if args.agentic_scored_labels else None
        )
        card = rescore(args.record, contract, layer_contract)
    except RecordError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 2
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(card, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
