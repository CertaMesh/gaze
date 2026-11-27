from __future__ import annotations

import hashlib
import io
import json
from pathlib import Path
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
    [item] = panels.grascco_items(tmp_path, download=False)
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


def test_privacybench_marks_unresolved_and_indirect_spans_neutral(tmp_path, monkeypatch):
    company = panels.PRIVACYBENCH_HELD_OUT[0]
    record = {"text": "Hi Anna, Project Lark ships.", "ground_truth_spans": [
        {"start": 3, "end": 7, "label": "NAME_GIVEN"}],
        "_tracer_review": {"unknown_targets": [{"annotation": {"start": 9, "end": 16}}]}}
    files = {f"data/human_annotations/{company}.jsonl": json.dumps(record).encode(),
             panels.PRIVACYBENCH_INDIRECT: json.dumps(
                 {"file": f"human_annotations/{company}.jsonl", "line": 1, "start": 17, "end": 21}).encode()}
    for other in panels.PRIVACYBENCH_HELD_OUT:
        for name in (f"{other}.jsonl", f"{other}_documents.jsonl"):
            files.setdefault(f"data/human_annotations/{name}", b"")
    manifest = json.dumps({path: _seed(tmp_path, data) for path, data in files.items()}).encode()
    monkeypatch.setattr(panels, "PRIVACYBENCH_MANIFEST_SHA256", _seed(tmp_path, manifest))
    [item] = panels.privacybench_items(tmp_path, download=False)
    assert item.uid == f"{company}.jsonl:1" and item.group == company
    assert item.gold == [(3, 7, "NAME_GIVEN")]
    assert sorted(item.neutral) == [(9, 16), (17, 21)]


def test_daemon_classes_read_in_canonical_form():
    assert panels.class_name("Organization") == "organization"
    assert panels.class_name({"Custom": "phone"}) == "custom:phone"
    with pytest.raises(panels.PanelError):
        panels.class_name(7)


def test_panel_label_maps_do_not_overlap():
    assert not set(panels.GRASCCO_TYPES) & (set(panels.GRASCCO_UNTYPED) | panels.GRASCCO_NEUTRAL)
    assert not set(panels.PRIVACYBENCH_TYPES) & set(panels.PRIVACYBENCH_UNTYPED)
