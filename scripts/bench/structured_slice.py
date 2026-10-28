#!/usr/bin/env python3
"""Report the structured-input slice from one value-free record.

The scorecard already gates these cells in bytes. This reader adds the
counts bytes cannot show: false positives by document and by value, gold
values protected whole, refusals kept apart from protection, and whether
each payload stays usable. A usable payload is not refused, has no non-gold
byte covered (decoy, key or tool name, enum string, quote, backslash or other
syntax) and changes no JSON type: a token for a value in a JSON number
position is a string, so the payload no longer has the type it had. Refused
documents count only as refusals, with their gold bytes reported apart; zero
leaks by refusing is not a gain.
"""

from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path
import sys

import agentic_layers as agentic
import gaze_bench_score as score
import leak_diff
import scorecard_record as records
import structured_input_cells as cells


SCHEMA_VERSION = 1
FIRST_GENERATOR_VERSION = 26
# Bytes that carry JSON, NDJSON or CSV syntax when they lie outside every value.
STRUCTURAL = frozenset(b'{}[],:"\\\n')
COUNTS = (
    "documents", "refused_documents", "gold_bytes", "leaked_gold_bytes", "refused_gold_bytes",
    "gold_values", "gold_values_protected", "gold_values_partial", "gold_values_refused",
    "false_positive_bytes", "false_positive_documents", "decoy_values",
    "false_positive_values", "structural_bytes_covered", "type_changed_documents",
    "usable_documents",
)


def slice_records(partition: str = agentic.PUBLISHED_PARTITION) -> list[agentic.Record]:
    return [r for r in agentic.generate(partition) if r.surface.startswith(cells.SURFACE_PREFIX)]


def _covered(start: int, end: int, protected) -> int:
    return sum(max(0, min(end, b) - max(start, a)) for a, b in protected)


def structural_offsets(record: agentic.Record) -> list[int]:
    encoded = record.text.encode("utf-8")
    inside = bytearray(len(encoded))
    for span in (*record.gold, *record.decoys):
        inside[span.start:span.end] = b"\x01" * (span.end - span.start)
    return [i for i, byte in enumerate(encoded) if byte in STRUCTURAL and not inside[i]]


def number_position(encoded: bytes, start: int) -> bool:
    """A bare JSON value: no opening quote, straight after the key's colon."""
    return encoded[start - 1:start] == b":"


def _empty() -> dict:
    return {**dict.fromkeys(COUNTS, 0), "false_positive_values_by_kind": {}}


def _add(row: dict, record: agentic.Record, protected, refused: bool) -> None:
    row["documents"] += 1
    row["gold_values"] += len(record.gold)
    row["decoy_values"] += len(record.decoys)
    gold = score.merge_intervals([(g.start, g.end) for g in record.gold])
    if refused:
        row["refused_documents"] += 1
        row["gold_values_refused"] += len(record.gold)
        row["refused_gold_bytes"] += score.interval_length(gold)
        return
    protected = score.merge_intervals(protected)
    row["gold_bytes"] += score.interval_length(gold)
    row["leaked_gold_bytes"] += score.interval_length(score.subtract_intervals(gold, protected))
    outside_gold = score.interval_length(score.subtract_intervals(protected, gold))
    row["false_positive_bytes"] += outside_gold
    row["false_positive_documents"] += int(bool(outside_gold))
    for span in record.gold:
        covered = _covered(span.start, span.end, protected)
        if covered == span.end - span.start:
            row["gold_values_protected"] += 1
        elif covered:
            row["gold_values_partial"] += 1
    touched = Counter(d.label for d in record.decoys if _covered(d.start, d.end, protected))
    row["false_positive_values"] += sum(touched.values())
    kinds = row["false_positive_values_by_kind"]
    for kind, count in touched.items():
        kinds[kind] = kinds.get(kind, 0) + count
    structural = sum(_covered(i, i + 1, protected) for i in structural_offsets(record))
    row["structural_bytes_covered"] += structural
    encoded = record.text.encode("utf-8")
    type_changed = any(number_position(encoded, g.start) and _covered(g.start, g.end, protected)
                       for g in record.gold)
    row["type_changed_documents"] += int(type_changed)
    row["usable_documents"] += int(not outside_gold and not type_changed)


def measure(slice_rows: list[agentic.Record], observations: dict) -> dict:
    """`observations` maps document id -> (protected byte intervals, refused)."""
    layers: dict[str, dict] = {}
    for record in slice_rows:
        if record.uid not in observations:
            raise records.RecordError(f"no observation for {record.uid}")
        protected, refused = observations[record.uid]
        shape = record.surface.removeprefix(cells.SURFACE_PREFIX)
        layer = layers.setdefault(record.layer, {"shapes": {}, "totals": _empty()})
        _add(layer["shapes"].setdefault(shape, _empty()), record, protected, refused)
        _add(layer["totals"], record, protected, refused)
    for layer in layers.values():
        for row in (*layer["shapes"].values(), layer["totals"]):
            row["false_positive_values_by_kind"] = dict(sorted(row["false_positive_values_by_kind"].items()))
    return dict(sorted(layers.items()))


def _check_inventory(header: dict, slice_rows: list[agentic.Record]) -> None:
    generator = header["scorecard"]["layers"]["generator"]
    if generator.get("generator_version", 0) < FIRST_GENERATOR_VERSION:
        raise records.RecordError("record predates the structured-input slice")
    expected = agentic.manifest(agentic.PUBLISHED_PARTITION, agentic.generate(agentic.PUBLISHED_PARTITION))
    if generator.get("corpus_sha256") != expected["corpus_sha256"]:
        raise records.RecordError("record measured a different generated corpus")
    documents = {row["id"]: row for row in header["documents"]}
    for record in slice_rows:
        row = documents.get(record.uid)
        gold = sorted([g.start, g.end, g.label] for g in record.gold)
        if (row is None or row["layer"] != record.layer
                or row["utf8_bytes"] != len(record.text.encode("utf-8"))
                or sorted(row["gold"]) != gold):
            raise records.RecordError(f"record inventory disagrees with {record.uid}")


def report(record_path: Path, *, config: str | None = None) -> dict:
    header, rows = records._read(record_path)
    slice_rows = slice_records()
    _check_inventory(header, slice_rows)
    config = config or agentic.production_config(header["scorecard"])
    observed = leak_diff._observations(header, rows, config)
    observations = {uid: value for (_, uid), value in observed.items()}
    return dict(schema_version=SCHEMA_VERSION, report_only=True, config=config,
                record_sha256=score.sha256_file(record_path),
                generator_version=header["scorecard"]["layers"]["generator"]["generator_version"],
                layers=measure(slice_rows, observations))


def markdown(value: dict) -> str:
    lines = [f"Config `{value['config']}`. Refused documents are counted apart and excluded from the other "
             "bytes and values; their gold is shown as refused gold bytes, which the scorer gate counts as leaked.", "",
             "| Layer | Shape | Docs | Refused | Gold values whole / partial | Leaked gold bytes "
             "| Refused gold bytes | FP docs | FP values / decoys | FP bytes | Syntax bytes covered "
             "| Type changed | Usable docs |",
             "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"]
    for layer, block in value["layers"].items():
        for shape, row in (*block["shapes"].items(), ("**total**", block["totals"])):
            lines.append(
                f"| {layer} | {shape} | {row['documents']} | {row['refused_documents']} | "
                f"{row['gold_values_protected']}/{row['gold_values']} / {row['gold_values_partial']} | "
                f"{row['leaked_gold_bytes']} | {row['refused_gold_bytes']} | {row['false_positive_documents']} | "
                f"{row['false_positive_values']}/{row['decoy_values']} | {row['false_positive_bytes']} | "
                f"{row['structural_bytes_covered']} | {row['type_changed_documents']} | {row['usable_documents']} |")
    return "\n".join(lines) + "\n"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("record", type=Path)
    parser.add_argument("--config")
    parser.add_argument("--markdown", action="store_true")
    args = parser.parse_args(argv)
    try:
        value = report(args.record, config=args.config)
    except (records.RecordError, OSError, KeyError, ValueError) as error:
        print(f"structured slice: {error}", file=sys.stderr)
        return 2
    print(markdown(value) if args.markdown else json.dumps(value, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
