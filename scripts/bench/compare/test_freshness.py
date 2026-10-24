"""Model-free freshness classification and immutable report binding."""
import copy
import json
from pathlib import Path
import sys
from urllib.error import URLError

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import freshness as f

DAY = "2026-10-24"
ROW = {"key": "pypi:synthetic-tool@1.0", "kind": "pypi", "name": "synthetic-tool",
       "pinned_version": "1.0", "consumers": [], "driver_constraint": None}


def file(yanked=False, python=None):
    return {"upload_time_iso_8601": "2026-10-01T12:00:00Z", "yanked": yanked, "requires_python": python}


def query(releases, row=ROW):
    return f.check(row, DAY, lambda _: {"releases": releases})


def test_stable_newer():
    row = query({"1.0": [file()], "1.1": [file()]})
    assert row["status"] == "update-unverified"
    assert row["latest_stable"] == {"version": "1.1", "release_date": "2026-10-01"}
    assert row["pinned"]["version"] == "1.0"
    assert row["checked_on"] == DAY
    assert "new subject identity" in row["finding"]


@pytest.mark.parametrize("version", ["1.2rc1", "1.2a1", "1.2b1", "1.2.dev1"])
def test_prerelease_only_newer_ignored(version):
    row = query({"1.0": [file()], version: [file()]})
    assert row["status"] == "current"
    assert row["latest_stable"]["version"] == "1.0"


def test_yanked_release_ignored():
    assert query({"1.0": [file()], "2.0": [file(yanked=True)]})["status"] == "current"


def test_missing_package():
    def missing(_):
        raise f.MissingSource()
    row = f.check(ROW, DAY, missing)
    assert row["status"] == "missing"
    assert row["finding"] and row["latest_stable"] is None


def test_missing_pinned_release():
    assert query({"2.0": [file()]})["status"] == "missing"


def test_network_failure_visible_nonfatal():
    def offline(_):
        raise URLError("synthetic private diagnostic must not be published")
    row = f.check(ROW, DAY, offline)
    assert row["status"] == "network-failure"
    assert "private diagnostic" not in json.dumps(row)
    assert "freshness unknown" in f.markdown({"checked_on": DAY, "rows": [row]})


@pytest.mark.parametrize("exc", [KeyError("releases"), f.SourceFailure("HTTP 429")])
def test_invalid_source_visible_nonfatal(exc):
    def fail(_):
        raise exc
    assert f.check(ROW, DAY, fail)["status"] == "source-failure"


def test_incompatible_latest_driver_keeps_old_row():
    row = {**ROW, "driver_constraint": {"specifier": "==1.0", "reason": "driver requires 1.0"}}
    result = query({"1.0": [file()], "2.0": [file()]}, row)
    assert result["status"] == "incompatible-latest"
    assert result["pinned"]["version"] == "1.0"
    assert result["latest_stable"]["version"] == "2.0"
    assert "explicitly old" in f.markdown({"checked_on": DAY, "rows": [result]})


def test_incompatible_python():
    assert query({"1.0": [file()], "2.0": [file(python=">=3.13")]})["status"] == "incompatible-latest"
    assert query({"1.0": [file()], "2.0": [file(python=">=3.13"), file(python=">=3.11")]})["status"] == "update-unverified"


def test_no_stable():
    assert query({"1.0rc1": [file()]}, {**ROW, "pinned_version": "1.0rc1"})["status"] == "no-stable-release"


def test_github_model_family_and_prerelease():
    row = {**ROW, "kind": "github", "name": "synthetic/models", "pinned_version": "en-1.0", "tag_prefix": "en-"}
    def get(url):
        if "/matching-refs/" in url:
            return [{"ref": "refs/tags/" + t} for t in ["en-1.0", "de-9.0", "en-3.0", "en-4.0", "en-2.0rc1"]]
        tag = url.rsplit("/", 1)[1]
        return {"published_at": "2025-01-01T00:00:00Z", "draft": tag == "en-4.0", "prerelease": tag == "en-3.0"}
    assert f.check(row, DAY, get)["status"] == "current"


def test_github_pagination_limit_is_not_current():
    row = {**ROW, "kind": "github", "name": "synthetic/models", "pinned_version": "1.0", "tag_prefix": ""}
    def get(url):
        if "/tags/" in url:
            return {"published_at": "2025-01-01T00:00:00Z"}
        return [{"tag_name": "1.0", "published_at": "2025-01-01T00:00:00Z", "draft": False, "prerelease": False}] * 100
    assert f.check(row, DAY, get)["status"] == "source-failure"


def test_untagged_model_does_not_call_main_stable():
    row = {**ROW, "kind": "huggingface", "name": "synthetic/model", "pinned_version": "a" * 40}
    def get(url):
        if url.endswith("/refs"):
            return {"tags": []}
        assert "/commits/" in url
        return [{"id": "a" * 40, "date": "2026-01-01T00:00:00Z"}]
    result = f.check(row, DAY, get)
    assert result["status"] == "unversioned" and result["latest_stable"] is None
    assert result["pinned"]["release_date"] == "2026-01-01"


def test_tagged_model_exact_commit():
    row = {**ROW, "kind": "huggingface", "name": "synthetic/model", "pinned_version": "a" * 40}
    def get(url):
        if url.endswith("/refs"):
            return {"tags": [{"name": "v2.0", "targetCommit": "b" * 40}, {"name": "v3rc1", "targetCommit": "c" * 40}]}
        return [{"id": url.rsplit("/", 1)[1], "date": "2026-01-01T00:00:00Z"}]
    result = f.check(row, DAY, get)
    assert result["status"] == "update-unverified"
    assert result["latest_stable"]["version"] == "b" * 40
    assert result["latest_stable"]["tag"] == "v2.0"


def test_inventory_includes_locks_models_and_tuned():
    rows, inputs = f.inventory()
    assert len(rows) > 100
    assert len([p for p in inputs if p.endswith(".lock")]) == 4
    assert any("OpenMed/" in r["name"] for r in rows)
    assert any("PII-Tracer" in r["name"] for r in rows)
    assert any("mdeberta" in r["name"] for r in rows)
    assert any(r["name"] == "presidio-anonymizer" and r["driver_constraint"] for r in rows)
    assert any("theirbench/requirements-theirbench.lock" in p for p in inputs)
    assert any("en_core_web_lg-3.8.0" in r["key"] for r in rows)


def test_sidecar_binding_and_completeness(tmp_path):
    rows, inputs = f.inventory()
    comparison = tmp_path / "comparison.json"
    comparison.write_text("{}")
    report = {"schema": f.SCHEMA, "checked_on": DAY, "inputs": inputs, "selection": None,
              "comparison_sha256": f.digest(comparison),
              "rows": [f.check(row, DAY, lambda _: {}) for row in rows]}
    f.validate(report, comparison)
    partial = copy.deepcopy(report)
    partial["rows"].pop()
    with pytest.raises(ValueError, match="incomplete"):
        f.validate(partial, comparison)
    stale = copy.deepcopy(report)
    stale["inputs"][next(iter(inputs))] = "0" * 64
    with pytest.raises(ValueError, match="changed"):
        f.validate(stale, comparison)
    report["rows"][0]["pinned_version"] = "foreign-pin"
    with pytest.raises(ValueError, match="pin metadata"):
        f.validate(report, comparison)
    comparison.write_text('{"changed":true}')
    with pytest.raises(ValueError, match="another comparison"):
        f.validate(report, comparison)


def test_partial_sidecar_not_publishable(tmp_path):
    rows, inputs = f.inventory()
    comparison = tmp_path / "comparison.json"
    comparison.write_text("{}")
    report = {"schema": f.SCHEMA, "checked_on": DAY, "inputs": inputs, "selection": [rows[0]["key"]],
              "comparison_sha256": f.digest(comparison), "rows": [f.check(rows[0], DAY, lambda _: {})]}
    path = tmp_path / "freshness.json"
    path.write_text(json.dumps(report))
    with pytest.raises(ValueError, match="partial"):
        f.read_for_comparison(path, comparison)


def test_opaque_checkpoint_is_visible():
    result = f.check({**ROW, "kind": "opaque"}, DAY, lambda _: pytest.fail("opaque source queried network"))
    assert result["status"] == "unversioned"
    assert result["pinned"]["version"] == ROW["pinned_version"]
    assert result["pinned"]["release_date"] is None


def test_renderer_optional_sidecar(tmp_path, monkeypatch):
    import render
    comparison = tmp_path / "comparison.json"
    comparison.write_text("{}")
    page = tmp_path / "page.md"
    ledger_path = tmp_path / "freshness.json"
    ledger = {"checked_on": DAY, "rows": [query({"1.0": [file()], "2.0": [file()]})]}
    monkeypatch.setattr(render, "input_paths", lambda *_: [])
    monkeypatch.setattr(render, "validate_current", lambda *_: None)
    monkeypatch.setattr(render, "render", lambda *_: "Existing comparison.\n")
    monkeypatch.setattr(f, "read_for_comparison", lambda path, report: ledger if path == ledger_path and report == comparison else pytest.fail("wrong binding"))
    monkeypatch.setattr(sys, "argv", ["render.py", str(comparison), "--page", str(page), "--freshness", str(ledger_path)])
    render.main()
    assert page.read_text().startswith("Existing comparison.\n\n## Competitor source freshness")
    assert "2.0" in page.read_text() and "update-unverified" in page.read_text()
    monkeypatch.setattr(sys, "argv", ["render.py", str(comparison), "--page", str(page)])
    render.main()
    assert page.read_text() == "Existing comparison.\n"
