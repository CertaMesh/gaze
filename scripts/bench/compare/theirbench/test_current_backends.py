"""Model-free migration of the live backend contract, retaining historical pins."""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import backends
import theirbench


def test_only_the_spanish_locale_addition_changes_the_historical_backend_slice(tmp_path, monkeypatch):
    source = (backends.COMPARE_DIR / "compare.py").read_text()
    previous = source.replace('("pt", "pt_PT"), ("es", "es_ES")):', '("pt", "pt_PT")):')
    assert previous != source
    assert hashlib.sha256(previous.encode()).hexdigest() == backends.PINNED_SHA256["compare.py"]
    (tmp_path / "compare.py").write_text(previous)
    monkeypatch.setattr(backends, "COMPARE_DIR", tmp_path)
    assert backends.backend_code_sha256() == backends.BACKEND_CODE_SHA256
    assert backends.COMPARISON_REVISION == "48418c26"
    assert backends.MEASURED_REVISION == "154f3da6"


def test_gaze_only_cli_records_current_source_without_loading_real_data_or_models(tmp_path, monkeypatch):
    historical_path = theirbench.compare.REPO / "docs/reference/benchmarks/their-benchmarks.json"
    historical_bytes = historical_path.read_bytes()
    document = theirbench.score.Document("fictional", "alice@example.invalid", "en", "", "synthetic", ())
    splits = {"test": [document]}
    monkeypatch.setattr(theirbench, "load_benchmark", lambda _: (splits, {"sha256": "a" * 64}))
    monkeypatch.setattr(theirbench, "harness_state", lambda: ("b" * 40, False))
    monkeypatch.setattr(theirbench.compare, "crates_tree", lambda _: "c" * 40)
    # Only label-map composition and scoring run. The runtime boundary is synthetic.
    monkeypatch.setattr(theirbench.loaders, "compose_mapping", lambda *args: {"email": ("EMAIL",)})
    def measure(name, args, policy, documents, mapping, common, predictions, diagnostics):
        return theirbench.measure_tool(name, lambda _: [], documents, mapping, common, predictions)
    monkeypatch.setattr(theirbench, "measure_gaze", measure)
    class Sampler:
        def __enter__(self):
            return self
        def __exit__(self, *args):
            return None
        def result(self):
            return {"contended": False}
    monkeypatch.setattr(theirbench, "ForeignCpuSampler", Sampler)
    def no_competitor(*args):
        raise AssertionError("Gaze-only CLI attempted competitor construction")
    monkeypatch.setattr(backends, "build_backend", no_competitor)
    policy = tmp_path / "policy.toml"
    policy.write_text('name = "fictional"\n')
    output = tmp_path / "report.json"
    monkeypatch.setattr(sys, "argv", ["theirbench.py", "--benchmark", "pii-trace", "--tool", "gaze-full",
        "--gaze-binary", str(tmp_path / "synthetic-binary"), "--gaze-model-dir", str(tmp_path / "synthetic-model"),
        "--gaze-policy", str(policy), "--predictions-dir", str(tmp_path / "native"), "--output", str(output)])
    assert theirbench.main() == 0
    result = json.loads(output.read_bytes())
    assert set(result["rows"]) == {"gaze-full"}
    assert result["comparison_revision"] == backends.CURRENT_COMPARISON_REVISION
    assert result["comparison_sha256"] == backends.CURRENT_PINNED_SHA256
    assert result["comparison_sha256"] != backends.PINNED_SHA256
    assert historical_path.read_bytes() == historical_bytes
