#!/usr/bin/env python3
"""Config-driven multi-model NER benchmark runner."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    import tomli as tomllib  # type: ignore[no-redef]

from safety_net_bench_lib import (
    LOCALE_TAGS,
    LatencyRecorder,
    Span,
    clean_fixtures,
    load_fixtures,
    rounded,
    score_direct,
    score_observer,
    sha256_file,
)


EVALUATED_LOCALES = ["Global", "EnUs", "DeDe"]


@dataclass(frozen=True)
class ModelConfig:
    id: str
    display_name: str
    hf_repo: str
    hf_commit: str
    bundle_sha: str
    license: str
    runtime: str
    wrapper: Path
    model_file: str | None
    label_map: dict[str, str]
    native_locales: list[str]
    label_set: list[str]
    fetch_script: Path | None
    default_checkpoint: Path | None
    shippable_default: bool
    default_caveat: str | None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--config", type=Path, default=Path("benches/ner_models.toml"))
    parser.add_argument("--coverage-report", type=Path, default=Path("target/coverage-report.json"))
    parser.add_argument(
        "--corpus-dir",
        type=Path,
        default=Path("crates/gaze-recognizers/testdata/coverage-loop/corpus"),
    )
    parser.add_argument(
        "--snapshot",
        type=Path,
        default=Path("crates/gaze-recognizers/benches/ner_models_snapshot.json"),
    )
    parser.add_argument("--cache-dir", type=Path, default=Path(".huggingface-cache"))
    parser.add_argument("--python", type=Path, default=Path(sys.executable))
    parser.add_argument("--device", default="cpu", choices=["cpu"])
    parser.add_argument("--model", action="append", help="Run only the selected model id.")
    parser.add_argument("--mode", choices=["direct", "observer-residual", "all"], default="all")
    parser.add_argument("--skip-download", action="store_true")
    parser.add_argument("--no-update", action="store_true")
    return parser.parse_args()


def repo_path(root: Path, path: Path) -> Path:
    return path.expanduser() if path.is_absolute() else root / path


def load_config(path: Path) -> list[ModelConfig]:
    raw = tomllib.loads(path.read_text(encoding="utf-8"))
    models: list[ModelConfig] = []
    for item in raw.get("model", []):
        fetch_script = item.get("fetch_script")
        default_checkpoint = item.get("default_checkpoint")
        models.append(
            ModelConfig(
                id=str(item["id"]),
                display_name=str(item["display_name"]),
                hf_repo=str(item["hf_repo"]),
                hf_commit=str(item["hf_commit"]),
                bundle_sha=str(item["bundle_sha"]),
                license=str(item["license"]),
                runtime=str(item["runtime"]),
                wrapper=Path(str(item["wrapper"])),
                model_file=item.get("model_file"),
                label_map={str(k): str(v) for k, v in item["label_map"].items()},
                native_locales=[str(v) for v in item["native_locales"]],
                label_set=[str(v) for v in item["label_set"]],
                fetch_script=Path(str(fetch_script)) if fetch_script else None,
                default_checkpoint=Path(str(default_checkpoint)).expanduser()
                if default_checkpoint
                else None,
                shippable_default=bool(item.get("shippable_default", True)),
                default_caveat=item.get("default_caveat"),
            )
        )
    if not models:
        raise ValueError(f"no [[model]] entries in {path}")
    return models


def model_checkpoint(root: Path, cache_dir: Path, model: ModelConfig) -> Path:
    if model.default_checkpoint is not None:
        return model.default_checkpoint
    return cache_dir / "models" / model.id


def checkpoint_complete(path: Path, model: ModelConfig) -> bool:
    if not path.is_dir():
        return False
    if model.runtime == "onnxruntime":
        model_file = model.model_file or "model.onnx"
        has_tokenizer = (path / "tokenizer.json").is_file() or (path / "vocab.txt").is_file()
        return (path / model_file).is_file() and has_tokenizer
    has_config = (path / "config.json").is_file()
    has_tokenizer = (path / "tokenizer.json").is_file() or (path / "vocab.txt").is_file()
    has_weights = any(
        (path / name).is_file()
        for name in ["model.safetensors", "pytorch_model.bin", "tf_model.h5", "flax_model.msgpack"]
    )
    return has_config and has_tokenizer and has_weights


def ensure_checkpoint(
    root: Path,
    cache_dir: Path,
    model: ModelConfig,
    skip_download: bool,
    python: Path,
) -> Path:
    checkpoint = model_checkpoint(root, cache_dir, model)
    if checkpoint_complete(checkpoint, model):
        return checkpoint
    if skip_download:
        raise FileNotFoundError(f"{model.id}: missing checkpoint {checkpoint}")

    if model.fetch_script is not None:
        script = repo_path(root, model.fetch_script)
        subprocess.run([str(script), str(checkpoint)], cwd=root, check=True)
        return checkpoint

    checkpoint.parent.mkdir(parents=True, exist_ok=True)
    code = (
        "from huggingface_hub import snapshot_download; "
        "import sys; "
        "snapshot_download(repo_id=sys.argv[1], revision=sys.argv[2], "
        "local_dir=sys.argv[3], local_dir_use_symlinks=False, "
        "allow_patterns=['*.json','*.txt','*.model','*.safetensors','*.bin','tokenizer*','vocab*','merges.txt','README.md'])"
    )
    subprocess.run(
        [str(python), "-c", code, model.hf_repo, model.hf_commit, str(checkpoint)],
        cwd=root,
        env={**os.environ, "HF_HUB_DISABLE_XET": "1"},
        check=True,
    )
    return checkpoint


def sha256_dir(path: Path) -> str:
    digest = hashlib.sha256()
    for child in sorted(p for p in path.rglob("*") if p.is_file()):
        rel = child.relative_to(path).as_posix().encode("utf-8")
        digest.update(rel)
        digest.update(b"\0")
        digest.update(sha256_file(child).encode("ascii"))
        digest.update(b"\0")
    return digest.hexdigest()


def run_model(
    root: Path,
    python: Path,
    model: ModelConfig,
    checkpoint: Path,
    fixture_id: str,
    text: str,
    device: str,
    latency: LatencyRecorder,
) -> list[Span]:
    command = [
        str(python),
        str(repo_path(root, model.wrapper)),
        "--format",
        "json",
        "--output-mode",
        "typed",
    ]
    if model.runtime == "onnxruntime":
        command.extend(["--model-dir", str(checkpoint)])
        if model.model_file is not None:
            command.extend(["--model-file", model.model_file])
    else:
        command.extend(["--checkpoint", str(checkpoint), "--device", device])

    start = time.perf_counter()
    proc = subprocess.run(
        command,
        input=text,
        text=True,
        encoding="utf-8",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    latency.record((time.perf_counter() - start) * 1000.0, len(text.encode("utf-8")))
    if proc.returncode != 0:
        raise RuntimeError(
            f"{model.id}/{fixture_id}: runner failed with {proc.returncode}: {proc.stderr.strip()}"
        )

    spans: list[Span] = []
    for raw in json.loads(proc.stdout):
        label = str(raw["label"])
        pii_class = model.label_map.get(label)
        if pii_class is None:
            raise RuntimeError(f"{model.id}/{fixture_id}: unsupported label {label!r}")
        spans.append(Span(int(raw["start"]), int(raw["end"]), pii_class))
    return spans


def run_one_model(
    root: Path,
    args: argparse.Namespace,
    model: ModelConfig,
    checkpoint: Path,
    fixtures: list[Any],
    clean: dict[str, Any] | None,
) -> dict[str, Any]:
    requested_modes = ["direct", "observer-residual"] if args.mode == "all" else [args.mode]
    metrics_by_mode: dict[str, Any] = {}
    latency = LatencyRecorder(True, [])

    for mode in requested_modes:
        predictions: dict[str, list[Span]] = {}
        mode_latency = latency if mode == "direct" else LatencyRecorder(False, [])
        for index, fixture in enumerate(fixtures, start=1):
            input_text = fixture.text
            if mode == "observer-residual":
                if clean is None:
                    raise AssertionError
                input_text = clean[fixture.fixture_id].clean_text
            predictions[fixture.fixture_id] = run_model(
                root,
                args.python,
                model,
                checkpoint,
                fixture.fixture_id,
                input_text,
                args.device,
                mode_latency,
            )
            print(
                f"{model.id}: {mode} {index}/{len(fixtures)} {fixture.fixture_id} "
                f"predictions={len(predictions[fixture.fixture_id])}",
                file=sys.stderr,
                flush=True,
            )

        matrix_mode = "direct_detector" if mode == "direct" else "observer_residual"
        metrics_by_mode[matrix_mode] = (
            score_direct(fixtures, predictions)
            if mode == "direct"
            else score_observer(fixtures, clean or {}, predictions)
        )

    return {
        "metrics_by_mode": metrics_by_mode,
        "latency": latency.summary(len(fixtures), args.device),
    }


def host_info() -> dict[str, str]:
    return {
        "os": platform.platform(),
        "machine": platform.machine(),
        "python": platform.python_version(),
    }


def write_snapshot(
    path: Path,
    models: list[ModelConfig],
    checkpoints: dict[str, Path],
    bundle_shas: dict[str, str],
    coverage_sha256: str,
    corpus_sha256: str | None,
    fixture_count: int,
    results: dict[str, Any],
) -> None:
    snapshot: dict[str, Any] = {
        "schema_version": 1,
        "benchmark": "ner-model-leaderboard",
        "status": "real_run",
        "corpus": {
            "coverage_report_sha256": coverage_sha256,
            "sha256": corpus_sha256,
            "fixture_count": fixture_count,
        },
        "run_environment": host_info(),
        "models": {},
        "cells": [],
    }

    for model in models:
        snapshot["models"][model.id] = {
            "display_name": model.display_name,
            "hf_repo": model.hf_repo,
            "hf_commit": model.hf_commit,
            "bundle_sha256_config": model.bundle_sha,
            "bundle_sha256_observed": bundle_shas[model.id],
            "license": model.license,
            "runtime": model.runtime,
            "wrapper": model.wrapper.as_posix(),
            "model_file": model.model_file,
            "checkpoint": str(checkpoints[model.id]),
            "native_locales": model.native_locales,
            "label_set": model.label_set,
            "label_map": model.label_map,
            "shippable_default": model.shippable_default,
            "default_caveat": model.default_caveat,
        }
        model_results = results[model.id]
        for mode, metrics_by_locale in model_results["metrics_by_mode"].items():
            for locale in EVALUATED_LOCALES:
                metrics = metrics_by_locale[locale]
                snapshot["cells"].append(
                    {
                        "model": model.id,
                        "locale": locale,
                        "native_locale": locale in model.native_locales,
                        "mode": mode,
                        "metrics": metrics,
                    }
                )
        if model_results["latency"] is not None:
            snapshot["models"][model.id]["latency"] = model_results["latency"]

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(snapshot, indent=2, sort_keys=False) + "\n", encoding="utf-8")


def main() -> int:
    args = parse_args()
    root = args.repo_root.resolve()
    args.config = repo_path(root, args.config)
    args.coverage_report = repo_path(root, args.coverage_report)
    args.corpus_dir = repo_path(root, args.corpus_dir)
    args.snapshot = repo_path(root, args.snapshot)
    args.cache_dir = repo_path(root, args.cache_dir)

    models = load_config(args.config)
    if args.model:
        selected = set(args.model)
        models = [model for model in models if model.id in selected]
        missing = selected - {model.id for model in models}
        if missing:
            raise ValueError(f"unknown model id(s): {', '.join(sorted(missing))}")

    if not args.coverage_report.is_file():
        raise FileNotFoundError(f"missing coverage report: {args.coverage_report}")

    fixtures = load_fixtures(args.corpus_dir)
    report = json.loads(args.coverage_report.read_text(encoding="utf-8"))
    expected_count = int(report["totals"]["fixture_count"])
    if expected_count != len(fixtures):
        raise ValueError(f"coverage report fixture_count {expected_count} != corpus {len(fixtures)}")

    build_manifest = args.corpus_dir.parent / "build-manifest.json"
    corpus_sha256 = None
    if build_manifest.is_file():
        corpus_sha256 = str(json.loads(build_manifest.read_text(encoding="utf-8"))["corpus_sha256"])

    clean = clean_fixtures(root, fixtures) if args.mode in {"observer-residual", "all"} else None
    checkpoints: dict[str, Path] = {}
    bundle_shas: dict[str, str] = {}
    results: dict[str, Any] = {}

    for model in models:
        checkpoint = ensure_checkpoint(root, args.cache_dir, model, args.skip_download, args.python)
        observed_sha = sha256_dir(checkpoint)
        print(f"{model.id}: checkpoint={checkpoint} sha256={observed_sha}", file=sys.stderr)
        checkpoints[model.id] = checkpoint
        bundle_shas[model.id] = observed_sha
        results[model.id] = run_one_model(root, args, model, checkpoint, fixtures, clean)

    if not args.no_update:
        write_snapshot(
            args.snapshot,
            models,
            checkpoints,
            bundle_shas,
            sha256_file(args.coverage_report),
            corpus_sha256,
            len(fixtures),
            results,
        )

    json.dump(
        {
            "models": [model.id for model in models],
            "snapshot": str(args.snapshot),
            "results": results,
        },
        sys.stdout,
        indent=2,
    )
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
