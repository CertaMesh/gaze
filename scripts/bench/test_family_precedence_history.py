"""Historical arm receipts and replay reject forged inputs and metrics."""
import copy
import hashlib
import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import family_precedence_arm as arm
import family_precedence_history as history
import scorecard_record as records


def receipt(monkeypatch):
    monkeypatch.setattr(history, "tag_commit", lambda *_: "a" * 40)
    monkeypatch.setattr(history, "expected_tag_source", lambda *_: b"locked source")
    monkeypatch.setattr(history, "expected_example_sha", lambda *_: "c" * 64)
    return {
        "release": "v0.16.0", "revision": "a" * 40, "fresh_locked_build": True,
        "toolchain": "rustc 1.96.0 (fixture)", "changed_paths": [],
        "cargo_lock_sha256": hashlib.sha256(b"locked source").hexdigest(),
        "example_source_sha256": "c" * 64, "binary_sha256": "d" * 64, "cli_sha256": "e" * 64,
    }


@pytest.mark.parametrize("key,value", [
    ("revision", "b" * 40), ("fresh_locked_build", False), ("toolchain", "rustc 1.95.0"),
    ("cargo_lock_sha256", "f" * 64), ("example_source_sha256", "f" * 64),
    ("binary_sha256", "unsigned"), ("changed_paths", ["crates/gaze/src/registry.rs"]),
])
def test_release_build_receipt_rejects_unbound_sources_and_binaries(monkeypatch, key, value):
    data = receipt(monkeypatch)
    history.validate_receipt(data, "v0.16.0")
    data[key] = value
    with pytest.raises(ValueError):
        history.validate_receipt(data, "v0.16.0")


def test_bridge_changes_only_the_benchmark_transport():
    text = history.BRIDGE.read_text()
    assert text.count("diff --git ") == 1
    assert text.startswith("diff --git a/crates/gaze-recognizers/examples/clean_for_bench.rs ")
    assert "fn build_policy_run()" in text
    assert "gaze_assembly::build_pipeline(" in text
    assert "registry.rs" not in text
    assert "resolver.rs" not in text


def valid_capture(tmp_path, monkeypatch):
    build = receipt(monkeypatch)
    cards = {}
    headers = {}
    capture = {
        "release": "v0.16.0", "revision": "a" * 40, "build": build,
        "corpus_sha256": hashlib.sha256(history.agentic.corpus_bytes(arm.generate("test"))).hexdigest(),
        "generator_sha256": history.sha(Path(arm.__file__)),
        "history_harness_sha256": history.sha(Path(history.__file__)),
        "policy_delta_sha256": history.sha(arm.DELTA), "layers": {},
    }
    for layer in ("A", "D"):
        rows = [row for row in arm.generate("test") if row.layer == layer]
        path = tmp_path / f"{layer}.gz"
        path.write_bytes(layer.encode())
        headers[path] = ({"corpus_sha256": capture["corpus_sha256"],
                          "scorecard": {"gaze": {"revision": "a" * 40}},
                          "documents": [records._document_row(row.to_document()) for row in rows]},
                         [{"document_id": row.uid} for row in rows])
        run = {"metrics": {"utf8_bytes": {"leaked": 0, "false_positive": 0}},
               "pipeline_contract": {"documents": len(rows), "restore_exact_documents": len(rows), "manifest_valid_documents": len(rows)},
               "pipeline_availability": {"failed_closed_documents": 0, "attempted_documents": len(rows)}, "per_cell": {}}
        cards[path] = {"runs": [run]}
        capture["layers"][layer] = {"record": {"file": path.name, "sha256": history.sha(path), "observations": len(rows)},
                                       "contracts": {str(v): history.metric_totals(run) for v in (1, 2, 3, 4)}}
    monkeypatch.setattr(history.records, "_read", lambda p: headers[p])
    monkeypatch.setattr(history.records, "rescore", lambda p, *_args, **_kw: cards[p])
    (tmp_path / "capture.json").write_text(json.dumps(capture))
    return capture, headers


@pytest.mark.parametrize("tamper", ["file", "gold", "population", "metrics", "missing_layer", "generator"])
def test_history_replay_rejects_tampering(tmp_path, monkeypatch, tamper):
    capture, headers = valid_capture(tmp_path, monkeypatch)
    history.check(tmp_path)
    if tamper == "file":
        (tmp_path / "A.gz").write_bytes(b"modified observation")
    elif tamper == "gold":
        headers[tmp_path / "A.gz"][0]["documents"][0]["gold"][0][0] += 1
    elif tamper == "population":
        headers[tmp_path / "D.gz"][0]["documents"].pop()
    elif tamper == "metrics":
        capture["layers"]["A"]["contracts"]["2"]["leaked"] += 1
    elif tamper == "missing_layer":
        del capture["layers"]["D"]
    else:
        capture["generator_sha256"] = "0" * 64
    (tmp_path / "capture.json").write_text(json.dumps(capture))
    with pytest.raises(ValueError):
        history.check(tmp_path)
