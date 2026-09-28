#!/usr/bin/env python3
"""Build PIIBench-commercial with PIIBench's own consolidation and split code.

PIIBench (pritesh-2711/pii-bench, arXiv 2604.15776) merges ten Hugging Face
datasets. Five carry non-commercial or custom-academic licences
(ai4privacy 400k/300k, MultiNERD, CoNLL-2003, Isotonic 200k) and WikiANN's
card says "unknown"; none of those six is downloaded or run here. The four
remaining sources are consolidated by PIIBench's own generator functions and
split by its own `prepare()` (seed 42, finer-139 cap, rare-type suppression,
Nemotron share, stratified 80/10/10, exact-size 5,000-record test subset).

Differences from the published benchmark, all disclosed: six sources are
absent, so the rare-type threshold and the stratified subset run over four
sources; Hugging Face revisions are pinned here (PIIBench pins none). The
published full-mix Presidio F1 (0.1385, 1,398 records) is therefore quoted,
never claimed as reproduced.

Writes prepared splits to --output-dir (outside the repo). Never commit them.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

PIIBENCH = {"repository": "pritesh-2711/pii-bench",
            "commit": "f96e13645f217c507d0cafa12340b99a26c2d32c",
            "licence": "Apache-2.0 (code)"}
# (source id in PIIBench, HF dataset, revision, licence)
SOURCES = (
    ("gretel_finance", "gretelai/synthetic_pii_finance_multilingual",
     "7b844d16738527a04264f50214cb426a4cea0897", "Apache-2.0"),
    ("nvidia_nemotron", "nvidia/Nemotron-PII",
     "b70ffaf5ff39e079776134c5bf4381f00a9fd1ed", "CC-BY-4.0"),
    ("few_nerd", "DFKI-SLT/few-nerd",
     "205f3e9c9f3577ea2561d43f2f62dc249ab92d5b", "CC-BY-SA-4.0"),
    ("finer_139", "nlpaueb/finer-139",
     "a8a2191f2ed8b5be966ac293721695cfea1410b5", "CC-BY-SA-4.0"),
)
EXCLUDED = {
    "ai4privacy/pii-masking-400k": "custom licence; commercial use requires a licence from ai4privacy",
    "ai4privacy/pii-masking-300k": "custom licence; commercial use requires a licence from ai4privacy",
    "Babelscape/multinerd": "CC-BY-NC-SA-4.0",
    "conll2003": "Reuters corpus, non-commercial research agreement",
    "Isotonic/pii-masking-200k": "CC-BY-NC-4.0 on its dataset card (the paper lists Apache-2.0)",
    "wikiann": "dataset card licence is unknown",
}
PUBLISHED = {"system": "Presidio AnalyzerEngine() default, en_core_web_lg",
             "precision": 0.1522, "recall": 0.1271, "f1": 0.1385,
             "records": 1398, "sources": 10, "metric": "seqeval span-level, exact span + type",
             "source": "arXiv 2604.15776 Table 5"}


def consolidate(checkout: Path, output: Path) -> Path:
    sys.path.insert(0, str(checkout))
    import stream_consolidate as sc
    from datasets import load_dataset

    path = output / "consolidated.jsonl"
    with path.open("w", encoding="utf-8") as handle:
        def write(records) -> None:
            for record in records:
                handle.write(json.dumps(record, ensure_ascii=False) + "\n")

        _, gretel, gretel_rev, _ = SOURCES[0]
        for split in ("train", "test"):
            rows = load_dataset(gretel, split=split, revision=gretel_rev)
            write(sc.yield_span_records(rows, "generated_text", "pii_spans", "gretel_finance", include_text=True))
        _, nemotron, nemotron_rev, _ = SOURCES[1]
        write(sc.yield_nvidia_records(load_dataset(nemotron, split="train", revision=nemotron_rev), include_text=True))
        _, fewnerd, fewnerd_rev, _ = SOURCES[2]
        fewnerd_labels = ["O", "art", "building", "event", "location",
                          "organization", "other", "person", "product"]
        for split in ("train", "validation", "test"):
            rows = load_dataset(fewnerd, "supervised", split=split, revision=fewnerd_rev)
            write(sc.yield_fewnerd_records(rows, fewnerd_labels, include_text=True))
        _, finer, finer_rev, _ = SOURCES[3]
        for split in ("train", "validation", "test"):
            rows = load_dataset(finer, split=split, revision=finer_rev)
            write(sc.yield_finer_records(rows, include_text=True))
    return path


def prepare(checkout: Path, consolidated: Path, output: Path) -> None:
    sys.path.insert(0, str(checkout / "src"))
    import data_preparation as dp

    dp.CONSOLIDATED_FILE = consolidated
    dp.OUTPUT_DIR = output / "data"
    dp.prepare(include_text=True)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--checkout", type=Path, required=True, help=f"{PIIBENCH['repository']} checkout")
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=args.checkout, text=True).strip()
    if head != PIIBENCH["commit"]:
        raise SystemExit(f"pii-bench checkout is at {head}, expected {PIIBENCH['commit']}")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    consolidated = consolidate(args.checkout, args.output_dir)
    prepare(args.checkout, consolidated, args.output_dir)
    data = args.output_dir / "data"
    manifest = {
        "piibench": PIIBENCH,
        "sources": [{"id": s, "dataset": d, "revision": r, "licence": lic} for s, d, r, lic in SOURCES],
        "excluded": EXCLUDED,
        "published_full_mix": PUBLISHED,
        "files": {name: sha256(data / name) for name in
                  ("test_5k.jsonl", "val.jsonl", "test.jsonl", "label_mapping.json")},
    }
    (args.output_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest["files"], indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
