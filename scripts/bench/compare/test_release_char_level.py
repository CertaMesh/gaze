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
