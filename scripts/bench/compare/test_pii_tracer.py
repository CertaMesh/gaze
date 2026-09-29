from __future__ import annotations

import json
import sys
import textwrap
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / "theirbench"))
sys.path.insert(0, str(HERE.parent))

import compare  # noqa: E402
import gaze_bench_score as score  # noqa: E402
import loaders  # noqa: E402
import pii_tracer  # noqa: E402
import pii_tracer_worker as worker  # noqa: E402
import theirbench  # noqa: E402

NATIVE_LABELS = {"private_person", "account_number", "private_url", "private_date", "private_address",
                 "private_email", "private_phone", "other_pii", "secret"}


def test_label_map_covers_the_documented_labels_and_credits_nothing_undocumented() -> None:
    mapping = pii_tracer.load_label_map()
    assert set(mapping) == (NATIVE_LABELS - {"secret"}) | {"custom:secret"}
    assert mapping["other_pii"] == () and mapping["custom:secret"] == ()
    credited = {label for labels in mapping.values() for label in labels}
    assert credited <= {label for table in compare.load_mapping().values() for labels in table.values() for label in labels}
    # Only the identifier types the paper names under account_number earn typed credit.
    assert set(mapping["account_number"]) == {"CREDITCARDNUMBER", "IBAN", "NATIONALID", "SSN"}
    assert not credited & {"PASSPORTID", "DRIVERLICENSENUM", "IDCARDNUM", "TAXNUM", "LICENSEPLATENUM",
                           "COMPANYNAME", "ORGANIZATION"}


def test_common_intersection_is_unchanged_by_adding_pii_tracer() -> None:
    base = compare.load_mapping()
    assert set(compare.common_claimed_labels(base)) <= {
        label for labels in pii_tracer.load_label_map().values() for label in labels}
    for benchmark in theirbench.BENCHMARKS:
        composed = {family: loaders.compose_mapping(family, base[family], benchmark)
                    for family in ("gaze", "presidio", "gliner", "datafog-core", "datafog-python", "scrubadub", "opf")}
        before = theirbench.common_intersection(composed)
        composed["pii-tracer"] = loaders.compose_mapping("pii-tracer", pii_tracer.load_label_map(), benchmark)
        assert theirbench.common_intersection(composed) == before


def test_theirbench_knows_the_tool() -> None:
    assert theirbench.tool_family("pii-tracer") == "pii-tracer"
    assert pii_tracer.TOOL in theirbench.ROSTER
    assert theirbench.typed_hold({"pii-tracer": pii_tracer.load_label_map()}) == ["pii-tracer"]


def test_windows_cover_the_text_without_gaps_or_overlap() -> None:
    offsets = [(i * 3, i * 3 + 2) for i in range(25)]  # one-space gaps between tokens
    ranges = worker.windows(offsets, 10)
    assert ranges[0][0] == 0 and ranges[-1][1] == offsets[-1][1]
    assert all(left[1] == right[0] for left, right in zip(ranges, ranges[1:]))
    assert len(ranges) == 3


def test_worker_refuses_files_that_do_not_match_the_pinned_revision(tmp_path: Path) -> None:
    for name in worker.PINNED_SHA256:
        (tmp_path / name).write_bytes(b"not the pinned bytes")
    with pytest.raises(SystemExit, match="refusing to load"):
        worker.verify_files(tmp_path)
    (tmp_path / "modeling_pii_masking.py").unlink()
    with pytest.raises(FileNotFoundError):
        worker.verify_files(tmp_path)


def test_worker_pins_every_file_that_changes_results() -> None:
    assert set(worker.PINNED_SHA256) == {
        "modeling_pii_masking.py", "model.safetensors", "config.json", "tokenizer.json", "tokenizer_config.json"}
    assert worker.REVISION == pii_tracer.REVISION


STUB = textwrap.dedent('''
    import json, sys
    print("PII_TRACER_READY " + json.dumps({"device": "cpu"}), flush=True)
    for line in sys.stdin:
        request = json.loads(line)
        text = request["text"]
        if "boom" in text:
            print(json.dumps({"id": request["id"], "error": "RuntimeError: boom"}), flush=True)
        elif "wrong id" in text:
            print(json.dumps({"id": 0, "spans": []}), flush=True)
        else:
            start = text.index("X")
            print(json.dumps({"id": request["id"], "spans": [[start, start + 2, "secret"],
                                                           [0, 1, "private_person"]]}), flush=True)
''')


@pytest.fixture
def stub(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> pii_tracer.PiiTracer:
    path = tmp_path / "stub_worker.py"
    path.write_text(STUB, encoding="utf-8")
    monkeypatch.setattr(pii_tracer, "WORKER", path)
    backend = pii_tracer.PiiTracer(Path(sys.executable), tmp_path)
    yield backend
    backend.close()


def doc(text: str) -> score.Document:
    return score.Document("d", text, "en", "", "synthetic", ())


def test_adapter_converts_characters_to_bytes_and_scores_secret_as_custom(stub: pii_tracer.PiiTracer) -> None:
    text = "é😀 XY tail"  # multi-byte characters before the span
    spans = stub.predict(doc(text))
    secret = next(span for span in spans if span.label == "custom:secret")
    assert text.encode()[secret.start:secret.end] == b"XY"
    assert {span.label for span in spans} <= set(pii_tracer.load_label_map())


def test_adapter_fails_closed_on_worker_error_or_mismatched_reply(stub: pii_tracer.PiiTracer) -> None:
    with pytest.raises(RuntimeError, match="boom"):
        stub.predict(doc("boom"))
    with pytest.raises(RuntimeError):
        stub.predict(doc("wrong id"))


def test_adapter_refuses_a_worker_that_never_reports_ready(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    path = tmp_path / "bad.py"
    path.write_text("print('something else')\n", encoding="utf-8")
    monkeypatch.setattr(pii_tracer, "WORKER", path)
    with pytest.raises(RuntimeError, match="failed to start"):
        pii_tracer.PiiTracer(Path(sys.executable), tmp_path)


def test_preflight_rejects_an_unmapped_native_label(stub: pii_tracer.PiiTracer) -> None:
    mapping = {label: labels for label, labels in pii_tracer.load_label_map().items() if label != "private_person"}
    with pytest.raises(ValueError, match="unmapped labels"):
        pii_tracer.preflight(stub, {"C": [doc("X1 X2")]}, 1, mapping)
