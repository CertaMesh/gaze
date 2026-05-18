#!/usr/bin/env python3
"""End-to-end Gaze pipeline benchmark snapshot generator."""

from __future__ import annotations

import argparse
import json
import platform
import statistics
import subprocess
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

from safety_net_bench_lib import (
    Fixture,
    ManifestSpan,
    Span,
    class_metrics,
    host_info,
    load_fixtures,
    percentile,
    rounded,
)


CONFIGS = [
    "rule-floor-core",
    "rule-floor-extended",
    "pass3-kiji",
    "pass3-opf",
    "pass3-locale-aware",
]
LOCALES = ["Global", "EnUs", "DeDe"]
MEASURABLE_LABELS = [
    "Email",
    "IBAN",
    "credit_card",
    "phone",
    "postal_code",
    "Name-first",
    "Name-surname",
]
CLASS_MAP = {
    "email": "Email",
    "Email": "Email",
    "iban": "IBAN",
    "IBAN": "IBAN",
    "custom:iban": "IBAN",
    "credit_card": "credit_card",
    "custom:credit_card": "credit_card",
    "phone": "phone",
    "custom:phone": "phone",
    "postal_code": "postal_code",
    "custom:postal_code": "postal_code",
    "name": "Name",
    "Name": "Name",
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument(
        "--corpus-dir",
        type=Path,
        default=Path("crates/gaze-recognizers/testdata/coverage-loop/corpus"),
    )
    parser.add_argument(
        "--snapshot",
        type=Path,
        default=Path("crates/gaze-recognizers/benches/gaze_pipeline_bench_snapshot.json"),
    )
    parser.add_argument("--config", choices=CONFIGS, action="append")
    parser.add_argument("--no-update", action="store_true")
    args = parser.parse_args()

    root = args.repo_root.resolve()
    corpus_dir = repo_path(root, args.corpus_dir)
    snapshot_path = repo_path(root, args.snapshot)
    fixtures = load_fixtures(corpus_dir)
    build_manifest = corpus_dir.parent / "build-manifest.json"
    corpus_sha256 = None
    corpus_seed = None
    if build_manifest.is_file():
        manifest = json.loads(build_manifest.read_text(encoding="utf-8"))
        corpus_sha256 = manifest.get("corpus_sha256")
        corpus_seed = manifest.get("seed")

    cells: list[dict[str, Any]] = []
    requested = args.config or CONFIGS
    for config in requested:
        result = run_config(root, fixtures, config)
        cells.extend(cells_for_config(config, fixtures, result))

    snapshot = {
        "schema_version": 1,
        "benchmark": "gaze-pipeline-end-to-end",
        "status": "measured_with_nulls_for_unavailable_configs",
        "corpus": {
            "path": str(args.corpus_dir),
            "fixture_count": len(fixtures),
            "sha256": corpus_sha256,
            "seed": corpus_seed,
        },
        "configs": config_contracts(),
        "locales": LOCALES,
        "measurable_labels": MEASURABLE_LABELS,
        "label_notes": {
            "Name-first": (
                "null in schema v1 because the coverage-loop oracle labels full Name spans, "
                "not first-name subspans"
            ),
            "Name-surname": (
                "null in schema v1 because the coverage-loop oracle labels full Name spans, "
                "not surname subspans"
            ),
        },
        "host": host_info() | {"platform": platform.platform()},
        "cells": cells,
    }
    if not args.no_update:
        snapshot_path.write_text(json.dumps(snapshot, indent=2, sort_keys=False) + "\n")
    json.dump(snapshot, sys.stdout, indent=2)
    sys.stdout.write("\n")
    return 0


def repo_path(root: Path, path: Path) -> Path:
    return path if path.is_absolute() else root / path


def config_contracts() -> dict[str, Any]:
    return {
        "rule-floor-core": {
            "rulepacks": ["core"],
            "pass2": None,
            "pass3": None,
        },
        "rule-floor-extended": {
            "rulepacks": ["core", "core-extended"],
            "pass2": None,
            "pass3": None,
        },
        "pass3-kiji": {
            "rulepacks": ["core", "core-extended"],
            "pass2": None,
            "pass3": "KijiDistilbertSafetyNet observer residual",
        },
        "pass3-opf": {
            "rulepacks": ["core", "core-extended"],
            "pass2": None,
            "pass3": "OpenAiFilterSafetyNet observer residual",
        },
        "pass3-locale-aware": {
            "rulepacks": ["core", "core-extended"],
            "pass2": None,
            "pass3": "LocaleAwareModelRegistry(Kiji+OPF) routed by locale",
        },
    }


def run_config(root: Path, fixtures: list[Fixture], config: str) -> dict[str, Any]:
    features: list[str] = []
    if config == "pass3-kiji":
        features = ["--features", "safety-net-kiji"]
    elif config in {"pass3-opf", "pass3-locale-aware"}:
        features = ["--features", "safety-net-kiji,safety-net-openai"]
    command = [
        "cargo",
        "run",
        "-q",
        "-p",
        "gaze-recognizers",
        *features,
        "--example",
        "clean_for_bench",
        "--",
        "--config",
        config,
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
    if proc.stdin is None:
        raise AssertionError
    if proc.stdout is None:
        raise AssertionError
    for fixture in fixtures:
        try:
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
        except BrokenPipeError:
            break
    try:
        proc.stdin.close()
    except BrokenPipeError:
        pass
    rows = []
    for line in proc.stdout:
        rows.append(json.loads(line))
    stderr = proc.stderr.read() if proc.stderr is not None else ""
    status = proc.wait()
    if status != 0:
        return {
            "status": "unavailable",
            "reason": stderr.strip() or f"clean_for_bench exited {status}",
            "rows": [],
        }
    if len(rows) != len(fixtures):
        return {
            "status": "unavailable",
            "reason": f"clean_for_bench returned {len(rows)} fixtures, expected {len(fixtures)}",
            "rows": rows,
        }
    return {"status": "measured", "reason": None, "rows": rows}


def cells_for_config(
    config: str,
    fixtures: list[Fixture],
    result: dict[str, Any],
) -> list[dict[str, Any]]:
    if result["status"] != "measured":
        return [
            null_cell(config, locale, str(result["reason"]))
            for locale in LOCALES
        ]

    rows_by_fixture = {row["fixture_id"]: row for row in result["rows"]}
    predictions_by_locale: dict[str, list[Span]] = defaultdict(list)
    gold_by_locale: dict[str, list[Span]] = defaultdict(list)
    timings_by_locale: dict[str, list[dict[str, float | None]]] = defaultdict(list)
    bytes_by_locale: dict[str, int] = defaultdict(int)

    for fixture in fixtures:
        row = rows_by_fixture[fixture.fixture_id]
        manifest = [
            ManifestSpan(
                raw_start=int(span["raw_start"]),
                raw_end=int(span["raw_end"]),
                clean_start=int(span["clean_start"]),
                clean_end=int(span["clean_end"]),
                pii_class=map_class(str(span["class"])),
            )
            for span in row["manifest_spans"]
        ]
        predictions = [
            Span(item.raw_start, item.raw_end, item.pii_class)
            for item in manifest
        ]
        for suspect in row.get("leak_suspects", []):
            predictions.append(
                Span(
                    start=translate_clean_to_raw(int(suspect["clean_start"]), manifest),
                    end=translate_clean_to_raw(int(suspect["clean_end"]), manifest),
                    pii_class=map_class(str(suspect["class"])),
                )
            )
        gold_by_locale[fixture.locale].extend(map_gold_spans(fixture.gold_spans))
        predictions_by_locale[fixture.locale].extend(predictions)
        timings_by_locale[fixture.locale].append(row["timing"])
        bytes_by_locale[fixture.locale] += len(fixture.text.encode("utf-8"))

    cells = []
    for locale in LOCALES:
        metrics = class_metrics(gold_by_locale[locale], predictions_by_locale[locale])
        strict_recall = metrics["recall"]
        strict_leak_rate = None if strict_recall is None else rounded(1.0 - strict_recall)
        per_class = class_view(metrics["per_class"])
        cells.append(
            {
                "pipeline_config": config,
                "locale": locale,
                "status": "measured",
                "reason": None,
                "detection": {
                    "strict_recall": strict_recall,
                    "strict_leak_rate": strict_leak_rate,
                    "precision": metrics["precision"],
                    "f1": metrics["f1"],
                    "per_class_recall": per_class,
                    "raw_per_class": metrics["per_class"],
                },
                "performance": performance_summary(
                    config,
                    timings_by_locale[locale],
                    bytes_by_locale[locale],
                ),
            }
        )
    return cells


def null_cell(config: str, locale: str, reason: str) -> dict[str, Any]:
    return {
        "pipeline_config": config,
        "locale": locale,
        "status": "unavailable",
        "reason": reason,
        "detection": None,
        "performance": None,
    }


def map_gold_spans(spans: list[Span]) -> list[Span]:
    return [Span(span.start, span.end, map_class(span.pii_class)) for span in spans]


def map_class(value: str) -> str:
    return CLASS_MAP.get(value, value)


def class_view(per_class: dict[str, Any]) -> dict[str, Any]:
    result = {label: None for label in MEASURABLE_LABELS}
    for label in ["Email", "IBAN", "credit_card", "phone", "postal_code"]:
        result[label] = per_class.get(label, {}).get("recall")
    result["Name-first"] = None
    result["Name-surname"] = None
    return result


def performance_summary(
    config: str,
    samples: list[dict[str, float | None]],
    byte_count: int,
) -> dict[str, Any]:
    totals = [float(sample["total_ms"]) for sample in samples]
    pass1 = [float(sample["pass1_ms"]) for sample in samples]
    pass3 = [float(sample["pass3_ms"]) for sample in samples]
    pass3_mean = statistics.mean(pass3) if pass3 else None
    if config in {"rule-floor-core", "rule-floor-extended"}:
        pass3_mean = 0.0
    total_ms = sum(totals)
    return {
        "cold_start_ms": rounded(totals[0] if totals else None),
        "mean_ms": rounded(statistics.mean(totals) if totals else None),
        "p50_ms": rounded(statistics.median(totals) if totals else None),
        "p95_ms": rounded(percentile(sorted(totals), 0.95) if totals else None),
        "p99_ms": rounded(percentile(sorted(totals), 0.99) if totals else None),
        "throughput_bytes_per_sec": rounded(
            None if total_ms == 0 else byte_count / (total_ms / 1000.0)
        ),
        "pass_timing": {
            "pass1_ms": rounded(statistics.mean(pass1) if pass1 else None),
            "pass2_ms": None,
            "pass2_reason": "no Pass-2 NER is enabled in the five locked configs",
            "pass3_ms": rounded(pass3_mean),
            "measurement_scope": (
                "full pipeline wall clock from clean_for_bench; pass1 is matching "
                "rule-floor wall clock; pass3 is full minus rule-floor delta"
            ),
        },
    }


def translate_clean_to_raw(offset: int, manifest: list[ManifestSpan]) -> int:
    delta = 0
    for span in sorted(manifest, key=lambda item: (item.clean_start, item.clean_end)):
        if span.clean_end <= offset:
            delta += (span.raw_end - span.raw_start) - (span.clean_end - span.clean_start)
            continue
        break
    return offset + delta


if __name__ == "__main__":
    raise SystemExit(main())
