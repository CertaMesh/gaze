"""Portable, content-addressed measuring specification, independent of the subject."""

from __future__ import annotations

import copy
import json
import os
import re
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
                or set(inputs) not in (fields, fields | {"validation_vocabulary"})
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


def population(documents) -> dict:
    ids = sorted(document.uid for document in documents)
    if len(ids) != len(set(ids)):
        raise SpecError("benchmark_population_invalid")
    return {"documents": len(ids), "document_ids_sha256": digest(ids)}


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
    sources += [bench / "pyproject.toml", bench / "uv.lock"]
    return {
        "harness": {path.relative_to(root).as_posix(): file_digest(path) for path in sources},
        "validation_vocabulary": sorted(score.load_committed_source_id_vocabulary(root)),
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
        populations.update({name: population(docs) for name, docs in (
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
