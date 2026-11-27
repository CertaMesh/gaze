#!/usr/bin/env python3
"""Report-only detection metrics: boundaries, exposure, types and harmless masking.

The gate scores protected bytes. These metrics describe the same observations
from other sides, so a reviewer can see what the byte totals hide:

- exact spans: precision and recall of predicted spans whose bounds equal a
  gold span, on raw gold and on gold with adjacent same-type parts merged
  (Gaze emits "Anna Weber" as one name; many corpora split it);
- exposure: each gold span is fully hidden, partially exposed or completely
  missed, judged on its letters and digits only, so a visible ", " between
  tokenized parts is not exposure; residue is the letters and digits left;
- type agreement: of the fully hidden spans whose label has a Gaze class, the
  share an overlapping prediction gives an accepted class. The label map is
  explicit and fails closed: a label in neither `TYPE_CLASSES` nor `UNTYPED`
  stops the report;
- harmless masking: recorded decoys (benign words and digit runs beside gold)
  that a prediction touches, and benign documents with any masking.

Nothing here enters the gain gate or the capture: the report reads a record
and writes its own JSON. Offsets are characters of the rebuilt text; the
metadata-only holdout documents are read only when `--dataset` names the
pinned Dataiku parquet, and are otherwise counted as unread.

    python scripts/bench/report_metrics.py RECORD [--dataset test.parquet] [--output report.json]
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from dataclasses import dataclass
import json
from pathlib import Path
import re
import sys
from typing import Iterable, Mapping, Sequence

import agentic_layers as agentic
import gaze_bench_score as score
import json_structure_census as census
import scorecard_record as records

SCHEMA_VERSION = 1

_LOCATION = frozenset({"location"})
_ZIP = frozenset({"location", "custom:postal_code"})
_BUILDING = frozenset({"location", "custom:building_number"})
_IBAN = frozenset({"custom:iban", "custom:account_number", "custom:family:payment-card-or-iban"})
_CARD = frozenset({"custom:credit_card", "custom:family:payment-card-or-iban"})
_NATIONAL = frozenset({"custom:national_id", "custom:ssn", "custom:nino"})
_TAX = frozenset({"custom:tax_number", "custom:steuer_id", "custom:tax_id"})

# Corpus gold label -> the Gaze classes that type it correctly.
TYPE_CLASSES: dict[str, frozenset[str]] = {
    "FIRSTNAME": frozenset({"name"}), "GIVENNAME": frozenset({"name"}),
    "MIDDLENAME": frozenset({"name"}), "SURNAME": frozenset({"name"}),
    "COMPANYNAME": frozenset({"organization"}), "ORGANIZATION": frozenset({"organization"}),
    "ORGANISATION": frozenset({"organization"}), "BANKNAME": frozenset({"organization"}),
    "CITY": _LOCATION, "STREET": _LOCATION, "STATE": _LOCATION, "COUNTRY": _LOCATION,
    "REGION": _LOCATION, "ZIP": _ZIP, "ZIPCODE": _ZIP, "BUILDINGNUM": _BUILDING,
    "EMAIL": frozenset({"email"}),
    "TELEPHONENUM": frozenset({"custom:phone"}), "PHONENUMBER": frozenset({"custom:phone"}),
    "IBAN": _IBAN, "ACCOUNTNUM": _IBAN, "CREDITCARDNUMBER": _CARD,
    "PASSWORD": frozenset({"custom:password", "custom:secret"}),
    "SECURITYTOKEN": frozenset({"custom:security_token", "custom:secret"}),
    "ETHADDRESS": frozenset({"custom:eth_address"}),
    "IPADDRESS": frozenset({"custom:ip_address"}),
    "URL": frozenset({"custom:url"}), "USERNAME": frozenset({"custom:username"}),
    "DATEOFBIRTH": frozenset({"custom:birth_date", "custom:date"}),
    "AGE": frozenset({"custom:age"}),
    "NATIONALID": _NATIONAL, "IDCARDNUM": _NATIONAL, "SSN": _NATIONAL, "SOCIALNUM": _NATIONAL,
    "PASSPORTID": frozenset({"custom:passport"}), "PASSPORTNUM": frozenset({"custom:passport"}),
    "DRIVERLICENSENUM": frozenset({"custom:driver_license"}),
    "LICENSEPLATENUM": frozenset({"custom:license_plate"}),
    "LICENSEPLATE": frozenset({"custom:license_plate"}),
    "TAXNUM": _TAX, "VATID": frozenset({"custom:vat_id"}),
    "AADHAAR": frozenset({"custom:aadhaar"}), "BSN": frozenset({"custom:bsn"}),
    "CNPJ": frozenset({"custom:cnpj"}), "CPF": frozenset({"custom:cpf"}),
    "NHSNUMBER": frozenset({"custom:nhs_number"}), "NIR": frozenset({"custom:nir"}),
    "PAN": frozenset({"custom:pan"}),
}
# Labels with no built-in Gaze class: their exposure is measured, their type is not.
UNTYPED: dict[str, str] = {
    "CUSTOMER_ID": "tenant identifier; its class is whatever the integrator declares",
    "EMPLOYEE_ID": "tenant identifier; its class is whatever the integrator declares",
    "RECORD_ID": "tenant identifier; its class is whatever the integrator declares",
    "TITLE": "an honorific has no class of its own",
}
_MERGE_GAP = re.compile(r"[ \t.]*")
_WHITESPACE = " \t\n\r"


class MetricsError(Exception):
    pass


@dataclass(frozen=True)
class Item:
    """One document: text and spans in character offsets."""

    uid: str
    group: str
    text: str
    gold: Sequence[tuple[int, int, str]]
    predictions: Sequence[tuple[int, int, str]]
    # Characters that are neither gold nor false positive (unresolved spans).
    neutral: Sequence[tuple[int, int]] = ()
    # Known-benign spans; any coverage is harmless masking.
    decoys: Sequence[tuple[int, int]] = ()
    # A document that holds no PII at all.
    benign: bool = False


def _mask(spans: Iterable[tuple[int, int]]) -> int:
    value = 0
    for start, end in spans:
        if end > start:
            value |= ((1 << (end - start)) - 1) << start
    return value


def _strip(text: str, start: int, end: int) -> tuple[int, int]:
    while start < end and text[start] in _WHITESPACE:
        start += 1
    while end > start and text[end - 1] in _WHITESPACE:
        end -= 1
    return start, end


def _type_key(label: str, type_classes: Mapping[str, frozenset[str]]) -> object:
    return type_classes.get(label, label)


def merge_adjacent(text: str, gold: Sequence[tuple[int, int, str]],
                   type_classes: Mapping[str, frozenset[str]]) -> list[tuple[int, int, str]]:
    """Join gold parts of one type separated only by spaces, tabs or dots."""
    merged: list[tuple[int, int, str]] = []
    for start, end, label in sorted(gold):
        if (merged and _type_key(merged[-1][2], type_classes) == _type_key(label, type_classes)
                and start >= merged[-1][1] and _MERGE_GAP.fullmatch(text, merged[-1][1], start)):
            merged[-1] = (merged[-1][0], max(end, merged[-1][1]), merged[-1][2])
        else:
            merged.append((start, end, label))
    return merged


def check_labels(labels: Iterable[str], type_classes: Mapping[str, frozenset[str]],
                 untyped: Mapping[str, str]) -> None:
    unknown = sorted(set(labels) - set(type_classes) - set(untyped))
    if unknown:
        raise MetricsError(f"no type ruling for gold labels {unknown}; add each to the type map")


def _prf(hits: int, predicted: int, gold: int) -> dict[str, float | None]:
    return {"precision": round(100 * hits / predicted, 2) if predicted else None,
            "recall": round(100 * hits / gold, 2) if gold else None,
            "f1": round(200 * hits / (predicted + gold), 2) if predicted + gold else None}


def _pct(part: int, whole: int) -> float | None:
    return round(100 * part / whole, 2) if whole else None


def score_items(items: Sequence[Item], type_classes: Mapping[str, frozenset[str]],
                untyped: Mapping[str, str]) -> dict:
    """Every metric over a set of documents; gold labels must all be ruled on."""
    check_labels((label for item in items for *_, label in item.gold), type_classes, untyped)
    c: Counter = Counter()
    labels: dict[str, Counter] = defaultdict(Counter)
    mistyped: Counter = Counter()
    for item in items:
        text = item.text
        gold = [(*_strip(text, start, end), label) for start, end, label in item.gold]
        gold = [span for span in gold if span[1] > span[0]]
        predictions = [(*_strip(text, start, end), cls) for start, end, cls in item.predictions]
        predictions = [span for span in predictions if span[1] > span[0]]
        gold_mask = _mask((s, e) for s, e, _ in gold)
        neutral = _mask(item.neutral) & ~gold_mask
        covered = _mask((s, e) for s, e, _ in predictions)
        scored = [p for p in predictions if _mask([(p[0], p[1])]) & ~neutral]
        bounds = {(s, e) for s, e, _ in scored}
        c["predicted"] += len(scored)
        for name, spans in (("raw", gold), ("merged", merge_adjacent(text, gold, type_classes))):
            spans = {(s, e) for s, e, _ in spans}
            c[f"gold_{name}"] += len(spans)
            c[f"exact_{name}"] += len(spans & bounds)
        for start, end, cls in scored:
            if not _mask([(start, end)]) & gold_mask:
                c["stray"] += 1
        for start, end, label in gold:
            row = labels[label]
            alnum = [i for i in range(start, end) if text[i].isalnum()] or list(range(start, end))
            hidden = sum(1 for i in alnum if covered >> i & 1)
            row["spans"] += 1
            row["characters"] += len(alnum)
            row["residue"] += len(alnum) - hidden
            if hidden == len(alnum):
                row["fully_hidden"] += 1
                if label in type_classes:
                    row["typed"] += 1
                    classes = {cls for s, e, cls in predictions if s < end and e > start}
                    if classes & type_classes[label]:
                        row["type_agrees"] += 1
                    else:
                        mistyped[(label, "+".join(sorted(classes)))] += 1
            elif hidden:
                row["partially_exposed"] += 1
                row["partial_residue"] += len(alnum) - hidden
            else:
                row["completely_missed"] += 1
        for start, end in item.decoys:
            c["decoys"] += 1
            c["decoys_masked"] += bool(_mask([(start, end)]) & covered)
        if item.benign:
            c["benign_documents"] += 1
            c["benign_documents_masked"] += bool(covered & ~neutral)
    total: Counter = sum(labels.values(), Counter())
    spans = total["spans"]
    return {
        "documents": len(items),
        "gold_spans": spans,
        "exact_span": {"raw": _prf(c["exact_raw"], c["predicted"], c["gold_raw"]),
                       "merged": _prf(c["exact_merged"], c["predicted"], c["gold_merged"]),
                       "predicted_spans": c["predicted"], "stray_predicted_spans": c["stray"]},
        "exposure": {
            "fully_hidden": total["fully_hidden"], "partially_exposed": total["partially_exposed"],
            "completely_missed": total["completely_missed"],
            "fully_hidden_pct": _pct(total["fully_hidden"], spans),
            "partially_exposed_pct": _pct(total["partially_exposed"], spans),
            "completely_missed_pct": _pct(total["completely_missed"], spans),
            "residue_characters": total["residue"],
            "partial_residue_characters": total["partial_residue"],
            "missed_residue_characters": total["residue"] - total["partial_residue"],
            "gold_characters": total["characters"],
        },
        "type_agreement": {
            "typed_fully_hidden": total["typed"], "agrees": total["type_agrees"],
            "agreement_pct": _pct(total["type_agrees"], total["typed"]),
            "untyped_fully_hidden": total["fully_hidden"] - total["typed"],
            "top_disagreements": [{"label": label, "classes": classes, "spans": n}
                                  for (label, classes), n in mistyped.most_common(15)],
        },
        "harmless_masking": {
            "decoys": c["decoys"], "decoys_masked": c["decoys_masked"],
            "decoy_rate_pct": _pct(c["decoys_masked"], c["decoys"]),
            "benign_documents": c["benign_documents"],
            "benign_documents_masked": c["benign_documents_masked"],
            "benign_document_rate_pct": _pct(c["benign_documents_masked"], c["benign_documents"]),
        },
        "per_label": {label: {key: row[key] for key in (
            "spans", "fully_hidden", "partially_exposed", "completely_missed", "residue",
            "typed", "type_agrees")} for label, row in sorted(labels.items())},
    }


# --------------------------------------------------------------------------
# Benchmark records.


def _byte_to_char(text: str) -> dict[int, int]:
    return {offset: index for index, offset in enumerate(score.char_to_byte_offsets(text))}


def _holdout_texts(dataset: Path) -> dict[str, str]:
    import dataiku_en_de_gaze_bench as dataiku

    documents, _ = dataiku.load_documents(dataset)
    return {document.uid: document.text for document in documents}


def record_items(record_path: Path, *, config: str | None = None,
                 dataset: Path | None = None) -> tuple[dict, list[Item], Counter]:
    header, rows = records._read(record_path)
    config = config or agentic.production_config(header["scorecard"])
    documents = {row["id"]: row for row in header["documents"]}
    texts = {uid: text for uid, (text, _) in census._texts(header)[0].items()}
    decoys = {record.uid: [(d.start, d.end) for d in record.decoys]
              for record in agentic.generate(agentic.PUBLISHED_PARTITION) if record.decoys}
    if dataset is not None:
        texts.update(_holdout_texts(dataset))
    items: list[Item] = []
    skipped: Counter = Counter()
    for row in census._selected_rows(header, rows, config):
        uid, layer = row["document_id"], row["layer"]
        document = documents[uid]
        if row["response"].get("refused"):
            skipped[f"{layer}:refused"] += 1
            continue
        text = texts.get(uid)
        if text is None:
            skipped[f"{layer}:unread"] += 1
            continue
        if len(text.encode("utf-8")) != document["utf8_bytes"]:
            raise MetricsError(f"rebuilt text length disagrees with the record: {uid}")
        chars = _byte_to_char(text)
        try:
            gold = [(chars[s], chars[e], label) for s, e, label in document["gold"]]
            predictions = [(chars[t["raw_start"]], chars[t["raw_end"]], t["class"])
                           for t in row["response"]["final_protection_trace"]]
            spans = [(chars[s], chars[e]) for s, e in decoys.get(uid, ())]
        except KeyError as error:
            raise MetricsError(f"{uid}: an offset is not on a character boundary") from error
        items.append(Item(uid, layer, text, gold, predictions, decoys=spans,
                          benign=not gold and layer in ("C", "D")))
    return header, items, skipped


def report(record_path: Path, *, config: str | None = None, dataset: Path | None = None) -> dict:
    header, items, skipped = record_items(record_path, config=config, dataset=dataset)
    by_layer: dict[str, list[Item]] = defaultdict(list)
    for item in items:
        by_layer[item.group].append(item)
    return {
        "schema_version": SCHEMA_VERSION,
        "report_only": True,
        "record_sha256": score.sha256_file(record_path),
        "benchmark_spec_id": header.get("benchmark_spec_id"),
        "config": config or agentic.production_config(header["scorecard"]),
        "untyped_labels": UNTYPED,
        "skipped_documents": dict(sorted(skipped.items())),
        "layers": {layer: score_items(group, TYPE_CLASSES, UNTYPED)
                   for layer, group in sorted(by_layer.items())},
        "all": score_items(items, TYPE_CLASSES, UNTYPED),
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("record", type=Path)
    parser.add_argument("--config", help="observation config (default: the production config)")
    parser.add_argument("--dataset", type=Path, help="pinned Dataiku parquet, to read holdout C")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    try:
        value = report(args.record, config=args.config, dataset=args.dataset)
    except (MetricsError, census.CensusError, records.RecordError, OSError, KeyError, ValueError) as error:
        print(f"report metrics: {error}", file=sys.stderr)
        return 2
    text = json.dumps(value, indent=1, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(text, encoding="utf-8")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
