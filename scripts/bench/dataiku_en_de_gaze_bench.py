#!/usr/bin/env python3
"""Run the full Gaze scorecard on a pinned synthetic English/German holdout."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import urllib.request
from collections import Counter
from pathlib import Path

import gaze_bench_score as score


DATASET_REPO = "DataikuNLP/kiji-pii-training-data"
DATASET_REVISION = "0275550f0b1f1b8f2dc9356fd31ac1c788b8228b"
DATASET_FILE = "data/test-00000-of-00001.parquet"
DATASET_URL = (
    f"https://huggingface.co/datasets/{DATASET_REPO}/resolve/"
    f"{DATASET_REVISION}/{DATASET_FILE}"
)
DATASET_SHA256 = "916c63792345bf3c2e0888941b3d14526c43b7c7fe8af60e0d283fed71b1234d"
DATASET_BYTES = 2_013_107
DATASET_ROWS = 5_150

CONFIG_CHOICES = (
    "rule-floor-core",
    "rule-floor-extended",
    "pass2-ner",
    "full-stack-kiji-resolve",
    "full-stack-opf-resolve",
)

COUNTRY_REGIONS = {
    "Australia": "AU",
    "Austria": "AT",
    "Belgium": "BE",
    "Canada": "CA",
    "Germany": "DE",
    "Ireland": "IE",
    "Luxembourg": "LU",
    "New Zealand": "NZ",
    "Switzerland": "CH",
    "United Kingdom": "GB",
    "United States": "US",
}


def fetch_dataset(path: Path) -> None:
    if path.exists():
        verify_dataset(path)
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    request = urllib.request.Request(
        DATASET_URL,
        headers={"User-Agent": "gaze-dataiku-en-de-benchmark/1"},
    )
    digest = hashlib.sha256()
    size = 0
    with urllib.request.urlopen(request, timeout=120) as response:
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as output:
            temporary = Path(output.name)
            try:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    output.write(chunk)
                    digest.update(chunk)
                    size += len(chunk)
            except BaseException:
                temporary.unlink(missing_ok=True)
                raise
    if size != DATASET_BYTES or digest.hexdigest() != DATASET_SHA256:
        temporary.unlink(missing_ok=True)
        raise RuntimeError("downloaded Dataiku test split failed its integrity check")
    os.replace(temporary, path)


def verify_dataset(path: Path) -> None:
    size = path.stat().st_size
    digest = score.sha256_file(path)
    if size != DATASET_BYTES or digest != DATASET_SHA256:
        raise RuntimeError(
            f"dataset integrity mismatch for {path}: size={size}, sha256={digest}"
        )


def load_documents(path: Path) -> tuple[list[score.Document], dict[str, object]]:
    try:
        import pyarrow.parquet as parquet
    except ImportError as error:
        raise RuntimeError(
            "pyarrow is required to read the pinned parquet file; run with "
            "`uv run --with pyarrow scripts/bench/dataiku_en_de_gaze_bench.py`"
        ) from error

    verify_dataset(path)
    rows = parquet.read_table(path).to_pylist()
    if len(rows) != DATASET_ROWS:
        raise ValueError(f"expected {DATASET_ROWS} rows, found {len(rows)}")

    documents: list[score.Document] = []
    language_counts: Counter[str] = Counter()
    region_counts: Counter[str] = Counter()
    country_counts: Counter[str] = Counter()
    label_counts: Counter[str] = Counter()
    zero_span_documents = 0
    for row_index, row in enumerate(rows):
        language_name = row["language"]
        if language_name not in {"English", "German"}:
            continue
        language = "en" if language_name == "English" else "de"
        country = row["country"]
        region = COUNTRY_REGIONS.get(country, "")
        text = row["text"]
        offsets = score.char_to_byte_offsets(text)
        spans: list[score.Span] = []
        for entity in row["privacy_mask"]:
            start = entity["start"]
            end = entity["end"]
            value = entity["value"]
            label = entity["label"]
            if not isinstance(start, int) or not isinstance(end, int):
                raise ValueError(f"row {row_index}: non-integer entity offset")
            if start < 0 or end <= start or end > len(text):
                raise ValueError(f"row {row_index}: invalid entity bounds")
            if text[start:end] != value:
                raise ValueError(f"row {row_index}: entity value/offset mismatch")
            spans.append(score.Span(offsets[start], offsets[end], label))
            label_counts[label] += 1
        zero_span_documents += not spans
        language_counts[language] += 1
        country_counts[country] += 1
        region_counts[f"{language}-{region}" if region else language] += 1
        documents.append(
            score.Document(
                uid=f"dataiku-test-{row_index}",
                text=text,
                language=language,
                region=region,
                source_dataset=DATASET_REPO,
                spans=tuple(spans),
            )
        )
    if not documents:
        raise ValueError("Dataiku English/German selection is empty")

    report: dict[str, object] = {
        "integrity": {
            "rows": len(rows),
            "sha256": DATASET_SHA256,
            "bytes": DATASET_BYTES,
            "selected_entities": sum(label_counts.values()),
            "invalid_bounds": 0,
            "value_offset_mismatches": 0,
        },
        "selection": {
            "documents": len(documents),
            "entities": sum(label_counts.values()),
            "languages": dict(sorted(language_counts.items())),
            "regions": dict(sorted(region_counts.items())),
            "countries": dict(sorted(country_counts.items())),
            "labels": dict(sorted(label_counts.items())),
            "negative_only_documents": zero_span_documents,
        },
    }
    return documents, report


def build_binary(repo_root: Path, configs: tuple[str, ...]) -> Path:
    command = [
        "cargo",
        "build",
        "-q",
        "-p",
        "gaze-recognizers",
        "--example",
        "clean_for_bench",
    ]
    features = []
    if any("kiji" in config for config in configs):
        features.append("safety-net-kiji")
    if any("opf" in config for config in configs):
        features.append("safety-net-openai")
    if features:
        command.extend(["--features", ",".join(features)])
    subprocess.run(command, cwd=repo_root, check=True)
    binary = repo_root / "target/debug/examples/clean_for_bench"
    if not binary.is_file():
        raise FileNotFoundError(f"benchmark runner is missing: {binary}")
    return binary


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--dataset",
        type=Path,
        default=Path("target/bench-data/dataiku-en-de/test.parquet"),
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("target/bench-data/dataiku-en-de/gaze-scorecard.json"),
    )
    parser.add_argument(
        "--model-dir",
        type=Path,
        default=Path(
            os.environ.get(
                "GAZE_NER_MODEL_DIR",
                "~/.local/share/gaze/models/davlan-mbert-ner-hrl",
            )
        ).expanduser(),
    )
    parser.add_argument(
        "--kiji-model-dir",
        type=Path,
        default=Path(
            os.environ.get(
                "GAZE_KIJI_DISTILBERT_MODEL_DIR",
                "~/.local/share/gaze/models/kiji-distilbert",
            )
        ).expanduser(),
    )
    parser.add_argument("--threshold", type=float, default=0.3)
    parser.add_argument("--max-documents", type=int)
    parser.add_argument(
        "--sampling-seed", type=int, default=score.DEFAULT_SAMPLE_SEED
    )
    parser.add_argument(
        "--opf-command",
        type=Path,
        default=(
            Path(os.environ["GAZE_OPENAI_FILTER_OPF"])
            if os.environ.get("GAZE_OPENAI_FILTER_OPF")
            else None
        ),
    )
    parser.add_argument(
        "--opf-checkpoint",
        type=Path,
        default=(
            Path(os.environ["OPF_CHECKPOINT"])
            if os.environ.get("OPF_CHECKPOINT")
            else None
        ),
    )
    parser.add_argument(
        "--opf-daemon-socket",
        type=Path,
        default=(
            Path(os.environ["GAZE_OPF_DAEMON_SOCKET"])
            if os.environ.get("GAZE_OPF_DAEMON_SOCKET")
            else None
        ),
    )
    parser.add_argument(
        "--config", action="append", choices=CONFIG_CHOICES, help="repeatable"
    )
    parser.add_argument("--no-download", action="store_true")
    parser.add_argument("--skip-build", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = Path(__file__).resolve().parents[2]
    dataset_path = args.dataset if args.dataset.is_absolute() else repo_root / args.dataset
    output_path = args.output if args.output.is_absolute() else repo_root / args.output
    if args.no_download:
        verify_dataset(dataset_path)
    else:
        fetch_dataset(dataset_path)
    available_documents, dataset_report = load_documents(dataset_path)
    sampling_seed = getattr(args, "sampling_seed", score.DEFAULT_SAMPLE_SEED)
    documents, sampling_report = score.stratified_sample(
        available_documents, args.max_documents, sampling_seed
    )
    configs = tuple(args.config) if args.config else score.DEFAULT_CONFIGS
    if not args.model_dir.is_dir():
        raise FileNotFoundError(f"NER model directory does not exist: {args.model_dir}")
    if any("kiji" in config for config in configs) and not args.kiji_model_dir.is_dir():
        raise FileNotFoundError(
            f"Kiji model directory does not exist: {args.kiji_model_dir}"
        )
    if any("opf" in config for config in configs):
        if args.opf_command is None or not args.opf_command.is_file():
            raise FileNotFoundError("OPF command does not exist; pass --opf-command")
        if args.opf_checkpoint is None or not args.opf_checkpoint.is_dir():
            raise FileNotFoundError(
                "OPF checkpoint does not exist; pass --opf-checkpoint"
            )
    binary = (
        repo_root / "target/debug/examples/clean_for_bench"
        if args.skip_build
        else build_binary(repo_root, configs)
    )

    runs = []
    for config in configs:
        print(f"running {config} on {len(documents)} documents", file=sys.stderr)
        runs.append(
            score.run_config(
                repo_root,
                binary,
                config,
                documents,
                args.model_dir,
                args.kiji_model_dir,
                args.opf_command,
                args.opf_checkpoint,
                args.opf_daemon_socket,
                args.threshold,
                output_path.parent,
            )
        )

    result = score.assemble_scorecard(
        repo_root=repo_root,
        dataset_metadata={
            "repository": DATASET_REPO,
            "revision": DATASET_REVISION,
            "file": DATASET_FILE,
            "url": DATASET_URL,
            "license": "Apache-2.0",
            "synthetic_only": True,
            "reserved_from_training": True,
            "selection_policy": "English and German rows from the upstream test split",
        },
        dataset_report=dataset_report,
        sampling_report=sampling_report,
        parameters={
            "configs": list(configs),
            "ner_model_dir": str(args.model_dir),
            "kiji_model_dir": str(args.kiji_model_dir),
            "opf_command": str(args.opf_command) if args.opf_command else None,
            "opf_checkpoint": str(args.opf_checkpoint) if args.opf_checkpoint else None,
            "opf_daemon_socket": (
                str(args.opf_daemon_socket) if args.opf_daemon_socket else None
            ),
            "max_documents": args.max_documents,
            "sampling_seed": sampling_seed,
            "ner_threshold": args.threshold,
        },
        runs=runs,
    )
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {output_path}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
