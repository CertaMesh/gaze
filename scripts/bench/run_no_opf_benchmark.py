#!/usr/bin/env python3
"""Canonical local Dataiku EN/DE plus A4 negative-corpus benchmark runner."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Sequence

import dataiku_en_de_gaze_bench as dataiku
import gaze_bench_score as score


QUICK_DOCUMENTS = 256
DEFAULT_WARMUPS = 1
DEFAULT_MEASURED_REPETITIONS = 1
DEFAULT_PERFORMANCE_TOLERANCE_PERCENT = 20.0
BASELINE_CONFIRMATION = "I_HAVE_REVIEWED_FULL_RESULTS"
NEGATIVE_CORPUS = Path("crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl")
MODEL_CONFIG = Path("crates/gaze-recognizers/benches/ner_models.toml")
NO_OPF_MODEL_CONFIG = Path("scripts/bench/no_opf_models.toml")
DEFAULT_DAVLAN_MODEL = Path("~/.local/share/gaze/models/davlan-mbert-ner-hrl")
DEFAULT_KIJI_MODEL = Path("~/.local/share/gaze/models/kiji-distilbert")
DAVLAN_RUNTIME_ARTIFACTS = frozenset(
    {
        "config.json",
        "labels.json",
        "model.onnx",
        "special_tokens_map.json",
        "tokenizer.json",
        "tokenizer_config.json",
        "vocab.txt",
    }
)
DAVLAN_BUNDLE_SURFACE = DAVLAN_RUNTIME_ARTIFACTS | {"SHA256SUMS"}


class RunnerError(RuntimeError):
    code = "runner_error"


class ModelBundleError(RunnerError):
    code = "model_bundle_error"


class CandidateError(RunnerError):
    code = "candidate_error"


class BaselineError(RunnerError):
    code = "baseline_error"


class BaselineAcceptanceError(RunnerError):
    code = "baseline_acceptance_error"


class RepetitionMismatchError(RunnerError):
    code = "repetition_mismatch"


@dataclass(frozen=True)
class ModelPin:
    model_id: str
    path: Path
    digest_kind: str
    expected_sha256: str
    hf_repo: str | None = None
    hf_commit: str | None = None
    runtime: str | None = None


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("profile", choices=("quick", "full"))
    parser.add_argument("--seed", type=int, default=score.DEFAULT_SAMPLE_SEED)
    parser.add_argument("--quick-documents", type=int, default=QUICK_DOCUMENTS)
    parser.add_argument(
        "--dataset",
        type=Path,
        default=Path("target/bench-data/dataiku-en-de/test.parquet"),
    )
    parser.add_argument("--negative-corpus", type=Path, default=NEGATIVE_CORPUS)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("target/bench-data/no-opf"),
    )
    parser.add_argument(
        "--model-dir",
        type=Path,
        default=Path(
            os.environ.get("GAZE_NER_MODEL_DIR", str(DEFAULT_DAVLAN_MODEL))
        ).expanduser(),
    )
    parser.add_argument(
        "--kiji-model-dir",
        type=Path,
        default=Path(
            os.environ.get("GAZE_KIJI_DISTILBERT_MODEL_DIR", str(DEFAULT_KIJI_MODEL))
        ).expanduser(),
    )
    parser.add_argument("--threshold", type=float, default=0.3)
    parser.add_argument("--warmups", type=int, default=DEFAULT_WARMUPS)
    parser.add_argument(
        "--measured-repetitions",
        type=int,
        default=DEFAULT_MEASURED_REPETITIONS,
    )
    parser.add_argument("--compare-baseline", type=Path)
    parser.add_argument("--accept-baseline", type=Path)
    parser.add_argument("--accept-baseline-confirm")
    parser.add_argument(
        "--performance-tolerance-percent",
        type=float,
        default=DEFAULT_PERFORMANCE_TOLERANCE_PERCENT,
    )
    parser.add_argument("--performance-gating", action="store_true")
    parser.add_argument("--no-download", action="store_true")
    parser.add_argument("--skip-build", action="store_true")
    return parser.parse_args(argv)


def repo_path(repo_root: Path, path: Path) -> Path:
    return path if path.is_absolute() else repo_root / path


def load_model_pins(
    repo_root: Path, davlan_path: Path, kiji_path: Path
) -> tuple[ModelPin, ModelPin]:
    config_path = repo_root / MODEL_CONFIG
    raw = tomllib.loads(config_path.read_text(encoding="utf-8"))
    models = {item["id"]: item for item in raw.get("model", [])}
    no_opf_config_path = repo_root / NO_OPF_MODEL_CONFIG
    no_opf_raw = tomllib.loads(no_opf_config_path.read_text(encoding="utf-8"))
    try:
        pass2_ner = no_opf_raw["pass2_ner"]
        davlan_id = str(pass2_ner["id"])
        davlan_repo = str(pass2_ner["hf_repo"])
        davlan_commit = str(pass2_ner["hf_commit"])
        davlan_sha = str(pass2_ner["bundle_sha"])
        davlan_runtime = str(pass2_ner["runtime"])
        davlan_model_file = str(pass2_ner["model_file"])
        davlan_fetch_script = str(pass2_ner["fetch_script"])
        davlan_artifacts = frozenset(
            str(item) for item in pass2_ner["required_artifacts"]
        )
        kiji_sha = str(models["kiji-distilbert"]["bundle_sha"])
    except (KeyError, TypeError) as error:
        raise ModelBundleError(
            "required model pin is missing or invalid in "
            f"{no_opf_config_path} or {config_path}: {error}"
        ) from error
    if (
        davlan_id != "davlan-mbert-ner-hrl-onnx"
        or davlan_runtime != "onnxruntime"
        or davlan_model_file != "model.onnx"
        or davlan_fetch_script != "scripts/fetch/fetch-ner-model.sh"
        or davlan_artifacts != DAVLAN_RUNTIME_ARTIFACTS
    ):
        raise ModelBundleError(
            f"production pass2 NER contract is invalid in {no_opf_config_path}"
        )
    return (
        ModelPin(
            davlan_id,
            davlan_path,
            "SHA256SUMS",
            davlan_sha,
            hf_repo=davlan_repo,
            hf_commit=davlan_commit,
            runtime=davlan_runtime,
        ),
        ModelPin("kiji-distilbert", kiji_path, "SHA256SUMS", kiji_sha),
    )


def _validate_davlan_manifest_surface(pin: ModelPin) -> dict[str, object]:
    manifest = pin.path / "SHA256SUMS"
    if not manifest.is_file() or manifest.is_symlink():
        raise ModelBundleError(
            f"{pin.model_id}: required checksum manifest is missing or unsafe: "
            f"{manifest}; install the pinned local model bundle before running"
        )
    actual_manifest_sha = score.sha256_file(manifest)
    if actual_manifest_sha != pin.expected_sha256:
        raise ModelBundleError(
            f"{pin.model_id}: checksum manifest digest mismatch at {manifest}; "
            f"expected {pin.expected_sha256}, got {actual_manifest_sha}"
        )

    entries = list(pin.path.iterdir())
    actual_surface = {entry.name for entry in entries}
    if actual_surface != DAVLAN_BUNDLE_SURFACE:
        missing = sorted(DAVLAN_BUNDLE_SURFACE - actual_surface)
        unexpected = sorted(actual_surface - DAVLAN_BUNDLE_SURFACE)
        raise ModelBundleError(
            f"{pin.model_id}: unexpected bundle surface at {pin.path}; "
            f"missing={missing}, unexpected={unexpected}"
        )
    for entry in entries:
        if entry.is_symlink():
            raise ModelBundleError(
                f"{pin.model_id}: bundle surface contains a symlink: {entry}"
            )
        if not entry.is_file():
            raise ModelBundleError(
                f"{pin.model_id}: unexpected bundle surface entry: {entry}"
            )

    try:
        manifest_lines = manifest.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeError) as error:
        raise ModelBundleError(
            f"{pin.model_id}: cannot read checksum manifest {manifest}: {error}"
        ) from error
    listed_artifacts: list[tuple[str, str]] = []
    for line_number, line in enumerate(manifest_lines, start=1):
        fields = line.split()
        if (
            len(fields) != 2
            or len(fields[0]) != 64
            or any(character not in "0123456789abcdef" for character in fields[0])
        ):
            raise ModelBundleError(
                f"{pin.model_id}: malformed {manifest.name} line {line_number}"
            )
        expected, relative_raw = fields
        relative = Path(relative_raw)
        if relative.is_absolute() or ".." in relative.parts:
            raise ModelBundleError(
                f"{pin.model_id}: unsafe path in {manifest.name} line {line_number}"
            )
        listed_artifacts.append((relative_raw, expected))
    if (
        len(listed_artifacts) != len(DAVLAN_RUNTIME_ARTIFACTS)
        or {name for name, _ in listed_artifacts} != DAVLAN_RUNTIME_ARTIFACTS
    ):
        raise ModelBundleError(
            f"{pin.model_id}: {manifest.name} must list exactly the canonical "
            "seven ONNX runtime artifacts"
        )
    for relative_raw, expected in listed_artifacts:
        artifact = pin.path / relative_raw
        if not artifact.is_file() or artifact.is_symlink():
            raise ModelBundleError(
                f"{pin.model_id}: required artifact is missing or unsafe: {artifact}"
            )
        actual = score.sha256_file(artifact)
        if actual != expected:
            raise ModelBundleError(
                f"{pin.model_id}: artifact digest mismatch for {artifact}; "
                f"expected {expected}, got {actual}"
            )
    return {
        "model_id": pin.model_id,
        "hf_repo": pin.hf_repo,
        "hf_commit": pin.hf_commit,
        "runtime": pin.runtime,
        "digest_kind": pin.digest_kind,
        "bundle_sha": pin.expected_sha256,
        "expected_sha256": pin.expected_sha256,
        "observed_sha256": actual_manifest_sha,
        "verified_artifacts": len(listed_artifacts),
    }


def _validate_checksum_manifest(pin: ModelPin) -> dict[str, object]:
    manifest = pin.path / pin.digest_kind
    if not manifest.is_file():
        raise ModelBundleError(
            f"{pin.model_id}: required checksum manifest is missing: {manifest}; "
            "install the pinned local model bundle before running"
        )
    actual_manifest_sha = score.sha256_file(manifest)
    if actual_manifest_sha != pin.expected_sha256:
        raise ModelBundleError(
            f"{pin.model_id}: checksum manifest digest mismatch at {manifest}; "
            f"expected {pin.expected_sha256}, got {actual_manifest_sha}"
        )
    checked = 0
    for line_number, line in enumerate(
        manifest.read_text(encoding="utf-8").splitlines(), start=1
    ):
        if not line:
            continue
        fields = line.split()
        if len(fields) != 2 or len(fields[0]) != 64:
            raise ModelBundleError(
                f"{pin.model_id}: malformed {manifest.name} line {line_number}"
            )
        expected, relative_raw = fields
        relative = Path(relative_raw)
        if relative.is_absolute() or ".." in relative.parts:
            raise ModelBundleError(
                f"{pin.model_id}: unsafe path in {manifest.name} line {line_number}"
            )
        artifact = pin.path / relative
        if not artifact.is_file() or artifact.is_symlink():
            raise ModelBundleError(
                f"{pin.model_id}: required artifact is missing or unsafe: {artifact}"
            )
        actual = score.sha256_file(artifact)
        if actual != expected:
            raise ModelBundleError(
                f"{pin.model_id}: artifact digest mismatch for {artifact}; "
                f"expected {expected}, got {actual}"
            )
        checked += 1
    if checked == 0:
        raise ModelBundleError(f"{pin.model_id}: checksum manifest is empty")
    return {
        "model_id": pin.model_id,
        "digest_kind": pin.digest_kind,
        "expected_sha256": pin.expected_sha256,
        "observed_sha256": actual_manifest_sha,
        "verified_artifacts": checked,
    }


def validate_model_bundle(pin: ModelPin) -> dict[str, object]:
    if not pin.path.is_dir():
        raise ModelBundleError(
            f"{pin.model_id}: model directory does not exist: {pin.path}; "
            "install the pinned local model bundle before running"
        )
    if pin.model_id == "davlan-mbert-ner-hrl-onnx":
        if pin.path.is_symlink():
            raise ModelBundleError(
                f"{pin.model_id}: model directory is a symlink: {pin.path}"
            )
        return _validate_davlan_manifest_surface(pin)
    return _validate_checksum_manifest(pin)


def validate_required_models(
    repo_root: Path, davlan_path: Path, kiji_path: Path
) -> list[dict[str, object]]:
    return [
        validate_model_bundle(pin)
        for pin in load_model_pins(repo_root, davlan_path, kiji_path)
    ]


def load_negative_documents(
    path: Path,
) -> tuple[list[score.Document], dict[str, object]]:
    if not path.is_file():
        raise CandidateError(f"negative corpus is missing: {path}")
    documents: list[score.Document] = []
    languages: dict[str, int] = {}
    categories: dict[str, int] = {}
    for line_number, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), 1
    ):
        try:
            row = json.loads(line)
        except json.JSONDecodeError as error:
            raise CandidateError(
                f"negative corpus JSON is invalid at line {line_number}: {error}"
            ) from error
        if not isinstance(row, dict):
            raise CandidateError(f"negative corpus line {line_number} is not an object")
        uid = row.get("id")
        language = row.get("language")
        category = row.get("category")
        text = row.get("text")
        if not all(isinstance(value, str) and value for value in (uid, category, text)):
            raise CandidateError(
                f"negative corpus line {line_number} has invalid id/category/text"
            )
        if language not in {"en", "de"}:
            raise CandidateError(
                f"negative corpus line {line_number} has unsupported language {language!r}"
            )
        if row.get("oracle_spans") != []:
            raise CandidateError(
                f"negative corpus line {line_number} is not a negative-only fixture"
            )
        documents.append(
            score.Document(
                uid=uid,
                text=text,
                language=language,
                region="",
                source_dataset="gaze-a4-negative-corpus",
                spans=(),
                negative_category=category,
            )
        )
        languages[language] = languages.get(language, 0) + 1
        categories[category] = categories.get(category, 0) + 1
    if not documents:
        raise CandidateError("negative corpus contains no documents")
    return documents, {
        "documents": len(documents),
        "languages": dict(sorted(languages.items())),
        "categories": dict(sorted(categories.items())),
        "sha256": score.sha256_file(path),
    }


def composite_dataset_report(
    dataiku_report: Mapping[str, object], negative_report: Mapping[str, object]
) -> tuple[dict[str, object], dict[str, object]]:
    dataiku_integrity = dataiku_report["integrity"]
    assert isinstance(dataiku_integrity, dict)
    component_digests = {
        "dataiku": str(dataiku_integrity["sha256"]),
        "negative_corpus": str(negative_report["sha256"]),
    }
    composite_digest = hashlib.sha256(
        json.dumps(component_digests, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    metadata = {
        "repository": f"{dataiku.DATASET_REPO}+gaze",
        "revision": f"{dataiku.DATASET_REVISION}+a4-negative-v1",
        "file": f"{dataiku.DATASET_FILE}+{NEGATIVE_CORPUS.as_posix()}",
        "license": "Apache-2.0 + CC0-1.0 synthetic",
        "synthetic_only": True,
        "reserved_from_training": True,
        "selection_policy": "complete EN/DE holdout plus complete committed A4 negatives",
    }
    report = {
        "integrity": {
            "sha256": composite_digest,
            "component_sha256": component_digests,
        },
        "components": {
            "dataiku": copy.deepcopy(dict(dataiku_report)),
            "negative_corpus": copy.deepcopy(dict(negative_report)),
        },
    }
    return metadata, report


def build_no_opf_environment(source: Mapping[str, str]) -> dict[str, str]:
    def is_opf_key(key: str) -> bool:
        upper = key.upper()
        return (
            upper.startswith("OPF_")
            or upper.startswith("GAZE_OPF_")
            or "OPENAI_FILTER" in upper
        )

    return {key: value for key, value in source.items() if not is_opf_key(key)}


def execute_measurements(
    *,
    repo_root: Path,
    binary: Path,
    documents: Sequence[score.Document],
    davlan_model: Path,
    kiji_model: Path,
    threshold: float,
    diagnostics_dir: Path,
    warmup_count: int,
    measured_repetitions: int,
    source_environment: Mapping[str, str] | None = None,
) -> tuple[list[dict[str, object]], list[dict[str, object]]]:
    if measured_repetitions <= 0:
        raise CandidateError("measured repetitions must be positive")
    if warmup_count < 0:
        raise CandidateError("warmup count must be non-negative")
    configs = tuple(score.DEFAULT_CONFIGS)
    if any("opf" in config.lower() for config in configs):
        raise CandidateError("canonical no-OPF config set unexpectedly contains OPF")
    environment = build_no_opf_environment(source_environment or os.environ)
    repetition_runs: list[list[dict[str, object]]] = []
    provenance: list[dict[str, object]] = []
    for repetition in range(1, measured_repetitions + 1):
        current: list[dict[str, object]] = []
        for config in configs:
            run = score.run_config(
                repo_root,
                binary,
                config,
                documents,
                davlan_model,
                kiji_model,
                None,
                None,
                None,
                threshold,
                diagnostics_dir / f"repetition-{repetition}",
                base_environment=environment,
                warmup_count=warmup_count,
            )
            current.append(run)
        repetition_runs.append(current)
        provenance.append(
            {
                "repetition": repetition,
                "runs": [
                    {
                        "config": run["config"],
                        "cold_start_to_first_validated_response_ms": run["process"][
                            "cold_start_to_first_validated_response_ms"
                        ],
                        "warmup_count": run["process"]["warmup_count"],
                        "discarded_warmup_samples": run["process"][
                            "discarded_warmup_samples"
                        ],
                        "clean_ms": run["latency_ms"].get("clean_ms"),
                    }
                    for run in current
                ],
            }
        )
    canonical = repetition_runs[0]
    canonical_counts = {
        str(run["config"]): {
            "counts": score._run_correctness_counts(run),
            "scored_population": copy.deepcopy(run["scored_population"]),
            "failed_closed_population": copy.deepcopy(run["failed_closed_population"]),
        }
        for run in canonical
    }
    for repetition, runs in enumerate(repetition_runs[1:], start=2):
        counts = {
            str(run["config"]): {
                "counts": score._run_correctness_counts(run),
                "scored_population": copy.deepcopy(run["scored_population"]),
                "failed_closed_population": copy.deepcopy(
                    run["failed_closed_population"]
                ),
            }
            for run in runs
        }
        if counts != canonical_counts:
            raise RepetitionMismatchError(
                f"measured repetition {repetition} disagrees with repetition 1 "
                "on integer correctness counts or identified populations"
            )
    return canonical, provenance


def load_scorecard(path: Path, label: str) -> dict[str, object]:
    if not path.is_file():
        raise BaselineError(f"{label} scorecard is missing: {path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise BaselineError(f"cannot read {label} scorecard {path}: {error}") from error
    if not isinstance(value, dict):
        raise BaselineError(f"{label} scorecard must be a JSON object")
    return value


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def diagnostics(scorecard: Mapping[str, object]) -> dict[str, object]:
    runs = scorecard.get("runs")
    if not isinstance(runs, list) or not runs:
        raise CandidateError("candidate scorecard has no diagnostic runs")
    return {
        "schema_version": score.SCORECARD_SCHEMA_VERSION,
        "runs": [
            {
                "config": run["config"],
                "scored_population": run["scored_population"],
                "failed_closed_population": run["failed_closed_population"],
                "per_language": run["per_language"],
                "per_label": run["per_label_recall"],
                "per_negative_category": run["per_negative_category"],
                "pipeline_availability": run["pipeline_availability"],
                "pipeline_contract": run["pipeline_contract"],
            }
            for run in runs
        ],
    }


def markdown_summary(
    scorecard: Mapping[str, object],
    regression: Mapping[str, object],
    readiness: Mapping[str, object],
    performance: Mapping[str, object],
) -> str:
    lines = [
        "# Gaze canonical no-OPF benchmark",
        "",
        f"- Regression: **{regression['status']}**",
        f"- Release readiness: **{readiness['status']}**",
        f"- Performance: **{performance['disposition']} / {performance['status']}**",
        "",
        "| Config | Attempted | Failed closed | Leaked bytes | Restore failures "
        "| Strict rejections | Residual suspects | clean_ms p95 |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for run in scorecard["runs"]:
        counts = score._run_correctness_counts(run)
        clean_p95 = run["latency_ms"].get("clean_ms", {}).get("p95")
        clean_display = f"{float(clean_p95):.3f}" if clean_p95 is not None else "n/a"
        lines.append(
            f"| {run['config']} | {counts['attempted_documents']} | "
            f"{counts['failed_closed_documents']} | "
            f"{counts['leaked_labeled_utf8_bytes']} | "
            f"{counts['restore_failures']} | {counts['strict_rejections']} | "
            f"{counts['residual_suspects']} | "
            f"{clean_display} |"
        )
    return "\n".join(lines) + "\n"


def _status_document(kind: str, result: Mapping[str, object]) -> dict[str, object]:
    passed = bool(result["passed"])
    return {
        "schema_version": 1,
        "kind": kind,
        "status": "passed" if passed else "failed",
        "passed": passed,
        "failures": copy.deepcopy(result.get("failures", [])),
    }


def accept_baseline(
    args: argparse.Namespace,
    candidate: Mapping[str, object],
    regression: Mapping[str, object],
    readiness: Mapping[str, object],
) -> None:
    if args.accept_baseline is None:
        if args.accept_baseline_confirm is not None:
            raise BaselineAcceptanceError(
                "--accept-baseline-confirm requires --accept-baseline"
            )
        return
    if args.profile != "full":
        raise BaselineAcceptanceError(
            "only a full-profile result may become a baseline"
        )
    if args.accept_baseline_confirm != BASELINE_CONFIRMATION:
        raise BaselineAcceptanceError(
            "baseline acceptance requires --accept-baseline-confirm "
            f"{BASELINE_CONFIRMATION}"
        )
    initializing = args.compare_baseline is None
    if initializing and args.accept_baseline.exists():
        raise BaselineAcceptanceError(
            "refusing to overwrite an existing baseline without --compare-baseline"
        )
    if not initializing and (
        args.compare_baseline.resolve() != args.accept_baseline.resolve()
    ):
        raise BaselineAcceptanceError(
            "--compare-baseline and --accept-baseline must name the same reviewed file"
        )
    regression_failed = not initializing and regression.get("passed") is not True
    if regression_failed or readiness.get("passed") is not True:
        raise BaselineAcceptanceError(
            "refusing to accept a regressing or not-release-ready candidate"
        )
    write_json(args.accept_baseline, candidate)


def validate_cli_guards(args: argparse.Namespace) -> None:
    if args.performance_gating and args.compare_baseline is None:
        raise BaselineError("--performance-gating requires --compare-baseline")
    if args.accept_baseline is None:
        if args.accept_baseline_confirm is not None:
            raise BaselineAcceptanceError(
                "--accept-baseline-confirm requires --accept-baseline"
            )
        return
    if args.profile != "full":
        raise BaselineAcceptanceError(
            "only a full-profile result may become a baseline"
        )
    if args.accept_baseline_confirm != BASELINE_CONFIRMATION:
        raise BaselineAcceptanceError(
            "baseline acceptance requires --accept-baseline-confirm "
            f"{BASELINE_CONFIRMATION}"
        )
    if args.compare_baseline is None and args.accept_baseline.exists():
        raise BaselineAcceptanceError(
            "refusing to overwrite an existing baseline without --compare-baseline"
        )
    if args.compare_baseline is not None and (
        args.compare_baseline.resolve() != args.accept_baseline.resolve()
    ):
        raise BaselineAcceptanceError(
            "--compare-baseline and --accept-baseline must name the same reviewed file"
        )


def run(args: argparse.Namespace) -> int:
    repo_root = Path(__file__).resolve().parents[2]
    if args.compare_baseline is not None:
        args.compare_baseline = repo_path(repo_root, args.compare_baseline)
    if args.accept_baseline is not None:
        args.accept_baseline = repo_path(repo_root, args.accept_baseline)
    validate_cli_guards(args)
    output_dir = repo_path(repo_root, args.output_dir) / args.profile
    dataset_path = repo_path(repo_root, args.dataset)
    negative_path = repo_path(repo_root, args.negative_corpus)
    davlan_model = args.model_dir.expanduser().resolve()
    kiji_model = args.kiji_model_dir.expanduser().resolve()

    model_provenance = validate_required_models(repo_root, davlan_model, kiji_model)
    if dataset_path.is_file():
        dataiku.verify_dataset(dataset_path)
    elif args.no_download:
        raise CandidateError(f"Dataiku dataset is missing: {dataset_path}")
    else:
        dataiku.fetch_dataset(dataset_path)
    positive_documents, dataiku_report = dataiku.load_documents(dataset_path)
    negative_documents, negative_report = load_negative_documents(negative_path)
    available_documents = positive_documents + negative_documents
    max_documents = args.quick_documents if args.profile == "quick" else None
    if max_documents is not None and max_documents <= 0:
        raise CandidateError("quick document count must be positive")
    documents, sampling_report = score.stratified_sample(
        available_documents, max_documents, seed=args.seed
    )
    if args.profile == "full" and len(documents) != len(available_documents):
        raise CandidateError("full profile did not select the complete corpus")

    binary = (
        repo_root / "target/debug/examples/clean_for_bench"
        if args.skip_build
        else dataiku.build_binary(repo_root, tuple(score.DEFAULT_CONFIGS))
    )
    if not binary.is_file():
        raise CandidateError(f"benchmark binary is missing: {binary}")
    runs, repetition_provenance = execute_measurements(
        repo_root=repo_root,
        binary=binary,
        documents=documents,
        davlan_model=davlan_model,
        kiji_model=kiji_model,
        threshold=args.threshold,
        diagnostics_dir=output_dir / "logs",
        warmup_count=args.warmups,
        measured_repetitions=args.measured_repetitions,
    )
    metadata, dataset_report = composite_dataset_report(dataiku_report, negative_report)
    candidate = score.assemble_scorecard(
        repo_root=repo_root,
        dataset_metadata=metadata,
        dataset_report=dataset_report,
        sampling_report=sampling_report,
        parameters={
            "profile": args.profile,
            "configs": list(score.DEFAULT_CONFIGS),
            "max_documents": max_documents,
            "sampling_seed": args.seed,
            "ner_threshold": args.threshold,
            "warmup_count": args.warmups,
            "measured_repetitions": args.measured_repetitions,
            "opf": False,
        },
        runs=runs,
    )
    candidate["runner_provenance"] = {
        "entry_point": "scripts/bench/run_no_opf_benchmark.py",
        "profile": args.profile,
        "model_bundles": model_provenance,
        "warmup_count": args.warmups,
        "measured_repetitions": args.measured_repetitions,
        "repetitions": repetition_provenance,
        "latency_source": "validated response timing.clean_ms",
        "cold_start_metric": "external process start to first validated response",
    }

    readiness_result = score.evaluate_release_readiness(candidate)
    readiness_correctness_failed = not readiness_result["passed"]
    if args.profile != "full":
        readiness_result = copy.deepcopy(readiness_result)
        readiness_result["passed"] = False
        readiness_result["failures"].append(
            {
                "gate": "full_profile_required",
                "reason": "a sampled quick run cannot establish release readiness",
            }
        )
    readiness = _status_document("release_readiness", readiness_result)
    if args.compare_baseline is None:
        regression: dict[str, object] = {
            "schema_version": 1,
            "kind": "regression",
            "status": "not_compared",
            "passed": None,
            "failures": [],
        }
        performance: dict[str, object] = {
            "schema_version": 1,
            "kind": "performance",
            "status": "not_compared",
            "passed": None,
            "gating": args.performance_gating,
            "disposition": "gating" if args.performance_gating else "informational",
            "tolerance_percent": args.performance_tolerance_percent,
            "comparisons": [],
            "failures": [],
        }
    else:
        baseline = load_scorecard(args.compare_baseline, "baseline")
        comparison = score.compare_scorecards(candidate, baseline)
        regression = _status_document("regression", comparison["regression"])
        performance_result = score.compare_performance(
            candidate,
            baseline,
            tolerance_percent=args.performance_tolerance_percent,
            gating=args.performance_gating,
        )
        performance = {
            "schema_version": 1,
            "kind": "performance",
            "status": "passed" if performance_result["passed"] else "failed",
            **performance_result,
        }

    write_json(output_dir / "scorecard-v3.json", candidate)
    write_json(output_dir / "diagnostics.json", diagnostics(candidate))
    write_json(output_dir / "regression-status.json", regression)
    write_json(output_dir / "release-readiness-status.json", readiness)
    write_json(output_dir / "performance-status.json", performance)
    (output_dir / "summary.md").write_text(
        markdown_summary(candidate, regression, readiness, performance),
        encoding="utf-8",
    )

    accept_baseline(args, candidate, regression, readiness)
    if regression["passed"] is False:
        return 3
    if args.profile == "full" and readiness["passed"] is False:
        return 4
    if args.profile == "quick" and readiness_correctness_failed:
        return 4
    if args.performance_gating and performance["passed"] is False:
        return 5
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    try:
        return run(parse_args(argv))
    except RunnerError as error:
        print(f"ERROR [{error.code}]: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
