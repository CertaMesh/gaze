#!/usr/bin/env python3
"""Populate OPF direct-detector benchmark cells from the coverage-loop corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import subprocess
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any


LOCALES = {"global": "Global", "en-US": "EnUs", "de-DE": "DeDe"}
OPF_TO_GAZE = {
    "private_person": "Name",
    "private_address": "Location",
    "private_email": "Email",
    "private_phone": "custom:phone",
    "private_url": "custom:url",
    "private_date": "custom:date",
    "account_number": "custom:account_number",
    "secret": "custom:secret",
}
OBSERVER_KEYS = [
    "observer_residual_recall",
    "agreement_with_rule_floor",
    "expansion_fraction",
    "contradiction_fraction",
    "novel_tp_over_rule_floor",
]


@dataclass(frozen=True)
class Span:
    start: int
    end: int
    pii_class: str


@dataclass(frozen=True)
class Fixture:
    fixture_id: str
    locale: str
    text: str
    gold_spans: list[Span]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--opf", type=Path, default=Path(".opf-bench-venv/bin/opf"))
    parser.add_argument(
        "--checkpoint",
        type=Path,
        default=Path.home() / ".opf/privacy_filter",
    )
    parser.add_argument("--device", default="cpu")
    parser.add_argument(
        "--coverage-report",
        type=Path,
        default=Path("target/coverage-report.json"),
    )
    parser.add_argument(
        "--snapshot",
        type=Path,
        default=Path("crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json"),
    )
    parser.add_argument(
        "--corpus-dir",
        type=Path,
        default=Path("crates/gaze-recognizers/testdata/coverage-loop/corpus"),
    )
    parser.add_argument("--no-update", action="store_true")
    return parser.parse_args()


def repo_path(root: Path, path: Path) -> Path:
    return path if path.is_absolute() else root / path


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_fixtures(corpus_dir: Path) -> list[Fixture]:
    fixtures: list[Fixture] = []
    for labels_path in sorted(corpus_dir.glob("*.labels.json")):
        labels = json.loads(labels_path.read_text(encoding="utf-8"))
        fixture_id = labels["fixture_id"]
        locale = labels["locale_chain"][0]
        if locale not in LOCALES:
            raise ValueError(f"{fixture_id}: unsupported locale {locale!r}")
        text = labels_path.with_name(f"{fixture_id}.txt").read_text(encoding="utf-8")
        gold_spans = [
            Span(
                start=int(span["byte_start"]),
                end=int(span["byte_end"]),
                pii_class=str(span["class_id"]),
            )
            for span in labels["spans"]
        ]
        fixtures.append(Fixture(fixture_id, LOCALES[locale], text, gold_spans))
    if not fixtures:
        raise ValueError(f"no coverage-loop fixtures found in {corpus_dir}")
    return fixtures


def parse_first_json_object(stdout: str) -> Any:
    decoder = json.JSONDecoder()
    stripped = stdout.lstrip()
    try:
        value, _ = decoder.raw_decode(stripped)
    except json.JSONDecodeError as exc:
        raise RuntimeError("invalid OPF JSON") from exc
    return value


def run_opf(opf: Path, checkpoint: Path, device: str, fixture: Fixture) -> list[Span]:
    proc = subprocess.run(
        [
            str(opf),
            "redact",
            "--format",
            "json",
            "--output-mode",
            "typed",
            "--no-print-color-coded-text",
            "--device",
            device,
            "--checkpoint",
            str(checkpoint),
        ],
        input=fixture.text,
        text=True,
        encoding="utf-8",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"{fixture.fixture_id}: opf failed with {proc.returncode}: "
            f"{proc.stderr.strip()}"
        )

    raw_output = parse_first_json_object(proc.stdout)
    raw_spans = raw_output["detected_spans"] if isinstance(raw_output, dict) else raw_output
    spans: list[Span] = []
    for raw in raw_spans:
        label = str(raw["label"])
        pii_class = OPF_TO_GAZE.get(label)
        if pii_class is None:
            raise RuntimeError(f"{fixture.fixture_id}: unsupported OPF label {label!r}")
        spans.append(Span(int(raw["start"]), int(raw["end"]), pii_class))
    return spans


def safe_div(numerator: int, denominator: int) -> float | None:
    if denominator == 0:
        return None
    return numerator / denominator


def f1(precision: float | None, recall: float | None) -> float | None:
    if precision is None or recall is None or precision + recall == 0.0:
        return None
    return 2.0 * precision * recall / (precision + recall)


def rounded(value: float | None) -> float | None:
    if value is None:
        return None
    return round(value, 6)


def class_metrics(gold: list[Span], predictions: list[Span]) -> dict[str, Any]:
    gold_counts = Counter(span.pii_class for span in gold)
    pred_counts = Counter(span.pii_class for span in predictions)
    gold_set = Counter((span.start, span.end, span.pii_class) for span in gold)
    true_positive_counts: Counter[str] = Counter()

    for pred in predictions:
        key = (pred.start, pred.end, pred.pii_class)
        if gold_set[key] > 0:
            gold_set[key] -= 1
            true_positive_counts[pred.pii_class] += 1

    classes = sorted(set(gold_counts) | set(pred_counts))
    per_class: dict[str, Any] = {}
    for pii_class in classes:
        tp = true_positive_counts[pii_class]
        precision = safe_div(tp, pred_counts[pii_class])
        recall = safe_div(tp, gold_counts[pii_class])
        per_class[pii_class] = {
            "support": gold_counts[pii_class],
            "predicted": pred_counts[pii_class],
            "true_positive": tp,
            "false_positive": pred_counts[pii_class] - tp,
            "false_negative": gold_counts[pii_class] - tp,
            "precision": rounded(precision),
            "recall": rounded(recall),
            "f1": rounded(f1(precision, recall)),
        }

    total_tp = sum(true_positive_counts.values())
    precision = safe_div(total_tp, len(predictions))
    recall_values = [
        metrics["recall"] for metrics in per_class.values() if metrics["support"] > 0
    ]
    recall = sum(recall_values) / len(recall_values) if recall_values else None
    return {
        "precision": rounded(precision),
        "recall": rounded(recall),
        "f1": rounded(f1(precision, recall)),
        "per_class": per_class,
    }


def score(fixtures: list[Fixture], predictions: dict[str, list[Span]]) -> dict[str, Any]:
    by_locale_gold: dict[str, list[Span]] = defaultdict(list)
    by_locale_pred: dict[str, list[Span]] = defaultdict(list)
    for fixture in fixtures:
        by_locale_gold[fixture.locale].extend(fixture.gold_spans)
        by_locale_pred[fixture.locale].extend(predictions[fixture.fixture_id])

    return {
        locale: class_metrics(by_locale_gold[locale], by_locale_pred[locale])
        for locale in ["Global", "EnUs", "DeDe"]
    }


def update_snapshot(
    snapshot_path: Path,
    metrics_by_locale: dict[str, Any],
    coverage_sha256: str,
    corpus_sha256: str,
    fixture_count: int,
    device: str,
) -> dict[str, Any]:
    snapshot = json.loads(snapshot_path.read_text(encoding="utf-8"))
    snapshot["status"] = "opf_kiji_direct_run_v1_observer_residual_deferred"
    snapshot["corpus"]["sha256"] = corpus_sha256
    snapshot["corpus"]["coverage_report_sha256"] = coverage_sha256
    snapshot["corpus"]["fixture_count"] = fixture_count
    snapshot["run_environment"] = {
        "os": platform.platform(),
        "machine": platform.machine(),
        "python": platform.python_version(),
        "opf_device": device,
    }

    for locale, metrics in metrics_by_locale.items():
        snapshot["strict_span_leak_rate"][f"openai_privacy_filter.{locale}"] = rounded(
            None if metrics["recall"] is None else 1.0 - metrics["recall"]
        )
        for cell in snapshot["cells"]:
            if (
                cell["backend"] == "openai_privacy_filter"
                and cell["locale"] == locale
                and cell["mode"] == "direct_detector"
            ):
                cell["metrics"] = metrics
            if (
                cell["backend"] == "openai_privacy_filter"
                and cell["locale"] == locale
                and cell["mode"] == "observer_residual"
            ):
                cell["metrics"]["precision"] = None
                cell["metrics"]["recall"] = None
                cell["metrics"]["f1"] = None
                for key in OBSERVER_KEYS:
                    cell["metrics"][key] = None
                cell["metrics"]["per_class"] = {}

    snapshot_path.write_text(
        json.dumps(snapshot, indent=2, sort_keys=False) + "\n",
        encoding="utf-8",
    )
    return snapshot


def main() -> int:
    args = parse_args()
    root = args.repo_root.resolve()
    coverage_report = repo_path(root, args.coverage_report)
    snapshot_path = repo_path(root, args.snapshot)
    corpus_dir = repo_path(root, args.corpus_dir)
    build_manifest = corpus_dir.parent / "build-manifest.json"
    opf = repo_path(root, args.opf)
    checkpoint = args.checkpoint.expanduser()

    if not coverage_report.is_file():
        raise FileNotFoundError(f"missing coverage report: {coverage_report}")
    if not opf.is_file():
        raise FileNotFoundError(f"missing OPF command: {opf}")
    if not checkpoint.is_dir():
        raise FileNotFoundError(f"missing OPF checkpoint dir: {checkpoint}")
    if not build_manifest.is_file():
        raise FileNotFoundError(f"missing coverage-loop build manifest: {build_manifest}")

    coverage_sha256 = sha256_file(coverage_report)
    manifest = json.loads(build_manifest.read_text(encoding="utf-8"))
    corpus_sha256 = str(manifest["corpus_sha256"])
    fixtures = load_fixtures(corpus_dir)
    report = json.loads(coverage_report.read_text(encoding="utf-8"))
    expected_count = int(report["totals"]["fixture_count"])
    if expected_count != len(fixtures):
        raise ValueError(
            f"coverage report fixture_count {expected_count} != corpus {len(fixtures)}"
        )

    print(
        f"opf-bench-scorer: fixtures={len(fixtures)} "
        f"coverage_report_sha256={coverage_sha256} corpus_sha256={corpus_sha256}",
        file=sys.stderr,
    )
    predictions: dict[str, list[Span]] = {}
    for index, fixture in enumerate(fixtures, start=1):
        predictions[fixture.fixture_id] = run_opf(opf, checkpoint, args.device, fixture)
        print(
            f"opf-bench-scorer: {index}/{len(fixtures)} {fixture.fixture_id} "
            f"predictions={len(predictions[fixture.fixture_id])}",
            file=sys.stderr,
            flush=True,
        )

    metrics_by_locale = score(fixtures, predictions)
    result = {
        "backend": "openai_privacy_filter",
        "mode": "direct_detector",
        "coverage_report_sha256": coverage_sha256,
        "corpus_sha256": corpus_sha256,
        "fixture_count": len(fixtures),
        "opf_command": str(opf),
        "checkpoint": str(checkpoint),
        "device": args.device,
        "metrics_by_locale": metrics_by_locale,
    }
    if not args.no_update:
        update_snapshot(
            snapshot_path,
            metrics_by_locale,
            coverage_sha256,
            corpus_sha256,
            len(fixtures),
            args.device,
        )
    json.dump(result, sys.stdout, indent=2)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
