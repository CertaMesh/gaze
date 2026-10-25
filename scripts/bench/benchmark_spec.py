"""Portable, content-addressed measuring specification, independent of the subject."""

from __future__ import annotations

import copy
import importlib.metadata
import json
import os
import re
import sys
import unicodedata
from pathlib import Path
from typing import Mapping

from capture_identity import digest, file_digest

SCHEMA_VERSION = 1
PREFIX = "gaze-benchmark-v1:"
PROTOCOL = Path("scripts/bench/benchmark-protocol.json")
HEX64 = re.compile(r"[0-9a-f]{64}")


class SpecError(ValueError):
    """Closed, value-free refusal at a specification boundary."""

    def __init__(self, code: str = "invalid_benchmark_spec") -> None:
        self.code = code
        super().__init__(code)


QUALIFIED_RUNTIME = {"implementation": "cpython", "python": "3.13", "unicode": "15.1.0", "pyarrow": "23.0.1"}
CANONICAL_CAPTURE = {"version": 1, "mode": "canonical-once"}
CURRENT_VALIDATION = {"version": 1, "manifest_replacing_actions": ["redact", "tokenize"],
                      "split_composite_source_ids": False}
LEGACY_VALIDATION = {**CURRENT_VALIDATION, "manifest_replacing_actions": ["tokenize"]}
# Only releases whose protocol predates manifest redactions may use this adapter.
RELEASE_PROTOCOLS = {
    "v0.14.0": LEGACY_VALIDATION,
    "v0.15.0": CURRENT_VALIDATION,
    "v0.15.1": CURRENT_VALIDATION,
    "v0.16.0": CURRENT_VALIDATION,
    "v0.17.0": CURRENT_VALIDATION,
    "v0.18.0": CURRENT_VALIDATION,
}


def response_validation(args) -> dict:
    return {"version": 1,
            "manifest_replacing_actions": sorted(getattr(args, "manifest_actions", "tokenize,redact").split(",")),
            "split_composite_source_ids": getattr(args, "split_composite_source_ids", False)}


def runtime_identity() -> dict:
    """Portable stdlib/Unicode semantics and the canonical parquet reader."""
    try:
        arrow = importlib.metadata.version("pyarrow")
    except importlib.metadata.PackageNotFoundError:
        arrow = None
    return {"implementation": sys.implementation.name,
            "python": ".".join(map(str, sys.version_info[:2])),
            "unicode": unicodedata.unidata_version, "pyarrow": arrow}


def require_runtime(spec: Mapping | None = None) -> dict:
    actual = runtime_identity()
    expected = QUALIFIED_RUNTIME if spec is None else spec["inputs"].get("measuring_runtime")
    if actual != QUALIFIED_RUNTIME or actual != expected:
        raise SpecError("benchmark_runtime_mismatch")
    return actual


def capture_protocol(args) -> dict:
    # Mode comes from execution flags, never from a resulting scorecard.
    cross_check = (getattr(args, "cross_check", False)
                   or getattr(args, "scored_labels", None) is not None
                   or getattr(args, "agentic_scored_labels", None) is not None)
    return {"version": 1, "mode": "cross-check" if cross_check else "canonical-once"}


def manifest(inputs: Mapping) -> dict:
    """No revision, timestamp, host, binary, policy or model belongs in this hash."""
    value = {"schema_version": SCHEMA_VERSION, "inputs": copy.deepcopy(dict(inputs))}
    return {"id": PREFIX + digest(value), **value}


def validate(value: object) -> dict:
    try:
        if (not isinstance(value, dict) or set(value) != {"id", "schema_version", "inputs"}
                or type(value["schema_version"]) is not int
                or value["schema_version"] != SCHEMA_VERSION):
            raise SpecError()
        inputs = value["inputs"]
        fields = {"corpus_sha256", "generator", "seed", "contracts", "harness",
                  "record_schema", "protocol", "population"}
        if (not isinstance(inputs, dict)
                or not fields <= set(inputs)
                or set(inputs) - fields - {"validation_vocabulary", "measuring_runtime"}
                or not HEX64.fullmatch(inputs["corpus_sha256"])
                or type(inputs["seed"]) is not int
                or not inputs["contracts"] or not inputs["harness"]
                or not isinstance(inputs["record_schema"], list)
                or not isinstance(inputs["protocol"], dict)
                or not isinstance(inputs["population"], dict)):
            raise SpecError()
        if manifest(inputs) != value:
            raise SpecError()
    except (KeyError, TypeError, ValueError) as error:
        raise SpecError() from error
    return value


def identity(card: Mapping, *, required: bool = False) -> str | None:
    value = card.get("benchmark_spec")
    if value is None:
        if required:
            raise SpecError("benchmark_spec_unavailable")
        return None
    return validate(value)["id"]


def require_same(left: Mapping, right: Mapping, *, required: bool = True) -> str | None:
    a, b = identity(left, required=required), identity(right, required=required)
    if a != b:
        raise SpecError("benchmark_spec_mismatch")
    return a


def population(documents, layer: str = "C") -> dict:
    import scorecard_record as records

    ids = sorted(document.uid for document in documents)
    if len(ids) != len(set(ids)):
        raise SpecError("benchmark_population_invalid")
    return {"documents": len(ids), "document_ids_sha256": digest(ids),
            "descriptors_sha256": descriptor_digest([
                {**records._document_row(document), "layer": layer} for document in documents])}


def descriptor_digest(rows) -> str:
    """Hash only canonical, value-free document descriptors; exclude subject validators."""
    keys = ("id", "utf8_bytes", "language", "region", "source_dataset",
            "negative_category", "cell", "gold", "layer")
    descriptors = [{**{key: row[key] for key in keys}, "gold": sorted(row["gold"])}
                   for row in rows]
    return digest(sorted(descriptors, key=lambda row: row["id"]))


def measuring_inputs(root: Path) -> dict:
    """Resolve file membership and the effective scorer vocabulary on every admission."""
    import gaze_bench_score as score

    bench = root / "scripts/bench"
    sources = []
    for directory, folders, names in os.walk(bench):
        folders[:] = sorted(name for name in folders
                            if not name.startswith((".", "__")) and name != "fixtures")
        sources.extend(Path(directory) / name for name in sorted(names)
                       if name.endswith(".py") and not name.startswith("test_"))
    # These are measuring semantics; subject bytes have separate provenance.
    sources += sorted(bench.glob("*.json"))
    sources += sorted((bench / "mechanisms").glob("*.toml"))
    sources += [bench / "pyproject.toml", bench / "uv.lock"]
    return {
        "harness": {path.relative_to(root).as_posix(): file_digest(path) for path in sources},
        "validation_vocabulary": sorted(score.load_committed_source_id_vocabulary(root)),
        "measuring_runtime": require_runtime(),
    }


def full_inputs(root: Path, dataset: Path | None = None) -> dict:
    """Load the complete canonical population, without executing a detector."""
    import agentic_layers as agentic
    import dataiku_en_de_gaze_bench as dataiku
    import run_no_opf_benchmark as runner

    path = dataset or root / runner.parse_args(['full']).dataset
    dataiku.verify_dataset(path)
    positives, positive_report = dataiku.load_documents(path)
    negatives, negative_report = runner.load_negative_documents(root / runner.NEGATIVE_CORPUS)
    _, report = runner.composite_dataset_report(positive_report, negative_report)
    return dict(root=root, corpus_sha256=report['integrity']['sha256'],
                available=positives + negatives, prepared=agentic.prepare(root))


def require_current(value: dict, root: Path, *, dataset: Path | None = None) -> None:
    """Bind a supplied specification to the executing source and canonical inputs."""
    try:
        validate(value)
        executing = Path(__file__).resolve().parents[2]
        measured = measuring_inputs(root)
        if measured != measuring_inputs(executing):
            raise SpecError('harness_spec_mismatch')
        execution = value['inputs']['protocol']['execution']
        expected = resolve(**full_inputs(root, dataset), seed=value['inputs']['seed'],
                           profile='full', warmups=execution['warmups'],
                           repetitions=execution['repetitions'])
        if expected != value:
            raise SpecError('harness_spec_mismatch')
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, ImportError) as error:
        raise SpecError('harness_spec_mismatch') from error


def resolve(root: Path, *, corpus_sha256: str, seed: int, available, prepared,
            profile: str = "full", warmups: int = 1, repetitions: int = 1) -> dict:
    """Bind measuring source bytes, not Git HEAD (which also contains the subject).

    All non-test benchmark Python and its data/config inputs form the measuring
    closure. Output ledgers, retained scorecards and documentation are excluded.
    Rust detector/validator bytes stay in capture/subject provenance.
    """
    import scorecard_record as records

    measuring = measuring_inputs(root)
    contracts = {"v1": "all-corpus-labels-v1"}
    for path in sorted((root / "docs/reference/benchmarks").glob("scored-labels-v*.json")):
        contracts[path.stem] = file_digest(path)
    if prepared is not None:
        contracts["generated"] = prepared.contract.sha256
    protocol = json.loads((root / PROTOCOL).read_text(encoding="utf-8"))
    protocol["execution"] = {"profile": profile, "warmups": warmups, "repetitions": repetitions}
    populations = {"C": population(available)}
    if prepared is not None:
        populations.update({name: population(docs, name) for name, docs in (
            ("A", prepared.identifiers), ("D", prepared.lookalikes), ("R", prepared.repeats))})
    return validate(manifest({
        "corpus_sha256": corpus_sha256,
        "generator": copy.deepcopy(prepared.manifest) if prepared is not None else None,
        "seed": seed,
        "contracts": contracts,
        **measuring,
        "record_schema": sorted(records.READABLE_SCHEMA_VERSIONS),
        "protocol": protocol,
        "population": populations,
    }))


def main(argv=None) -> int:
    import argparse
    import sys
    import dataiku_en_de_gaze_bench as dataiku
    import agentic_layers
    import run_no_opf_benchmark as runner

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dataset', type=Path, required=True)
    parser.add_argument('--seed', type=int, default=20260710)
    parser.add_argument('--warmups', type=int, default=1)
    parser.add_argument('--repetitions', type=int, default=1)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        root = Path(__file__).resolve().parents[2]
        dataiku.verify_dataset(args.dataset)
        positives, positive_report = dataiku.load_documents(args.dataset)
        negatives, negative_report = runner.load_negative_documents(root / runner.NEGATIVE_CORPUS)
        _, report = runner.composite_dataset_report(positive_report, negative_report)
        value = resolve(root, corpus_sha256=report['integrity']['sha256'], seed=args.seed,
                        available=positives + negatives, prepared=agentic_layers.prepare(root),
                        warmups=args.warmups, repetitions=args.repetitions)
        runner.write_json(args.output, value)
        print(value['id'])
        return 0
    except (OSError, ValueError) as error:
        print(error.code if isinstance(error, SpecError) else 'benchmark_spec_unresolved', file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
