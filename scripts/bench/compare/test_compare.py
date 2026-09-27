from __future__ import annotations

import json
from pathlib import Path

import pytest

import compare
import render


def test_native_character_offsets_become_utf8_bytes() -> None:
    spans = compare.byte_spans("Ä alice@example.invalid", [(2, 23, "EMAIL_ADDRESS")])
    assert spans == [compare.score.Span(3, 24, "EMAIL_ADDRESS")]


def test_published_paths_do_not_expose_home_directory() -> None:
    assert compare.portable_path(Path.home() / ".local" / "bin" / "python") == "$HOME/.local/bin/python"


def test_pack_requires_byte_aligned_gold_and_unique_ids(tmp_path: Path) -> None:
    row = {
        "id": "synthetic-1", "partition": "sealed", "layer": "V",
        "language": "de", "region": "DE", "text": "Ä", "gold": [
            {"start": 0, "end": 2, "label": "FIRSTNAME"}
        ],
    }
    path = tmp_path / "pack.jsonl"
    path.write_text(json.dumps(row) + "\n", encoding="utf-8")
    layers, provenance = compare.load_pack(path)
    assert list(layers) == ["V/pack/sealed"]
    assert layers["V/pack/sealed"][0].spans == (compare.score.Span(0, 2, "FIRSTNAME"),)
    assert provenance["documents"] == 1

    row["gold"][0]["end"] = 1
    path.write_text(json.dumps(row) + "\n", encoding="utf-8")
    with pytest.raises(ValueError, match="UTF-8"):
        compare.load_pack(path)


def test_contract_v3_uses_reviewed_native_mapping_for_repeat_credit() -> None:
    contract = compare.runner.load_scored_label_contract(
        compare.REPO, compare.CONTRACTS["v3"]
    )
    text = "Dr. Schmidt met Dr. Schmidt."
    start = text.index("Schmidt")
    document = compare.score.Document(
        "synthetic", text, "de", "DE", "synthetic",
        (compare.score.Span(start, start + 7, "SURNAME"),),
    )
    applied = compare.score.apply_scored_label_contract([document], contract)[0]
    native = compare.byte_spans(text, [(text.rindex("Schmidt"), text.rindex("Schmidt") + 7, "PERSON")])
    accumulator = compare.score.MetricAccumulator()
    accumulator.add(compare.mapped_document(applied, compare.load_mapping()["presidio"]), native)
    assert accumulator.result()["gold_gap"]["gold_gap_protected_bytes"] == 7


def test_unmapped_tool_label_fails_closed() -> None:
    with pytest.raises(ValueError, match="unmapped"):
        compare.validate_labels([compare.score.Span(0, 1, "UNKNOWN")], {})


def test_v3_layer_without_gold_gap_uses_raw_false_positives() -> None:
    row = {
        "leaked_bytes": 1,
        "false_positive_bytes": 2,
        "false_positive_bytes_after_gold_gap": None,
        "latency": {"p50_ms": 3, "p95_ms": 4},
    }
    report = {
        "corpus": {"layers": {"A": {}}},
        "gaze": {version: {"layers": {"A": row}} for version in ("v1", "v2", "v3")},
        "tools": {},
    }
    assert "| v3 | A | gaze | 1 | 2 | 3.0 | 4.0 |" in render.render(report, "comparison.json")
