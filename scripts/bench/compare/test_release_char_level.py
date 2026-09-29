"""release_char_level.py: character-level scores of tagged releases, offline checks."""

from __future__ import annotations

import copy
import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release_char_level as rcl  # noqa: E402

DATA = json.loads(rcl.OUTPUT.read_text(encoding="utf-8"))
HISTORY = rcl.history_doc.load_history(rcl.history_doc.DEFAULT_HISTORY)
CORPUS = rcl.REPO / "target/bench-data/dataiku-en-de/test.parquet"


def test_committed_file_matches_history_and_metrics() -> None:
    rcl.check(DATA, HISTORY)
    assert set(DATA["releases"]) == {"v0.14.0", "v0.15.0", "v0.15.1"}
    for row in DATA["releases"].values():
        assert row["char_level"]["unit"] == "unicode_code_point"
        assert 0 < row["char_level"]["f2"] < 1


@pytest.mark.parametrize("mutation, match", [
    (lambda d: d["releases"]["v0.15.1"].update(leaked_bytes=1), "leaked bytes"),
    (lambda d: d["releases"]["v0.15.1"].update(false_positive_bytes=1), "false-positive bytes"),
    (lambda d: d["releases"]["v0.15.1"].update(record_sha256="0" * 64), "record hash"),
    (lambda d: d.update(metrics_sha256="0" * 64), "comparison_metrics"),
    (lambda d: d["releases"].update({"main": d["releases"]["v0.15.1"]}), "not a release tag"),
    (lambda d: d["releases"]["v0.15.1"]["char_level"].update(f2=0.99), "stored f2"),
    (lambda d: d["releases"]["v0.15.1"]["char_level"].update(recall=0.5), "stored recall"),
    (lambda d: d["releases"]["v0.15.1"]["char_level"].update(fn=10**9), "stored"),
    (lambda d: d["releases"]["v0.15.1"].update(crates_tree="0" * 40), "crates tree"),
])
def test_check_refuses_tampered_rows(mutation, match) -> None:
    data = copy.deepcopy(DATA)
    mutation(data)
    with pytest.raises(ValueError, match=match):
        rcl.check(data, HISTORY)


@pytest.mark.skipif(not CORPUS.exists(), reason="needs the local benchmark corpus")
def test_record_reproduces_the_committed_scores() -> None:
    rebuilt = rcl.build(CORPUS)
    assert rebuilt == DATA


def test_a_corrupted_record_file_is_refused(tmp_path) -> None:
    """`check` hashes the committed record itself, not just the hash the history states."""
    import shutil

    for entry in HISTORY["releases"]:
        observation = rcl.observation_of(entry)
        if observation:
            shutil.copy(rcl.BENCH_DIR / observation["file"], tmp_path / observation["file"])
    rcl.check(DATA, HISTORY, tmp_path)
    victim = tmp_path / rcl.observation_of(HISTORY["releases"][-1])["file"]
    victim.write_bytes(victim.read_bytes() + b"x")
    with pytest.raises(ValueError, match="does not match its recorded hash"):
        rcl.check(DATA, HISTORY, tmp_path)


def test_the_corpus_must_be_the_one_the_release_was_measured_on(monkeypatch) -> None:
    if not CORPUS.exists():
        pytest.skip("needs the local benchmark corpus")
    import compare

    real = compare.load_corpus

    def other(dataset, packs):
        layers, identity = real(dataset, packs)
        identity = {**identity, "negative_corpus_sha256": "0" * 64}
        return layers, identity

    monkeypatch.setattr(compare, "load_corpus", other)
    with pytest.raises(ValueError, match="not the one it was measured on"):
        rcl.build(CORPUS)


def test_character_counts_cannot_exceed_the_byte_counts() -> None:
    """Consistent P/R/F2 but more missed characters than leaked bytes is impossible."""
    tp, fp, fn = 90, 0, 10
    precision, recall = 1.0, 0.9
    f2 = 5 * precision * recall / (4 * precision + recall)
    row = {"leaked_bytes": 5, "false_positive_bytes": 0,
           "char_level": {"tp": tp, "fp": fp, "fn": fn, "precision": precision,
                          "recall": recall, "f2": f2}}
    with pytest.raises(ValueError, match="exceed 5 leaked bytes"):
        rcl.check_char_level("v9.9.9", row)
    row["leaked_bytes"] = 10
    rcl.check_char_level("v9.9.9", row)
