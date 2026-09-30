#!/usr/bin/env python3
"""Run Presidio Research's own tuned Presidio setup and write its raw findings.

Runs in the vendor environment (requirements-theirbench.lock), because the tuned
analyzer needs presidio-analyzer's transformers stack, not the comparison
environment's. The analyzer is presidio_research_repro.custom_analyzer, notebook 5
verbatim, called exactly as PresidioAnalyzerWrapper calls it. Output is one JSON
line per document (character offsets, entity, score; never text) plus a metadata
file. theirbench.py resolves overlaps with the comparison's resolver and scores the
spans with the comparison's metrics, so this row goes through our harness.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import subprocess
import sys
from pathlib import Path

import presidio_research_repro as repro

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import tagged_gaze  # noqa: E402

DECLARATION = Path(__file__).with_name("vendor-tuned.json")


def model_sha256(model: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(p for p in model.rglob("*") if p.is_file() and ".cache" not in p.parts):
        digest.update(str(path.relative_to(model)).encode() + b"\0")
        digest.update(hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--checkout", type=Path, required=True)
    parser.add_argument("--openmed-model", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="raw findings, one JSON line per document")
    parser.add_argument("--meta", type=Path, required=True)
    parser.add_argument("--limit", type=int, help="smoke only; never published")
    args = parser.parse_args()
    pinned = json.loads(DECLARATION.read_text(encoding="utf-8"))["presidio-research"]["model"]
    # The model directory is hashed and compared with the reviewed pin BEFORE any inference.
    receipt = {**pinned, "tree_sha256": model_sha256(args.openmed_model)}
    try:
        tagged_gaze.check_model_receipt(receipt, pinned, "--openmed-model")
    except ValueError as error:
        raise SystemExit(str(error)) from error
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=args.checkout, text=True).strip()
    if head != repro.COMMIT:
        raise SystemExit(f"presidio-research checkout is at {head}, expected {repro.COMMIT}")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=args.checkout, text=True).strip():
        raise SystemExit("the presidio-research checkout has local changes")
    from presidio_evaluator import InputSample
    from presidio_evaluator.models import PresidioAnalyzerWrapper

    dataset_path = args.checkout / repro.DATASET
    dataset = InputSample.read_dataset_json(dataset_path)[: args.limit]
    engine, threshold = repro.custom_analyzer(str(args.openmed_model))
    wrapper = PresidioAnalyzerWrapper(analyzer_engine=engine, score_threshold=threshold, language="en")
    kwargs: dict[str, object] = {}
    wrapper._PresidioAnalyzerWrapper__update_kwargs(kwargs)
    with args.output.open("w", encoding="utf-8") as sink:
        for index, sample in enumerate(dataset):
            found = engine.analyze(text=sample.full_text, **kwargs)
            rows = sorted((r.start, r.end, r.entity_type, round(float(r.score), 6)) for r in found)
            sink.write(json.dumps({"index": index, "results": rows}) + "\n")
    args.meta.write_text(json.dumps({
        "analyzer_kwargs": {key: kwargs[key] for key in ("language", "score_threshold")},
        "openmed": receipt, "model_verified_before_inference": True,
        "presidio_research_commit": head, "dataset_sha256": repro.sha256(dataset_path),
        "raw_sha256": hashlib.sha256(args.output.read_bytes()).hexdigest(),
        "documents": len(dataset), "smoke_limit": args.limit,
        "versions": {name: importlib.metadata.version(name) for name in (
            "presidio-analyzer", "presidio-anonymizer", "spacy", "en-core-web-lg", "transformers", "torch")},
        "python": sys.version.split()[0],
    }, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
