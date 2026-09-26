#!/usr/bin/env python3
"""Value-free benchmark observations and contract-independent scorecard replay."""

from __future__ import annotations

import argparse
import copy
import gzip
import hashlib
import json
from collections import defaultdict
from pathlib import Path
from typing import Mapping, Sequence

import gaze_bench_score as score


SCHEMA_VERSION = 1


class RecordError(ValueError):
    pass


def _write_rows(path: Path, rows: Sequence[Mapping[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0, compresslevel=9) as zipped:
            for row in rows:
                zipped.write(json.dumps(row, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8") + b"\n")


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


def _contract_from_row(row: Mapping[str, object]) -> score.ScoredLabelContract:
    return score.ScoredLabelContract(
        contract_id=row["id"], version=row["version"], path=row["path"],
        sha256=row["sha256"],
        scored_labels=frozenset(row["scored"]) if row["scored"] is not None else None,
        excluded_labels=frozenset(row["excluded"]),
        neutral_prediction_classes=frozenset(row["neutral"]),
        gold_gap=score.GoldGapRule(frozenset(tuple(pair) for pair in row["gold_gap"]))
        if row["gold_gap"] is not None else None,
    )


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
        "restore": response["restore"],
        "manifest_integrity": response["manifest_integrity"],
        "initial_safety_net_stats": response["initial_safety_net_stats"],
        "strict_would_reject": response["strict_would_reject"],
        "post_policy_safety_net_stats": response["post_policy_safety_net_stats"],
        "timing": response["timing"],
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
        self.available = tuple(available)
        self.documents = {document.uid: document for document in (*available, *extra_documents)}
        if len(self.documents) != len(available) + len(extra_documents):
            raise RecordError("available document IDs are not unique")
        self.measurements = measurements
        self.corpus_sha256 = corpus_sha256
        self.layer_contract = layer_contract
        self.layer_measurements: dict[str, Mapping[str, object]] = {}
        self.rows: list[dict[str, object]] = []

    def add(
        self,
        layer: str,
        config: str,
        document: score.Document,
        response: Mapping[str, object],
        validator_measurements: Mapping[str, object] | None = None,
    ) -> None:
        original = self.documents[document.uid]
        row = {
            "kind": "observation",
            "corpus_sha256": self.corpus_sha256,
            "layer": layer,
            "config": config,
            "document": _document_row(original),
            "response": _compact_response(response),
        }
        measurement = self.layer_measurements.get(layer, validator_measurements)
        if measurement is not None:
            row["validator"] = measurement["documents"][document.uid]
        if "pipeline_error_code" not in response:
            row["gold_gap_evidence"] = _gap_evidence(
                original, score.final_trace_predictions(original, dict(response))
            )
        self.rows.append(row)

    def write(self, path: Path, scorecard: Mapping[str, object], *, add_reference: bool) -> dict[str, object]:
        available = [
            {
                **_document_row(document),
                "validator": self.measurements["documents"][document.uid],
            }
            for document in sorted(self.available, key=lambda item: item.uid)
        ]
        header = {
            "kind": "header",
            "schema_version": SCHEMA_VERSION,
            "corpus_sha256": self.corpus_sha256,
            "scorecard": scorecard,
            "add_reference": add_reference,
            "validator": {
                key: value for key, value in self.measurements.items() if key != "documents"
            },
            "layer_contract": _contract_row(self.layer_contract),
            "available": available,
        }
        rows = [header, *self.rows]
        _write_rows(path, rows)
        digest = score.sha256_file(path)
        return {
            "file": path.name,
            "sha256": digest,
            "format": "gzip-jsonl",
            "schema_version": SCHEMA_VERSION,
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


def _evidence(row: Mapping[str, object]) -> dict[tuple[int, int, str], tuple[int, int, bool, tuple[tuple[int, int, str], ...]]]:
    result = {}
    for start, end, label, trimmed_start, trimmed_end, boundary, matches in row.get("gold_gap_evidence", []):
        key = (start, end, label)
        result[key] = (trimmed_start, trimmed_end, boundary, tuple(tuple(match) for match in matches))
    return result


def _read(path: Path) -> tuple[dict[str, object], list[dict[str, object]]]:
    with gzip.open(path, "rt", encoding="utf-8") as stream:
        rows = [json.loads(line) for line in stream]
    if not rows or rows[0].get("kind") != "header" or rows[0].get("schema_version") != SCHEMA_VERSION:
        raise RecordError("unknown or missing record header")
    header, observations = rows[0], rows[1:]
    if any(row.get("kind") != "observation" for row in observations):
        raise RecordError("unknown record row")
    if header["scorecard"]["dataset"]["integrity"]["sha256"] != header["corpus_sha256"]:
        raise RecordError("record corpus digest disagrees with scorecard")
    return header, observations


def pin_template(source: Path, template: Path, output: Path) -> None:
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
    header["capture_scorecard"] = captured
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
    available_rows = {row["id"]: row for row in header["available"]}
    if len(available_rows) != len(header["available"]):
        raise RecordError("duplicate available document ID")
    available = _contract_documents(available_rows, contract)
    groups: dict[tuple[str, str], list[dict[str, object]]] = defaultdict(list)
    for row in observations:
        if row["corpus_sha256"] != header["corpus_sha256"]:
            raise RecordError("observation corpus digest mismatch")
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

    def replay(layer: str, config: str, selected_contract: score.ScoredLabelContract) -> dict[str, object]:
        rows = groups[(layer, config)]
        raw_documents = [_document_from_row(row["document"]) for row in rows]
        documents = score.apply_scored_label_contract(raw_documents, selected_contract)
        if len({document.uid for document in documents}) != len(documents):
            raise RecordError(f"{layer}/{config}: duplicate document ID")
        responses: dict[str, dict[str, object]] = {}
        with_evidence = []
        for document, row in zip(documents, rows, strict=True):
            if layer == "C" and row["document"] != {
                key: value for key, value in available_rows[document.uid].items() if key != "validator"
            }:
                raise RecordError(f"{document.uid}: observation disagrees with corpus header")
            response = row["response"]
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
                **document.__dict__, "gap_evidence": _evidence(row),
            }))
        measured_rows = {
            row["id"]: row for row in header["available"]
        } if layer == "C" else {
            row["document"]["id"]: {
                **row["document"], "validator": row.get("validator"),
            } for row in rows
        }
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

    result["runs"] = [replay("C", run["config"], contract) for run in result["runs"]]
    result["dataset"]["validator_gold_census"] = score.validator_gold_census(
        available, _filtered_measurements(header["validator"], available_rows, available)
    )
    selected = [document for document in available if document.uid in {
        row["document"]["id"] for row in groups[("C", result["runs"][0]["config"])]
    }]
    result["scoring"]["scored_label_contract"] = score.scored_label_contract_report(contract, selected)
    if "layers" in result:
        layer_contract = layer_contract or _contract_from_row(header["layer_contract"])
        for layer in ("A", "D", "R"):
            result["layers"][layer]["runs"] = [
                replay(layer, run["config"], layer_contract)
                for run in result["layers"][layer]["runs"]
            ]
            first_config = result["layers"][layer]["runs"][0]["config"]
            layer_rows = groups[(layer, first_config)]
            layer_docs = score.apply_scored_label_contract(
                [_document_from_row(row["document"]) for row in layer_rows],
                layer_contract,
            )
            result["layers"][layer]["population"] = score.population_summary(layer_docs)
            if "validator_gold_census" in result["layers"][layer]:
                row_index = {
                    row["document"]["id"]: {
                        "validator": row["validator"]
                    } for row in layer_rows
                }
                result["layers"][layer]["validator_gold_census"] = score.validator_gold_census(
                    layer_docs, _filtered_measurements(header["validator"], row_index, layer_docs)
                )
        layer_documents = [
            _document_from_row(row["document"])
            for (layer, config), rows in groups.items() if layer in {"A", "D", "R"}
            for row in rows if config == result["layers"][layer]["runs"][0]["config"]
        ]
        layer_documents = score.apply_scored_label_contract(layer_documents, layer_contract)
        result["layers"]["scored_label_contract"] = score.scored_label_contract_report(layer_contract, layer_documents)
        result["layers"]["gold_validity"]["C"] = _gold_validity_digest(selected, available_rows)
    if header["add_reference"]:
        result["observation_record"] = {
            "file": path.name, "sha256": score.sha256_file(path),
            "format": "gzip-jsonl", "schema_version": SCHEMA_VERSION,
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
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[2]
    def load(path: Path) -> score.ScoredLabelContract:
        resolved = path if path.is_absolute() else root / path
        try:
            display = resolved.resolve().relative_to(root).as_posix()
        except ValueError:
            display = resolved.as_posix()
        return score.load_scored_label_contract(resolved, display_path=display)
    contract = load(args.scored_labels) if args.scored_labels else score.SCORED_LABEL_CONTRACT_V1
    layer_contract = load(args.agentic_scored_labels) if args.agentic_scored_labels else None
    card = rescore(args.record, contract, layer_contract)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(card, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
