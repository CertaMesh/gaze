#!/usr/bin/env python3
"""Choose the Nym recognizer operating point on a development split and freeze it.

Single-pass Stage A (solo todo 3738) moves Nym-small from an observer pass over the
tokenized output into the candidate pool, where it reads the normalized input instead.
What the model reads changes its operating point (PR #617's rejected mask moved it), so
op-B's thresholds are re-chosen here instead of assumed.

Data discipline. The EN/DE rows of the pinned Dataiku *test* split and the committed A4
negative corpus are the evaluation set; they never reach the model here and are read only
to drop a development text equal to an evaluation text
(docs/reference/benchmarks/README.md, "evaluation-only ... threshold selection"). This
script uses:

* **Development positives:** a seeded sample of the English and German rows of the pinned
  Dataiku *train* split, at the same repository revision, with any row whose text also
  occurs in the test split dropped (a train row may still share a generator template with
  the test split: this is same-generator development data, disclosed as such).
* **Development negatives:** the A4 generator (`xtask generate-negative-corpus`) run with
  another seed and written elsewhere: the same templates, different values; any text equal
  to an evaluation negative is dropped.

The selection rule is op-B's gate with Stage A's own negative bar: per label, the lowest
grid threshold (0.30 to 0.99, the op-B probe's range) whose action precision (gold-touching
spans / spans, scored labels of contract v2) is at least 0.70 and whose flags per
development-negative document stay below 0.05 (op-B) and at or below 1 per 1,024 (the Stage
A acceptance bar on the evaluation negatives; the stricter of the two binds), the other
labels held at op-B. A label no threshold qualifies is left out. The chosen point is then
decoded jointly on the same development data and must keep the whole allowlist within the
1-per-1,024 bar.

Why both bars: op-B's 0.05 per document admits about 51 flags on 1,024 negatives, while
Stage A accepts at most one. Under op-B's bar alone the first sweep chose DATE_OF_BIRTH at
0.75, which flagged 42 of the 978 development negatives (0.043 per document): certain to
fail acceptance, so the binding bar is applied on the development data, never on the
evaluation data.

    uv run --project scripts/bench python scripts/bench/nym_recognizer_dev_sweep.py \
        --sweep-binary target/release/examples/nym_dev_sweep \
        --negatives target/bench-data/nym-dev/negatives-seed-20260923.jsonl

needs GAZE_NYM_MODEL_DIR (the pinned bundle) and writes
crates/gaze-recognizers/nym-recognizer-operating-point.json.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from collections import defaultdict
from dataclasses import replace
from pathlib import Path
from typing import Iterable, Mapping, Sequence

import dataiku_en_de_gaze_bench as dataiku
import gaze_bench_score as score

TRAIN_FILE = "data/train-00000-of-00001.parquet"
TRAIN_URL = (
    f"https://huggingface.co/datasets/{dataiku.DATASET_REPO}/resolve/"
    f"{dataiku.DATASET_REVISION}/{TRAIN_FILE}"
)
TRAIN_SHA256 = "9b37af7eea2718af8cb6696fbe01a4bfd01dc6160716c272c2ce0835e71846de"
TRAIN_BYTES = 17_817_224
TRAIN_ROWS = 46_345
DEV_SEED = 20_260_923
DEV_PER_LANGUAGE = 2_000
NEGATIVE_GENERATOR_SEED = 20_260_923
EVALUATION_NEGATIVES = Path("crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl")
SCORED_LABELS = Path("docs/reference/benchmarks/scored-labels-v2.json")
OUTPUT = Path("crates/gaze-recognizers/nym-recognizer-operating-point.json")
MIN_PRECISION = 0.70
MAX_FLAGS_PER_NEGATIVE = 0.05
# Stage A acceptance: at most one false flag on the 1,024 evaluation negatives.
ACCEPTANCE_FLAGS_PER_NEGATIVE = 1 / 1_024
OP_B = {"BUILDING_NUMBER": 0.5, "DATE_OF_BIRTH": 0.9, "LICENSE_PLATE": 0.5, "USERNAME": 0.5}
# The gold label each Nym label is meant to find (informational: precision counts any
# scored gold, like op-B's gate).
OWN_LABEL = {
    "BUILDING_NUMBER": "BUILDINGNUM",
    "DATE_OF_BIRTH": "DATEOFBIRTH",
    "LICENSE_PLATE": "LICENSEPLATENUM",
    "USERNAME": "USERNAME",
}


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def load_train_rows(path: Path) -> list[dict[str, object]]:
    try:
        import pyarrow.parquet as parquet
    except ImportError as error:  # pragma: no cover - environment guard
        raise RuntimeError("run with `uv run --project scripts/bench`") from error
    size = path.stat().st_size
    digest = score.sha256_file(path)
    if size != TRAIN_BYTES or digest != TRAIN_SHA256:
        raise RuntimeError(f"train split integrity mismatch: size={size} sha256={digest}")
    rows = parquet.read_table(path).to_pylist()
    if len(rows) != TRAIN_ROWS:
        raise RuntimeError(f"expected {TRAIN_ROWS} train rows, found {len(rows)}")
    return rows


def development_positives(
    train_rows: Sequence[Mapping[str, object]], test_texts: frozenset[str]
) -> tuple[list[score.Document], dict[str, object]]:
    by_language: dict[str, list[tuple[bytes, int, Mapping[str, object]]]] = defaultdict(list)
    dropped_test_overlap = 0
    for index, row in enumerate(train_rows):
        language_name = row["language"]
        if language_name not in {"English", "German"}:
            continue
        if row["text"] in test_texts:
            dropped_test_overlap += 1
            continue
        language = "en" if language_name == "English" else "de"
        rank = hashlib.sha256(f"{DEV_SEED}:{index}".encode()).digest()
        by_language[language].append((rank, index, row))
    documents: list[score.Document] = []
    for language in ("de", "en"):
        chosen = sorted(by_language[language])[:DEV_PER_LANGUAGE]
        if len(chosen) != DEV_PER_LANGUAGE:
            raise RuntimeError(f"not enough {language} train rows")
        for _, index, row in sorted(chosen, key=lambda item: item[1]):
            text = row["text"]
            assert isinstance(text, str)
            offsets = score.char_to_byte_offsets(text)
            spans = []
            for entity in row["privacy_mask"]:
                start, end = entity["start"], entity["end"]
                if text[start:end] != entity["value"]:
                    raise RuntimeError(f"train row {index}: entity value/offset mismatch")
                spans.append(score.Span(offsets[start], offsets[end], entity["label"]))
            documents.append(
                score.Document(
                    uid=f"dev-{language}-{index:05d}",
                    text=text,
                    language=language,
                    region=dataiku.COUNTRY_REGIONS.get(str(row["country"]), ""),
                    source_dataset="dataiku-train-dev",
                    spans=tuple(spans),
                )
            )
    report = {
        "source": {
            "repository": dataiku.DATASET_REPO,
            "revision": dataiku.DATASET_REVISION,
            "file": TRAIN_FILE,
            "sha256": TRAIN_SHA256,
            "rows": TRAIN_ROWS,
        },
        "selection": (
            f"English and German rows whose text is not in the test split; per language the "
            f"{DEV_PER_LANGUAGE} rows with the smallest sha256('{DEV_SEED}:<row index>')"
        ),
        "rows_dropped_for_test_text_overlap": dropped_test_overlap,
        "documents": len(documents),
        "document_ids": score.document_ids_digest([d.uid for d in documents]),
        "caveat": (
            "same-generator development data: train and test rows may share templates, so "
            "the evaluation is not independent of the generator"
        ),
    }
    return documents, report


def exclude_out_of_contract(
    documents: Sequence[score.Document], contract_path: Path
) -> list[score.Document]:
    """Moves the gold of labels contract v2 puts out of contract (the credential labels) to
    `excluded_spans`. The train split carries a few labels the evaluation contract never had
    to rule on (single rows of `PERSONALAUSWEISNUMMER`, `MEDICARE`, ...); they are personal
    data and stay scored here, where the contract itself would fail closed on them."""
    contract = json.loads(contract_path.read_text(encoding="utf-8"))
    excluded = {entry["label"] for entry in contract["labels"] if not entry["scored"]}
    return [
        replace(
            document,
            spans=tuple(span for span in document.spans if span.label not in excluded),
            excluded_spans=tuple(span for span in document.spans if span.label in excluded),
        )
        for document in documents
    ]


def development_negatives(
    path: Path, repo_root: Path
) -> tuple[list[score.Document], dict[str, object]]:
    evaluation = {
        json.loads(line)["text"]
        for line in (repo_root / EVALUATION_NEGATIVES).read_text(encoding="utf-8").splitlines()
    }
    documents = []
    dropped = 0
    seeds = set()
    for line in path.read_text(encoding="utf-8").splitlines():
        row = json.loads(line)
        seeds.add(row["seed"])
        if row["text"] in evaluation:
            dropped += 1
            continue
        documents.append(
            score.Document(
                uid=f"devneg-{row['id']}",
                text=row["text"],
                language=row["language"],
                region="",
                source_dataset="gaze-a4-negative-generator-dev-seed",
                spans=(),
                negative_category=row["category"],
            )
        )
    if seeds != {NEGATIVE_GENERATOR_SEED}:
        raise RuntimeError(f"development negatives carry seeds {sorted(seeds)}")
    return documents, {
        "generator": "cargo run -p xtask -- generate-negative-corpus "
        f"--seed {NEGATIVE_GENERATOR_SEED} --out <path>",
        "sha256": score.sha256_file(path),
        "documents": len(documents),
        "dropped_equal_to_an_evaluation_negative": dropped,
        "caveat": "same generator and templates as the A4 evaluation corpus, other values",
    }


def run_sweep(
    binary: Path, documents: Sequence[score.Document], joint: Mapping[str, float] | None
) -> dict[str, dict[str, object]]:
    command = [str(binary)]
    if joint is not None:
        command += ["--joint", ",".join(f"{label}={value}" for label, value in joint.items())]
    with tempfile.TemporaryDirectory() as directory:
        requests = Path(directory) / "requests.jsonl"
        requests.write_text(
            "".join(json.dumps({"id": d.uid, "text": d.text}) + "\n" for d in documents),
            encoding="utf-8",
        )
        with requests.open("rb") as stdin:
            completed = subprocess.run(
                command, stdin=stdin, capture_output=True, check=False
            )
    if completed.returncode != 0:
        raise RuntimeError(
            f"sweep binary failed ({completed.returncode}): "
            f"{completed.stderr.decode(errors='replace')[-2000:]}"
        )
    results = {}
    for line in completed.stdout.decode("utf-8").splitlines():
        row = json.loads(line)
        results[row["id"]] = row
    if set(results) != {d.uid for d in documents}:
        raise RuntimeError("sweep binary did not answer every document")
    return results


def overlaps(span: tuple[int, int], intervals: Iterable[tuple[int, int]]) -> bool:
    return any(span[0] < end and start < span[1] for start, end in intervals)


class LabelStats:
    def __init__(self) -> None:
        self.actions = 0
        self.gold_touching = 0
        self.own_label_touching = 0
        self.ignored = 0
        self.own_gold_bytes_covered = 0
        self.negative_flags = 0

    def add_positive(self, document: score.Document, spans: Sequence[Sequence[int]], label: str):
        gold = score.merge_intervals((s.start, s.end) for s in document.spans)
        excluded = score.subtract_intervals(
            score.merge_intervals((s.start, s.end) for s in document.excluded_spans), gold
        )
        own = score.merge_intervals(
            (s.start, s.end) for s in document.spans if s.label == OWN_LABEL[label]
        )
        predicted = []
        for start, end in spans:
            span = (start, end)
            if excluded and score.interval_is_covered(span, excluded):
                self.ignored += 1
                continue
            predicted.append(span)
            self.actions += 1
            self.gold_touching += overlaps(span, gold)
            self.own_label_touching += overlaps(span, own)
        self.own_gold_bytes_covered += score.intersection_length(
            own, score.merge_intervals(predicted)
        )

    def add_negative(self, spans: Sequence[Sequence[int]]):
        self.negative_flags += len(spans)

    def result(self, negative_documents: int) -> dict[str, object]:
        return {
            "actions": self.actions,
            "gold_touching": self.gold_touching,
            "precision": round(score.safe_ratio(self.gold_touching, self.actions), 4),
            "own_label_touching": self.own_label_touching,
            "own_label_precision": round(
                score.safe_ratio(self.own_label_touching, self.actions), 4
            ),
            "own_gold_bytes_covered": self.own_gold_bytes_covered,
            "ignored_out_of_contract": self.ignored,
            "negative_flags": self.negative_flags,
            "flags_per_negative_document": round(
                score.safe_ratio(self.negative_flags, negative_documents), 4
            ),
        }


def sweep_table(
    positives: Sequence[score.Document],
    negatives: Sequence[score.Document],
    results: Mapping[str, Mapping[str, object]],
) -> dict[str, dict[str, dict[str, object]]]:
    table: dict[str, dict[str, dict[str, object]]] = {}
    for label in OP_B:
        thresholds = sorted(results[positives[0].uid]["spans"][label])
        table[label] = {}
        for threshold in thresholds:
            stats = LabelStats()
            for document in positives:
                stats.add_positive(document, results[document.uid]["spans"][label][threshold], label)
            for document in negatives:
                stats.add_negative(results[document.uid]["spans"][label][threshold])
            table[label][threshold] = stats.result(len(negatives))
    return table


def passes(row: Mapping[str, object], negative_documents: int) -> bool:
    flags_per_document = score.safe_ratio(row["negative_flags"], negative_documents)
    return (
        row["actions"] > 0
        and row["gold_touching"] / row["actions"] >= MIN_PRECISION
        and flags_per_document < MAX_FLAGS_PER_NEGATIVE
        and flags_per_document <= ACCEPTANCE_FLAGS_PER_NEGATIVE
    )


def choose(
    table: Mapping[str, Mapping[str, Mapping[str, object]]], negative_documents: int
) -> dict[str, float]:
    chosen = {}
    for label, rows in table.items():
        for threshold in sorted(rows, key=float):
            if passes(rows[threshold], negative_documents):
                chosen[label] = float(threshold)
                break
    return chosen


def joint_report(
    positives: Sequence[score.Document],
    negatives: Sequence[score.Document],
    results: Mapping[str, Mapping[str, object]],
) -> dict[str, dict[str, object]]:
    report = {}
    for label in OP_B:
        stats = LabelStats()
        for document in positives:
            spans = [(s, e) for s, e, found in results[document.uid]["joint"] if found == label]
            stats.add_positive(document, spans, label)
        for document in negatives:
            stats.add_negative(
                [(s, e) for s, e, found in results[document.uid]["joint"] if found == label]
            )
        report[label] = stats.result(len(negatives))
    return report


def parse_args(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--train", type=Path, default=Path("target/bench-data/dataiku-en-de/train.parquet"))
    parser.add_argument("--test", type=Path, default=Path("target/bench-data/dataiku-en-de/test.parquet"))
    parser.add_argument("--negatives", type=Path, required=True)
    parser.add_argument("--sweep-binary", type=Path, required=True)
    parser.add_argument("--evidence-dir", type=Path, default=Path("target/bench-data/nym-dev"))
    parser.add_argument("--output", type=Path, default=OUTPUT)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    repo_root = Path(__file__).resolve().parents[2]
    resolve = lambda path: path if path.is_absolute() else repo_root / path  # noqa: E731
    if not os.environ.get("GAZE_NYM_MODEL_DIR"):
        raise RuntimeError("GAZE_NYM_MODEL_DIR must point at the pinned Nym bundle")
    test_documents, _ = dataiku.load_documents(resolve(args.test))
    test_texts = frozenset(document.text for document in test_documents)
    positives, positive_report = development_positives(load_train_rows(resolve(args.train)), test_texts)
    positives = exclude_out_of_contract(positives, resolve(SCORED_LABELS))
    negatives, negative_report = development_negatives(resolve(args.negatives), repo_root)
    binary = resolve(args.sweep_binary)
    evidence_dir = resolve(args.evidence_dir)
    evidence_dir.mkdir(parents=True, exist_ok=True)

    documents = [*positives, *negatives]
    print(f"sweep: {len(positives)} positives, {len(negatives)} negatives", file=sys.stderr)
    results = run_sweep(binary, documents, None)
    table = sweep_table(positives, negatives, results)
    chosen = choose(table, len(negatives))
    print(f"sweep: chosen {chosen}", file=sys.stderr)
    joint_results = run_sweep(binary, documents, chosen)
    joint = joint_report(positives, negatives, joint_results)
    joint_flags = sum(row["negative_flags"] for row in joint.values())
    if score.safe_ratio(joint_flags, len(negatives)) > ACCEPTANCE_FLAGS_PER_NEGATIVE:
        raise RuntimeError(
            f"the chosen point flags {joint_flags} development negatives jointly; "
            "the allowlist exceeds the 1-per-1,024 bar"
        )
    (evidence_dir / "sweep-results.jsonl").write_text(
        "".join(json.dumps(results[d.uid]) + "\n" for d in documents), encoding="utf-8"
    )

    labels = sorted(chosen)
    frozen = {
        "schema": "gaze.nym-recognizer-operating-point/v1",
        "labels": labels,
        "thresholds": {label: chosen[label] for label in labels},
        "input": "normalized",
        "model": {
            "id": "nym-small-int8",
            "repository": "Wismut/nym-pii-multilingual-small",
            "commit": "4348999cd3c2e20c49615e9af7c6bbb45b64cd85",
        },
        "rule": (
            f"per label, the lowest grid threshold with action precision >= {MIN_PRECISION} "
            f"(gold-touching spans / spans, scored labels of contract v2), fewer than "
            f"{MAX_FLAGS_PER_NEGATIVE} flags per development-negative document (op-B) and at "
            "most 1 flag per 1,024 development-negative documents (the Stage A acceptance bar, "
            "the binding one), the other labels held at op-B; a label no threshold qualifies "
            "is left out; the joint point must stay within 1 per 1,024"
        ),
        "rule_note": (
            "op-B's bar alone chose DATE_OF_BIRTH 0.75, which flagged 42 of 978 development "
            "negatives; the acceptance bar is applied on development data only"
        ),
        "development_positives": positive_report,
        "development_negatives": negative_report,
        "sweep_binary_sha256": score.sha256_file(binary),
        "scored_labels": SCORED_LABELS.as_posix(),
        "sweep": table,
        "joint": joint,
        "op_b_reference": OP_B,
    }
    output = resolve(args.output)
    output.write_text(json.dumps(frozen, indent=2, sort_keys=False) + "\n", encoding="utf-8")
    print(f"wrote {output}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
