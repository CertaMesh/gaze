"""Perplexity PII-Tracer on Gaze's own comparison corpus.

The comparison's `compare.py` is hash-pinned by the third-party benchmark runner, so
this adapter lives beside it and reuses its scoring path (`compare.measure`) unchanged.
The declared configuration was fixed from the model card before any result was seen:
CPU, the checkpoint's stored bf16, `predict()` decoding (constrained BIOES Viterbi,
no public threshold exists to tune), and non-overlapping token windows for documents
longer than the 4096-token window. There is no threshold sweep for that reason.
Only aggregate JSON is written; per-document spans stay outside the repository.
"""

from __future__ import annotations

import argparse
import json
import platform
import select
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Sequence

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import compare  # noqa: E402
import gaze_bench_score as score  # noqa: E402
import agentic_layers as agentic  # noqa: E402
import run_no_opf_benchmark as runner  # noqa: E402
from cpu_contention import ForeignCpuSampler  # noqa: E402

TOOL = "pii-tracer"
MODEL_REPO = "perplexity-ai/PII-Tracer"
REVISION = "d25c16f2e57e321f6d2527715c01df9112f956f5"
LABEL_MAP = HERE / "pii-tracer-label-map.json"
WORKER = HERE / "pii_tracer_worker.py"
REQUIREMENTS = HERE / "requirements-pii-tracer.lock"
READY = "PII_TRACER_READY "
# The worker reports the checkpoint's own label; OPF's `secret` is scored the same way.
NATIVE_TO_SCORED = {"secret": "custom:secret"}


def load_label_map() -> dict[str, tuple[str, ...]]:
    raw = json.loads(LABEL_MAP.read_text(encoding="utf-8"))
    raw.pop("_comment")
    return {label: tuple(canonical) for label, canonical in raw.items()}


class PiiTracer:
    languages = None

    def __init__(self, python: Path, model_dir: Path) -> None:
        self.process = subprocess.Popen(
            [str(python), str(WORKER), "--model-dir", str(model_dir)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
            encoding="utf-8",
        )
        assert self.process.stdout is not None and self.process.stdin is not None
        if not select.select([self.process.stdout], [], [], 600)[0]:
            self.close()
            raise RuntimeError("PII-Tracer worker startup timed out")
        ready = self.process.stdout.readline()
        if not ready.startswith(READY):
            self.close()
            raise RuntimeError("PII-Tracer worker failed to start (pinned files or runtime rejected)")
        self.runtime = json.loads(ready.removeprefix(READY))
        self.counter = 0

    def predict(self, document: score.Document) -> list[score.Span]:
        self.counter += 1
        self.process.stdin.write(json.dumps({"id": self.counter, "text": document.text}, ensure_ascii=False) + "\n")
        self.process.stdin.flush()
        reply = json.loads(self.process.stdout.readline() or "null")
        if reply is None or reply.get("id") != self.counter or "error" in reply:
            raise RuntimeError(f"PII-Tracer failed to process {document.uid}: {(reply or {}).get('error')}")
        return compare.byte_spans(
            document.text,
            [(start, end, NATIVE_TO_SCORED.get(label, label)) for start, end, label in reply["spans"]],
        )

    def close(self) -> None:
        self.process.kill()
        self.process.wait(timeout=10)


def as_of_committed_generator(committed: dict, layers: dict[str, list[score.Document]],
                              corpus: dict[str, object]) -> tuple[dict[str, list[score.Document]], dict[str, object]]:
    """The agentic layers exactly as the committed comparison measured them.

    The competitor rows in comparison.json were measured on an older agentic generator. The
    current generator rebuilds that corpus as a filter of its own output (records_as_of), and the
    older scored-label contract stays committed, so PII-Tracer is scored on the same documents
    under the same contract. The rebuilt layers must reproduce every committed layer digest, the
    committed agentic corpus hash and the committed contract hash, or this raises: a different
    corpus would make the rows incomparable. The shared loader is then pointed at that contract
    for this process only, because compare.measure() loads it without a path.
    """
    version = committed["corpus"]["agentic"]["generator_version"]
    if version == agentic.GENERATOR_VERSION:
        return layers, corpus
    path = compare.REPO / agentic.HISTORICAL_CONTRACTS[version]
    contract = score.load_scored_label_contract(path, display_path=agentic.HISTORICAL_CONTRACTS[version].as_posix())
    if contract.sha256 != committed["contracts"]["agentic"]:
        raise ValueError("the historical agentic contract differs from the committed comparison's")
    records = agentic.records_as_of(version, agentic.generate(agentic.PUBLISHED_PARTITION))
    documents = agentic.apply_contract([record.to_document() for record in records], contract)
    rebuilt = {"A": agentic.LAYER_IDENTIFIERS, "D": agentic.LAYER_LOOKALIKES, "R": agentic.LAYER_REPEATS}
    restricted = dict(layers)
    for layer, name in rebuilt.items():
        restricted[layer] = [d for d in documents if d.cell and d.cell.startswith(name + "|")]
    for layer, documents_in_layer in restricted.items():
        expected = committed["corpus"]["layers"][layer]
        actual = {"documents": len(documents_in_layer),
                  "ids_sha256": score.document_ids_digest([d.uid for d in documents_in_layer])}
        if actual != expected:
            raise ValueError(f"layer {layer} as of generator v{version} differs from the committed comparison")
    if agentic.corpus_identity(compare.REPO, version)[0] != committed["corpus"]["agentic"]["corpus_sha256"]:
        raise ValueError(f"generator v{version} no longer rebuilds the committed agentic corpus")
    agentic.load_contract = lambda repo_root, contract_path=None: contract
    return restricted, committed["corpus"]


def preflight(backend: PiiTracer, layers: dict[str, list[score.Document]], per_language: int,
              mapping: dict[str, tuple[str, ...]]) -> dict[str, object]:
    """STEER 4 label preflight: every language of every layer, unmapped labels fail; never published."""
    seen: dict[str, dict[str, int]] = {}
    for layer, documents in layers.items():
        taken: dict[str, int] = {}
        for document in documents:
            if taken.get(document.language, 0) >= per_language:
                continue
            taken[document.language] = taken.get(document.language, 0) + 1
            predictions = backend.predict(document)
            compare.validate_labels(predictions, mapping)
            counts = seen.setdefault(f"{layer}/{document.language}", {})
            for span in predictions:
                counts[span.label] = counts.get(span.label, 0) + 1
    return seen


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--pii-tracer-python", type=Path, required=True)
    parser.add_argument("--pii-tracer-model", type=Path, required=True)
    parser.add_argument("--pack-dir", type=Path, default=compare.REPO / "docs/reference/benchmarks/variant-packs")
    parser.add_argument("--predictions-dir", type=Path)
    parser.add_argument("--preflight", type=int, metavar="N", help="first N documents per layer and language; not published")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.predictions_dir is not None and args.predictions_dir.resolve().is_relative_to(compare.REPO):
        raise ValueError("per-document predictions must stay outside the repository")
    committed = json.loads((compare.REPO / "docs/reference/benchmarks/comparison.json").read_text(encoding="utf-8"))
    layers, corpus = compare.load_corpus(args.dataset, args.pack_dir)
    layers, corpus = as_of_committed_generator(committed, layers, corpus)
    compare.preflight_contracts(layers)
    if committed["corpus"] != corpus:
        raise ValueError("corpus differs from the committed comparison; measure that corpus")
    base = compare.load_mapping()
    mapping = load_label_map()
    common = compare.common_claimed_labels(base)
    claimed = {label for labels in mapping.values() for label in labels}
    if not common <= claimed:
        raise ValueError(f"PII-Tracer must claim every common-intersection label; missing {sorted(common - claimed)}")
    with tempfile.TemporaryDirectory(prefix="gaze-pii-tracer-"):
        backend = PiiTracer(args.pii_tracer_python, args.pii_tracer_model)
        try:
            if args.preflight:
                found = preflight(backend, layers, args.preflight, mapping)
                print(json.dumps(found, indent=1, sort_keys=True))
                print("PII_TRACER_PREFLIGHT_OK", file=sys.stderr)
                return 0
            backend.predict(score.Document("warmup", "alice@example.invalid", "en", "", "synthetic", ()))
            with ForeignCpuSampler() as sampler:
                measured = compare.measure(TOOL, backend.predict, layers, mapping, None, common,
                                           args.predictions_dir)
            measured["host_load_1m_before_after"] = sampler.result()["load1_before_after"]
            measured["provenance"] = {
                "model_repo": MODEL_REPO, "revision": REVISION,
                "runtime": {**backend.runtime, "worker_sha256": compare.digest_file(WORKER),
                            "requirements_sha256": compare.digest_file(REQUIREMENTS)},
                "config": "declared before measurement: cpu, stored bf16, predict() Viterbi decode, "
                          "non-overlapping 4080-token windows, no threshold (none is published)",
                "remote_code": "modeling_pii_masking.py, sha256 pinned in the worker; imports torch and "
                               "transformers only",
            }
        finally:
            backend.close()
    report = {
        "schema_version": 2, "tool": TOOL,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=compare.REPO, text=True).strip(),
        "harness_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=compare.REPO, text=True).strip()),
        "compare_sha256": compare.digest_file(HERE / "compare.py"),
        "comparison_metrics_sha256": compare.digest_file(HERE / "comparison_metrics.py"),
        "adapter_sha256": compare.digest_file(Path(__file__)),
        "mapping_sha256": compare.digest_file(LABEL_MAP),
        "base_mapping_sha256": compare.digest_file(compare.MAP_PATH),
        "contracts": {
            **{v: runner.load_scored_label_contract(compare.REPO, p).sha256 for v, p in compare.CONTRACTS.items()},
            "agentic": agentic.load_contract(compare.REPO).sha256,
        },
        "corpus": corpus, "hardware": platform.platform(), "device": "cpu",
        "common_intersection_labels": sorted(common),
        "contention": sampler.result(),
        "tools": {TOOL: measured},
    }
    compare.write_report(args.output, report)
    print("PII_TRACER_DONE", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
