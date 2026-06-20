#!/usr/bin/env python3
"""Shared scorer for safety-net direct, observer-residual, and perf snapshots."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import statistics
import subprocess
import sys
import time
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal


LOCALES = {"global": "Global", "en-US": "EnUs", "de-DE": "DeDe"}
LOCALE_TAGS = {"Global": "global", "EnUs": "en-US", "DeDe": "de-DE"}
OBSERVER_KEYS = [
    "observer_residual_recall",
    "agreement_with_rule_floor",
    "expansion_fraction",
    "contradiction_fraction",
    "novel_tp_over_rule_floor",
]
KIJI_TO_GAZE = {
    "person": "Name",
    "PER": "Name",
    "B-PER": "Name",
    "I-PER": "Name",
    "location": "Location",
    "LOC": "Location",
    "B-LOC": "Location",
    "I-LOC": "Location",
    "organization": "Name",
    "ORG": "Name",
    "B-ORG": "Name",
    "I-ORG": "Name",
    "miscellaneous": "Name",
    "MISC": "Name",
    "B-MISC": "Name",
    "I-MISC": "Name",
}
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


@dataclass(frozen=True)
class Span:
    start: int
    end: int
    pii_class: str


@dataclass(frozen=True)
class ManifestSpan:
    raw_start: int
    raw_end: int
    clean_start: int
    clean_end: int
    pii_class: str


@dataclass(frozen=True)
class Fixture:
    fixture_id: str
    locale: str
    locale_chain: list[str]
    text: str
    gold_spans: list[Span]


@dataclass(frozen=True)
class CleanFixture:
    clean_text: str
    manifest_spans: list[ManifestSpan]
    residual_gold: list[Span]


@dataclass
class LatencyRecorder:
    enabled: bool
    samples_ms: list[float]
    bytes_seen: int = 0

    def record(self, elapsed_ms: float, byte_count: int) -> None:
        if not self.enabled:
            return
        self.samples_ms.append(elapsed_ms)
        self.bytes_seen += byte_count

    def summary(self, fixture_count: int, device: str) -> dict[str, Any] | None:
        if not self.enabled or not self.samples_ms:
            return None
        samples = sorted(self.samples_ms)
        total_ms = sum(samples)
        throughput = None if total_ms == 0.0 else self.bytes_seen / (total_ms / 1000.0)
        return {
            "cold_start_ms": rounded(self.samples_ms[0]),
            "per_fixture_median_ms": rounded(statistics.median(samples)),
            "per_fixture_p95_ms": rounded(percentile(samples, 0.95)),
            "per_fixture_p99_ms": rounded(percentile(samples, 0.99)),
            "per_fixture_mean_ms": rounded(statistics.mean(samples)),
            "throughput_bytes_per_sec": rounded(throughput),
            "fixture_count": fixture_count,
            "device": device,
            "measurement_scope": (
                "per-fixture subprocess wall clock; includes fork/import/model load "
                "because the published wrappers are one-shot CLIs"
            ),
        }


def add_common_args(parser: argparse.ArgumentParser, backend: Literal["kiji", "opf"]) -> None:
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument(
        "--mode",
        choices=["direct", "observer-residual", "all"],
        default="direct",
    )
    parser.add_argument("--measure-latency", action="store_true")
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
        "--perf-snapshot",
        type=Path,
        default=Path("crates/gaze-recognizers/benches/safety_net_perf_snapshot.json"),
    )
    parser.add_argument(
        "--corpus-dir",
        type=Path,
        default=Path("crates/gaze-recognizers/testdata/coverage-loop/corpus"),
    )
    parser.add_argument("--no-update", action="store_true")
    if backend == "kiji":
        parser.add_argument("--precision", choices=["fp32", "int8"], default="fp32")
        parser.add_argument(
            "--model-dir",
            type=Path,
            default=Path(
                "~/.cache/gaze/"
                "kiji-distilbert-3a19fe9404a4469d91aa3d551558a97f68872f67"
            ),
        )
        parser.add_argument("--python", type=Path, default=Path(sys.executable))
    else:
        parser.add_argument("--opf", type=Path, default=Path(".opf-bench-venv/bin/opf"))
        parser.add_argument("--checkpoint", type=Path, default=Path.home() / ".opf/privacy_filter")
        parser.add_argument("--device", default="cpu")


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
        locale_chain = [str(locale) for locale in labels["locale_chain"]]
        locale = locale_chain[0]
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
        fixtures.append(Fixture(fixture_id, LOCALES[locale], locale_chain, text, gold_spans))
    if not fixtures:
        raise ValueError(f"no coverage-loop fixtures found in {corpus_dir}")
    return fixtures


def parse_first_json_object(stdout: str) -> Any:
    decoder = json.JSONDecoder()
    stripped = stdout.lstrip()
    try:
        value, _ = decoder.raw_decode(stripped)
    except json.JSONDecodeError as exc:
        raise RuntimeError("invalid JSON from backend") from exc
    return value


def run_kiji(args: argparse.Namespace, fixture_id: str, text: str) -> list[Span]:
    runner = args.repo_root / "scripts/bench/kiji-runner.py"
    proc = subprocess.run(
        [
            str(args.python),
            str(runner),
            "--format",
            "json",
            "--output-mode",
            "typed",
            "--model-dir",
            str(args.model_dir),
            "--precision",
            args.precision,
        ],
        input=text,
        text=True,
        encoding="utf-8",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"{fixture_id}: kiji-runner failed with {proc.returncode}: {proc.stderr.strip()}"
        )
    raw_spans = json.loads(proc.stdout)
    spans: list[Span] = []
    for raw in raw_spans:
        label = str(raw["label"])
        pii_class = KIJI_TO_GAZE.get(label)
        if pii_class is None:
            raise RuntimeError(f"{fixture_id}: unsupported Kiji label {label!r}")
        spans.append(Span(int(raw["start"]), int(raw["end"]), pii_class))
    return spans


def run_opf(args: argparse.Namespace, fixture_id: str, text: str) -> list[Span]:
    proc = subprocess.run(
        [
            str(args.opf),
            "redact",
            "--format",
            "json",
            "--output-mode",
            "typed",
            "--no-print-color-coded-text",
            "--device",
            args.device,
            "--checkpoint",
            str(args.checkpoint),
        ],
        input=text,
        text=True,
        encoding="utf-8",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"{fixture_id}: opf failed with {proc.returncode}: {proc.stderr.strip()}")
    raw_output = parse_first_json_object(proc.stdout)
    raw_spans = raw_output["detected_spans"] if isinstance(raw_output, dict) else raw_output
    spans: list[Span] = []
    for raw in raw_spans:
        label = str(raw["label"])
        pii_class = OPF_TO_GAZE.get(label)
        if pii_class is None:
            raise RuntimeError(f"{fixture_id}: unsupported OPF label {label!r}")
        spans.append(Span(int(raw["start"]), int(raw["end"]), pii_class))
    return spans


def run_backend(
    args: argparse.Namespace,
    backend: Literal["kiji", "opf"],
    fixture_id: str,
    text: str,
    latency: LatencyRecorder,
) -> list[Span]:
    start = time.perf_counter()
    spans = run_kiji(args, fixture_id, text) if backend == "kiji" else run_opf(args, fixture_id, text)
    latency.record((time.perf_counter() - start) * 1000.0, len(text.encode("utf-8")))
    return spans


def safe_div(numerator: int, denominator: int) -> float | None:
    return None if denominator == 0 else numerator / denominator


def f1(precision: float | None, recall: float | None) -> float | None:
    if precision is None or recall is None or precision + recall == 0.0:
        return None
    return 2.0 * precision * recall / (precision + recall)


def rounded(value: float | None) -> float | None:
    return None if value is None else round(value, 6)


def percentile(samples: list[float], quantile: float) -> float:
    if len(samples) == 1:
        return samples[0]
    index = (len(samples) - 1) * quantile
    lower = int(index)
    upper = min(lower + 1, len(samples) - 1)
    weight = index - lower
    return samples[lower] * (1.0 - weight) + samples[upper] * weight


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


def score(fixtures: list[Fixture], predictions: dict[str, list[Span]], gold_attr: str) -> dict[str, Any]:
    by_locale_gold: dict[str, list[Span]] = defaultdict(list)
    by_locale_pred: dict[str, list[Span]] = defaultdict(list)
    for fixture in fixtures:
        by_locale_gold[fixture.locale].extend(getattr(fixture, gold_attr))
        by_locale_pred[fixture.locale].extend(predictions[fixture.fixture_id])
    return {
        locale: class_metrics(by_locale_gold[locale], by_locale_pred[locale])
        for locale in ["Global", "EnUs", "DeDe"]
    }


def score_direct(fixtures: list[Fixture], predictions: dict[str, list[Span]]) -> dict[str, Any]:
    by_locale_gold: dict[str, list[Span]] = defaultdict(list)
    by_locale_pred: dict[str, list[Span]] = defaultdict(list)
    for fixture in fixtures:
        by_locale_gold[fixture.locale].extend(fixture.gold_spans)
        by_locale_pred[fixture.locale].extend(predictions[fixture.fixture_id])
    return {
        locale: class_metrics(by_locale_gold[locale], by_locale_pred[locale])
        for locale in ["Global", "EnUs", "DeDe"]
    }


def score_observer(fixtures: list[Fixture], clean: dict[str, CleanFixture], predictions: dict[str, list[Span]]) -> dict[str, Any]:
    by_locale_gold: dict[str, list[Span]] = defaultdict(list)
    by_locale_pred: dict[str, list[Span]] = defaultdict(list)
    for fixture in fixtures:
        by_locale_gold[fixture.locale].extend(clean[fixture.fixture_id].residual_gold)
        by_locale_pred[fixture.locale].extend(predictions[fixture.fixture_id])

    result = {}
    for locale in ["Global", "EnUs", "DeDe"]:
        metrics = class_metrics(by_locale_gold[locale], by_locale_pred[locale])
        metrics["observer_residual_recall"] = metrics["recall"]
        metrics["agreement_with_rule_floor"] = None
        metrics["expansion_fraction"] = None
        metrics["contradiction_fraction"] = None
        metrics["novel_tp_over_rule_floor"] = metrics["recall"]
        result[locale] = metrics
    return result


def clean_fixtures(root: Path, fixtures: list[Fixture]) -> dict[str, CleanFixture]:
    command = [
        "cargo",
        "run",
        "-q",
        "-p",
        "gaze-recognizers",
        "--example",
        "clean_for_bench",
    ]
    proc = subprocess.Popen(
        command,
        cwd=root,
        text=True,
        encoding="utf-8",
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert proc.stdin is not None
    assert proc.stdout is not None
    for fixture in fixtures:
        proc.stdin.write(
            json.dumps(
                {
                    "fixture_id": fixture.fixture_id,
                    "locale_chain": fixture.locale_chain,
                    "text": fixture.text,
                },
                separators=(",", ":"),
            )
            + "\n"
        )
    proc.stdin.close()
    clean: dict[str, CleanFixture] = {}
    for line in proc.stdout:
        raw = json.loads(line)
        fixture = next(item for item in fixtures if item.fixture_id == raw["fixture_id"])
        manifest = [
            ManifestSpan(
                raw_start=int(span["raw_start"]),
                raw_end=int(span["raw_end"]),
                clean_start=int(span["clean_start"]),
                clean_end=int(span["clean_end"]),
                pii_class=str(span["class"]),
            )
            for span in raw["manifest_spans"]
        ]
        clean[fixture.fixture_id] = CleanFixture(
            clean_text=str(raw["clean_text"]),
            manifest_spans=manifest,
            residual_gold=residual_gold(fixture.gold_spans, manifest),
        )
    stderr = proc.stderr.read() if proc.stderr is not None else ""
    status = proc.wait()
    if status != 0:
        raise RuntimeError(f"clean_for_bench failed with {status}: {stderr.strip()}")
    if len(clean) != len(fixtures):
        raise RuntimeError(f"clean_for_bench returned {len(clean)} fixtures, expected {len(fixtures)}")
    return clean


def residual_gold(gold: list[Span], manifest: list[ManifestSpan]) -> list[Span]:
    residual: list[Span] = []
    for span in gold:
        if any(ranges_overlap(span.start, span.end, item.raw_start, item.raw_end) for item in manifest):
            continue
        residual.append(
            Span(
                start=translate_raw_to_clean(span.start, manifest),
                end=translate_raw_to_clean(span.end, manifest),
                pii_class=span.pii_class,
            )
        )
    return residual


def translate_raw_to_clean(offset: int, manifest: list[ManifestSpan]) -> int:
    delta = 0
    for span in sorted(manifest, key=lambda item: (item.raw_start, item.raw_end)):
        if span.raw_end <= offset:
            delta += (span.clean_end - span.clean_start) - (span.raw_end - span.raw_start)
            continue
        break
    return offset + delta


def ranges_overlap(left_start: int, left_end: int, right_start: int, right_end: int) -> bool:
    return left_start < right_end and right_start < left_end


def update_matrix_snapshot(
    snapshot_path: Path,
    backend_name: str,
    mode: str,
    metrics_by_locale: dict[str, Any],
    coverage_sha256: str,
    corpus_sha256: str | None,
    fixture_count: int,
    run_environment: dict[str, Any],
    backend_pins: dict[str, Any] | None = None,
) -> None:
    snapshot = json.loads(snapshot_path.read_text(encoding="utf-8"))
    snapshot["status"] = "observer_residual_and_direct_run_v1"
    if corpus_sha256 is not None:
        snapshot["corpus"]["sha256"] = corpus_sha256
    snapshot["corpus"]["coverage_report_sha256"] = coverage_sha256
    snapshot["corpus"]["fixture_count"] = fixture_count
    snapshot["run_environment"] = run_environment
    if backend_pins is not None:
        snapshot["backends"][backend_name] = {"pins": backend_pins}
    for locale, metrics in metrics_by_locale.items():
        if mode == "direct_detector":
            snapshot["strict_span_leak_rate"][f"{backend_name}.{locale}"] = rounded(
                None if metrics["recall"] is None else 1.0 - metrics["recall"]
            )
        for cell in snapshot["cells"]:
            if cell["backend"] == backend_name and cell["locale"] == locale and cell["mode"] == mode:
                cell["metrics"] = metrics
                break
        else:
            snapshot["cells"].append(
                {
                    "backend": backend_name,
                    "locale": locale,
                    "mode": mode,
                    "metrics": metrics,
                }
            )
    snapshot_path.write_text(json.dumps(snapshot, indent=2, sort_keys=False) + "\n", encoding="utf-8")


def update_perf_snapshot(
    perf_path: Path,
    backend_name: str,
    summary: dict[str, Any] | None,
) -> None:
    if summary is None:
        return
    if perf_path.exists():
        snapshot = json.loads(perf_path.read_text(encoding="utf-8"))
    else:
        snapshot = {
            "schema_version": 1,
            "benchmark": "safety-net-perf",
            "backends": {},
            "host": host_info(),
        }
    snapshot["backends"][backend_name] = summary
    snapshot["host"] = host_info()
    perf_path.write_text(json.dumps(snapshot, indent=2, sort_keys=False) + "\n", encoding="utf-8")


def host_info() -> dict[str, str]:
    return {
        "os": platform.platform(),
        "arch": platform.machine(),
        "cpu": cpu_name(),
        "python": platform.python_version(),
    }


def cpu_name() -> str:
    if platform.system() == "Darwin":
        try:
            return subprocess.check_output(
                ["sysctl", "-n", "machdep.cpu.brand_string"],
                text=True,
                stderr=subprocess.DEVNULL,
            ).strip()
        except Exception:
            pass
    return platform.processor() or "unknown"


def validate_inputs(args: argparse.Namespace, backend: Literal["kiji", "opf"]) -> tuple[Path, Path, Path, str | None]:
    root = args.repo_root.resolve()
    args.repo_root = root
    args.coverage_report = repo_path(root, args.coverage_report)
    args.snapshot = repo_path(root, args.snapshot)
    args.perf_snapshot = repo_path(root, args.perf_snapshot)
    args.corpus_dir = repo_path(root, args.corpus_dir)
    build_manifest = args.corpus_dir.parent / "build-manifest.json"
    if not args.coverage_report.is_file():
        raise FileNotFoundError(f"missing coverage report: {args.coverage_report}")
    if backend == "kiji":
        if not args.model_dir.is_dir():
            raise FileNotFoundError(f"missing Kiji model dir: {args.model_dir}")
        if not (root / "scripts/bench/kiji-runner.py").is_file():
            raise FileNotFoundError(
                f"missing Kiji runner: {root / 'scripts/bench/kiji-runner.py'}"
            )
    else:
        args.opf = repo_path(root, args.opf)
        args.checkpoint = args.checkpoint.expanduser()
        if not args.opf.is_file():
            raise FileNotFoundError(f"missing OPF command: {args.opf}")
        if not args.checkpoint.is_dir():
            raise FileNotFoundError(f"missing OPF checkpoint dir: {args.checkpoint}")
    corpus_sha256 = None
    if build_manifest.is_file():
        corpus_sha256 = str(json.loads(build_manifest.read_text(encoding="utf-8"))["corpus_sha256"])
    return root, args.coverage_report, args.corpus_dir, corpus_sha256


def scorer_main(args: argparse.Namespace, backend: Literal["kiji", "opf"]) -> int:
    backend_name = "kiji_distilbert" if backend == "kiji" else "openai_privacy_filter"
    if backend == "kiji" and args.precision == "int8":
        backend_name = "kiji_distilbert_int8"
    root, coverage_report, corpus_dir, corpus_sha256 = validate_inputs(args, backend)
    coverage_sha256 = sha256_file(coverage_report)
    fixtures = load_fixtures(corpus_dir)
    report = json.loads(coverage_report.read_text(encoding="utf-8"))
    expected_count = int(report["totals"]["fixture_count"])
    if expected_count != len(fixtures):
        raise ValueError(f"coverage report fixture_count {expected_count} != corpus {len(fixtures)}")

    requested_modes = ["direct", "observer-residual"] if args.mode == "all" else [args.mode]
    clean = clean_fixtures(root, fixtures) if "observer-residual" in requested_modes else None
    latency = LatencyRecorder(args.measure_latency, [])
    results: dict[str, Any] = {}

    for mode in requested_modes:
        predictions: dict[str, list[Span]] = {}
        mode_latency = latency if mode == "direct" else LatencyRecorder(False, [])
        for index, fixture in enumerate(fixtures, start=1):
            input_text = fixture.text
            if mode == "observer-residual":
                assert clean is not None
                input_text = clean[fixture.fixture_id].clean_text
            predictions[fixture.fixture_id] = run_backend(
                args, backend, fixture.fixture_id, input_text, mode_latency
            )
            print(
                f"{backend_name}: {mode} {index}/{len(fixtures)} {fixture.fixture_id} "
                f"predictions={len(predictions[fixture.fixture_id])}",
                file=sys.stderr,
                flush=True,
            )

        matrix_mode = "direct_detector" if mode == "direct" else "observer_residual"
        metrics = (
            score_direct(fixtures, predictions)
            if mode == "direct"
            else score_observer(fixtures, clean or {}, predictions)
        )
        results[matrix_mode] = metrics
        if not args.no_update:
            update_matrix_snapshot(
                args.snapshot,
                backend_name,
                matrix_mode,
                metrics,
                coverage_sha256,
                corpus_sha256,
                len(fixtures),
                run_environment(args, backend),
                kiji_int8_pins(args) if backend_name == "kiji_distilbert_int8" else None,
            )

    if not args.no_update:
        update_perf_snapshot(
            args.perf_snapshot,
            backend_name,
            latency.summary(len(fixtures), getattr(args, "device", "cpu")),
        )

    json.dump(
        {
            "backend": backend_name,
            "mode": args.mode,
            "coverage_report_sha256": coverage_sha256,
            "corpus_sha256": corpus_sha256,
            "fixture_count": len(fixtures),
            "metrics_by_mode": results,
            "latency": latency.summary(len(fixtures), getattr(args, "device", "cpu")),
        },
        sys.stdout,
        indent=2,
    )
    sys.stdout.write("\n")
    return 0


def kiji_int8_pins(args: argparse.Namespace) -> dict[str, Any]:
    fp32_pins = json.loads(args.snapshot.read_text(encoding="utf-8"))["backends"][
        "kiji_distilbert"
    ]["pins"]
    return {
        **fp32_pins,
        "bundle_sha256": sha256_file(args.model_dir / "SHA256SUMS.int8"),
        "model_sha256": sha256_file(args.model_dir / "model.int8.onnx"),
    }


def run_environment(args: argparse.Namespace, backend: Literal["kiji", "opf"]) -> dict[str, Any]:
    env = {
        "os": platform.platform(),
        "machine": platform.machine(),
        "python": platform.python_version(),
    }
    if backend == "kiji":
        env["onnxruntime_threads"] = "default"
        env["precision"] = args.precision
    else:
        env["opf_device"] = args.device
    return env
