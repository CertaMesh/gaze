from __future__ import annotations

import hashlib
import io
import json
from pathlib import Path
import sys
import zipfile

import pytest

import heldout_panels as panels


def _cas(text, phi):
    return json.dumps({"%FEATURE_STRUCTURES": [
        {"%TYPE": "uima.cas.Sofa", "sofaString": text},
        *({"%TYPE": "webanno.custom.PHI", "begin": b, "end": e, "kind": k} for b, e, k in phi)]})


def _seed(cache: Path, data: bytes) -> str:
    digest = hashlib.sha256(data).hexdigest()
    cache.mkdir(parents=True, exist_ok=True)
    (cache / digest).write_bytes(data)
    return digest


def test_fetch_never_returns_unverified_bytes(tmp_path):
    digest = _seed(tmp_path, b"pinned")
    assert panels.fetch("https://example.invalid/a", digest, tmp_path, download=False) == b"pinned"
    (tmp_path / digest).write_bytes(b"tampered")
    with pytest.raises(panels.PanelError):
        panels.fetch("https://example.invalid/a", digest, tmp_path, download=False)


def test_grascco_neutral_kinds_leave_gold(tmp_path, monkeypatch):
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        archive.writestr("grascco_pii_2_json/Muster.txtphi-pii_2.0.json",
                         _cas("Dr. Schmidt sah Frau Muster am 01.02.2020.",
                              [(0, 3, "NAME_TITLE"), (4, 11, "NAME_DOCTOR"), (21, 27, "NAME_PATIENT"),
                               (31, 41, "DATE")]))
    monkeypatch.setattr(panels, "GRASCCO_SHA256", _seed(tmp_path, buffer.getvalue()))
    [item], _ = panels.grascco_items(tmp_path, download=False)
    assert item.uid == "Muster"
    assert [label for *_, label in item.gold] == ["NAME_DOCTOR", "NAME_PATIENT"]
    assert sorted(item.neutral) == [(0, 3), (31, 41)]


def test_grascco_refuses_offsets_outside_the_bmp(tmp_path, monkeypatch):
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        archive.writestr("x/A.txtphi.json", _cas("\U0001F600 Muster", [(2, 8, "NAME_PATIENT")]))
    monkeypatch.setattr(panels, "GRASCCO_SHA256", _seed(tmp_path, buffer.getvalue()))
    with pytest.raises(panels.PanelError):
        panels.grascco_items(tmp_path, download=False)


def test_privacybench_skips_unresolved_records_and_neutralizes_indirect_spans(tmp_path, monkeypatch):
    company = panels.PRIVACYBENCH_HELD_OUT[0]
    unresolved = {"text": "Hi Bo.", "ground_truth_spans": [],
                  "_tracer_review": {"unknown_targets": [{"annotation": {"start": 3, "end": 5}}]}}
    record = {"text": "Hi Anna, Project Lark ships.", "ground_truth_spans": [
        {"start": 3, "end": 7, "label": "NAME_GIVEN"}]}
    lines = "\n".join(json.dumps(r) for r in (unresolved, record)) + "\n"
    files = {f"data/human_annotations/{company}.jsonl": lines.encode(),
             panels.PRIVACYBENCH_INDIRECT: json.dumps(
                 {"file": f"human_annotations/{company}.jsonl", "line": 2, "start": 17, "end": 21}).encode()}
    for other in panels.PRIVACYBENCH_HELD_OUT:
        for name in (f"{other}.jsonl", f"{other}_documents.jsonl"):
            files.setdefault(f"data/human_annotations/{name}", b"")
    manifest = json.dumps({path: _seed(tmp_path, data) for path, data in files.items()}).encode()
    monkeypatch.setattr(panels, "PRIVACYBENCH_MANIFEST_SHA256", _seed(tmp_path, manifest))
    [item], skipped = panels.privacybench_items(tmp_path, download=False)
    assert skipped == 1
    assert item.uid == f"{company}.jsonl:2" and item.group == company
    assert item.gold == [(3, 7, "NAME_GIVEN")]
    assert list(item.neutral) == [(17, 21)]


def test_daemon_classes_read_in_canonical_form():
    assert panels.class_name("Organization") == "organization"
    assert panels.class_name({"Custom": "phone"}) == "custom:phone"
    with pytest.raises(panels.PanelError):
        panels.class_name(7)


def test_panel_label_maps_do_not_overlap():
    assert not set(panels.GRASCCO_TYPES) & (set(panels.GRASCCO_UNTYPED) | panels.GRASCCO_NEUTRAL)
    assert not set(panels.PRIVACYBENCH_TYPES) & set(panels.PRIVACYBENCH_UNTYPED)


STUB = """import json, sys
for n, line in enumerate(sys.stdin):
    request = json.loads(line)
    if n == 1:
        print(json.dumps({"error": "Refused", "detail": "x"}), flush=True)
    elif n == 2:
        sys.stderr.write("stub stopped")
        sys.exit(3)
    else:
        # "Grüße " is 6 characters and 8 UTF-8 bytes: Anna is bytes 8..12.
        print(json.dumps({"session_id": request["session_id"], "clean_text": "",
                          "manifest": [{"raw_span": {"start": 8, "end": 12}, "class": "Name"}]}), flush=True)
"""


def _stub(tmp_path):
    script = tmp_path / "stub.py"
    script.write_text(STUB, encoding="utf-8")
    binary = tmp_path / "gaze"
    binary.write_text(f"#!/bin/sh\nexec {sys.executable} -I {script} \"$@\"\n", encoding="utf-8")
    binary.chmod(0o755)
    return binary


def test_daemon_byte_spans_become_characters_and_errors_hide_nothing(tmp_path):
    items = [panels.metrics.Item(f"d{n}", "g", "Grüße Anna", [(6, 10, "NAME_GIVEN")], ()) for n in range(2)]
    scored, latencies, errors = panels.predict(items, _stub(tmp_path), tmp_path / "policy.toml")
    assert scored[0].predictions == [(6, 10, "name")]
    assert scored[1].predictions == () and errors == 1 and len(latencies) == 2


def test_a_daemon_that_exits_stops_the_panel(tmp_path):
    items = [panels.metrics.Item(f"d{n}", "g", "Grüße Anna", [], ()) for n in range(3)]
    with pytest.raises(panels.PanelError, match="stub stopped"):
        panels.predict(items, _stub(tmp_path), tmp_path / "policy.toml")


def test_ledger_separates_ward_codes_from_case_numbers_without_changing_scores():
    text = "Station A33, Fallnummer: 23346011, Zimmer 3 Süd."
    gold = [(8, 11, "ID"), (25, 33, "ID"), (42, 47, "ID")]
    # Gaze protects the case number only.
    item = panels.metrics.Item("Muster", "Muster", text, gold, [(25, 33, "custom:record_id")])
    types, untyped = panels.GRASCCO_TYPES, panels.GRASCCO_UNTYPED
    before = panels.metrics.score_items([item], types, untyped)
    result = panels.ledger("grascco_pii_v2", [item], types, untyped)
    codes = result["entries"]["ward_room_codes"]
    assert (codes["gold_spans"], codes["fully_hidden"], codes["completely_missed"]) == (2, 0, 2)
    without = result["exposure_without_disputed_gold"]
    assert (without["fully_hidden"], without["completely_missed"]) == (1, 0)
    assert panels.metrics.score_items([item], types, untyped) == before
    assert before["exposure"]["completely_missed"] == 2


def test_ledger_counts_only_unlabelled_place_and_organization_strays():
    text = "Dr. Schmidt in Ohio met Acme HR on Wednesday."
    gold = [(4, 11, "NAME_FAMILY")]
    predictions = [(4, 11, "name"), (15, 19, "location"), (24, 31, "organization"),
                   (35, 44, "custom:date"), (0, 11, "location")]
    item = panels.metrics.Item("x", "c", text, gold, predictions)
    entries = panels.ledger("privacybench_plus", [item], panels.PRIVACYBENCH_TYPES,
                            panels.PRIVACYBENCH_UNTYPED)["entries"]
    # The location span overlapping gold is not a stray; the date stray belongs to no entry.
    assert entries["standalone_places"]["stray_predicted_spans"] == 1
    assert entries["brand_uses"]["stray_predicted_spans"] == 1
    assert "exposure_without_disputed_gold" not in panels.ledger(
        "privacybench_plus", [item], panels.PRIVACYBENCH_TYPES, panels.PRIVACYBENCH_UNTYPED)
