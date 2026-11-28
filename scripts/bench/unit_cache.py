"""Per-document detector response reuse, keyed on detection identity and the exact request.

A unit is one detector response for one config and one request (fixture ID,
locale chain and text). Its key is the detection identity, meaning every input
that can change a response, plus that request. The detection identity contains:

- the detector and validator executable bytes, the policy and its dependency
  files, and every model tree the policy loads;
- the environment, the native and Python runtimes, the machine, the build
  features, the threshold and the warmups;
- the response validation and record schema;
- the harness code on the request and response path (`UNIT_CODE`).

Generator, population, scored-label and gate code are not in the key: they
decide which requests exist and how responses are scored, and neither reaches a
response. Gold spans are not in the key either; gold-gap evidence is recomputed
from the current document when a record is assembled. The detector builds a
fresh session per request from its fixture ID, so a response does not depend
on which other documents share the process. The shadow check in this module
proves that on a full capture before reuse is trusted.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import random
import sys
import zlib
from pathlib import Path
from typing import Mapping, Sequence

from capture_identity import digest, file_digest

FORMAT = "unit-response-v1"
# Harness files on the request/response path: request construction, response
# validation, transport, compaction and this cache.
UNIT_CODE = ("bench_subprocess.py", "capture_identity.py", "gaze_bench_score.py",
             "refusal_contract.py", "scorecard_record.py", "stage_timing.py", "unit_cache.py")
# Observation-cache key fields that name the population or how it is scored.
POPULATION_FIELDS = ("layer_specs", "layer_spec", "corpus_sha256", "agentic_corpus_sha256",
                     "document_ids_sha256", "scored_labels_sha256", "agentic_scored_labels_sha256",
                     "seed", "profile", "max_documents", "capture_protocol", "measured_repetitions")
POPULATION_FILES = ("dataset", "negative_corpus", "benchmark_protocol", "scored_labels",
                    "agentic_scored_labels")


def detector_request(document) -> dict[str, object]:
    """The complete request a detector process receives for one document."""
    return {"fixture_id": document.uid, "locale_chain": document.locale_chain, "text": document.text}


# A reused response was not timed in this run; latency summaries skip it.
REUSED_TIMING = {"clean_ms": None, "restore_ms": None, "post_policy_scan_ms": None}


def run_config(units: "UnitCache", repo_root, binary, config, documents, *args,
               warmup_count: int = 0, record_document=None, **kwargs) -> dict[str, object]:
    """`gaze_bench_score.run_config` with stored responses answering the documents they hold.

    Missing documents run live in one warm process, with the warmups; then every
    document is scored through the scorer's replay path in its original order,
    so records keep their row order. Latency and process provenance come from
    the live run only; a fully reused run has no timing samples.
    """
    import gaze_bench_score as score

    # Response validation flags are not part of a unit; only the defaults reuse.
    if (kwargs.get("replacing_actions", score.MANIFEST_REPLACING_ACTIONS) != score.MANIFEST_REPLACING_ACTIONS
            or kwargs.get("split_composite_source_ids", False)):
        raise ValueError("per-document reuse supports only the default response validation")
    requests = {document.uid: detector_request(document) for document in documents}
    responses = {}
    for document in documents:
        reused = units.get(config, requests[document.uid])
        if reused is not None:
            responses[document.uid] = {**reused, "timing": dict(REUSED_TIMING)}
    missing = [document for document in documents if document.uid not in responses]
    live = None
    if missing:
        fresh = {}
        live = score.run_config(repo_root, binary, config, missing, *args, warmup_count=warmup_count,
                                record_document=lambda _config, document, response, _measured:
                                    fresh.__setitem__(document.uid, response), **kwargs)
        for document in missing:
            units.put(config, requests[document.uid], fresh[document.uid])
            responses[document.uid] = fresh[document.uid]
    run = score.run_config(repo_root, binary, config, documents, *args, warmup_count=0,
                           record_document=record_document, replay_responses=responses, **kwargs)
    if live is not None:
        for key in ("latency_ms", "warm_latency_ms", "process"):
            run[key] = live[key]
    else:
        # Nothing ran: no timing was measured, which is not a zero measurement.
        run["process"] = {**run["process"], "wall_seconds": None, "documents_per_second": None,
                          "start_to_first_response_ms": None,
                          "cold_start_to_first_validated_response_ms": None,
                          "warmup_count": warmup_count, "discarded_warmup_samples": [],
                          "reused_only": True}
    return run


def detection_identity(observation_key: Mapping[str, object], bench: Path,
                       machine: Mapping[str, object]) -> dict[str, object]:
    """The observation-cache key without its population, plus harness code and machine."""
    identity = {key: value for key, value in observation_key.items() if key not in POPULATION_FIELDS}
    inputs = dict(identity["effective_inputs"])
    inputs["files"] = {role: value for role, value in inputs["files"].items()
                       if role not in POPULATION_FILES}
    identity["effective_inputs"] = inputs
    identity["unit_format"] = FORMAT
    identity["unit_code"] = {name: file_digest(bench / name) for name in UNIT_CODE}
    # Response validation admits source IDs from this vocabulary; reused
    # responses are not revalidated, so the effective vocabulary is bound.
    import gaze_bench_score as score
    identity["validation_vocabulary"] = sorted(score.load_committed_source_id_vocabulary(bench.parents[1]))
    identity["machine"] = dict(machine)
    return identity


def _canonical(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False,
                      allow_nan=False).encode("utf-8")


def _unit_digest(config: str, request: Mapping[str, object]) -> str:
    return hashlib.sha256(_canonical({"config": config, "request": request})).hexdigest()


class UnitCache:
    """Read verified units, buffer new ones, and publish them only after the inputs re-verify.

    `read=False` captures every document live (the `--unit-shadow` capture).
    Lookups for the assembled shadow record use `peek`, which never counts
    toward reuse.
    """

    def __init__(self, root: Path, identity: Mapping[str, object], *, read: bool = True,
                 write: bool = True) -> None:
        self.identity_sha256 = digest(identity)
        self.directory = root / "units" / self.identity_sha256
        self.identity = identity
        self.read = read
        self.write = write
        self.pending: dict[str, dict[str, object]] = {}
        self.reused = 0
        self.live = 0

    def _path(self, unit: str) -> Path:
        return self.directory / unit[:2] / f"{unit}.json.gz"

    def peek(self, config: str, request: Mapping[str, object]) -> dict[str, object] | None:
        unit = _unit_digest(config, request)
        path = self._path(unit)
        try:
            with gzip.open(path, "rb") as stream:
                entry = json.loads(stream.read())
        except FileNotFoundError:
            return None
        except (OSError, EOFError, UnicodeError, json.JSONDecodeError, zlib.error):
            return None
        response = entry.get("response") if isinstance(entry, dict) else None
        if (not isinstance(response, dict) or entry.get("format") != FORMAT
                or entry.get("identity_sha256") != self.identity_sha256
                or entry.get("unit") != unit
                or entry.get("response_sha256") != hashlib.sha256(_canonical(response)).hexdigest()
                or response.get("fixture_id") != request["fixture_id"]):
            return None
        return response

    def get(self, config: str, request: Mapping[str, object]) -> dict[str, object] | None:
        if not self.read:
            return None
        response = self.peek(config, request)
        if response is not None:
            self.reused += 1
        return response

    def put(self, config: str, request: Mapping[str, object], response: Mapping[str, object]) -> None:
        import scorecard_record as records

        self.live += 1
        if self.write:
            self.pending[_unit_digest(config, request)] = records._compact_response(response)

    def publish(self) -> int:
        """Write buffered units atomically; an existing unit is never replaced."""
        if not self.write or not self.pending:
            return 0
        self.directory.mkdir(parents=True, exist_ok=True)
        identity = self.directory / "identity.json"
        if not identity.is_file():
            temporary = identity.with_name(f".identity.{os.getpid()}.tmp")
            temporary.write_text(json.dumps(self.identity, indent=2, sort_keys=True) + "\n",
                                 encoding="utf-8")
            temporary.replace(identity)
        written = 0
        for unit, response in sorted(self.pending.items()):
            path = self._path(unit)
            if path.is_file():
                continue
            path.parent.mkdir(exist_ok=True)
            temporary = path.with_name(f".{unit}.{os.getpid()}.tmp")
            entry = {"format": FORMAT, "identity_sha256": self.identity_sha256, "unit": unit,
                     "response_sha256": hashlib.sha256(_canonical(response)).hexdigest(),
                     "response": response}
            # Insertion order is kept: records serialize responses unsorted.
            with temporary.open("wb") as raw, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as stream:
                stream.write(json.dumps(entry, ensure_ascii=False, separators=(",", ":"),
                                        allow_nan=False).encode("utf-8"))
            temporary.replace(path)
            written += 1
        self.pending.clear()
        return written

    def provenance(self) -> dict[str, object]:
        return {"format": FORMAT, "identity_sha256": self.identity_sha256,
                "reused_responses": self.reused, "live_responses": self.live}


def observation_rows(path: Path) -> list[bytes]:
    """The canonical observation rows of a record, without its header."""
    with gzip.open(path, "rt", encoding="utf-8") as stream:
        rows = [json.loads(line) for line in stream]
    if not rows or rows[0].get("kind") != "header":
        raise ValueError("record has no header")
    return [_canonical(row) for row in rows[1:]]


def compare(left: Path, right: Path) -> dict[str, object]:
    """Byte-compare two records' observation rows (headers carry run timing)."""
    a, b = observation_rows(left), observation_rows(right)
    differing = sum(x != y for x, y in zip(a, b))
    return {"left": left.as_posix(), "right": right.as_posix(), "left_rows": len(a),
            "right_rows": len(b), "differing_rows": differing + abs(len(a) - len(b)),
            "identical": a == b}


def evict(root: Path, identity_sha256: str, fraction: float, seed: int) -> dict[str, int]:
    """Delete a seeded fraction of one identity's units, to force a mixed live/reused capture."""
    units = sorted((root / "units" / identity_sha256).glob("*/*.json.gz"))
    chosen = random.Random(seed).sample(units, round(len(units) * fraction))
    for path in chosen:
        path.unlink()
    return {"units": len(units), "evicted": len(chosen)}


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    comparison = commands.add_parser("compare", help="byte-compare two records' observation rows")
    comparison.add_argument("left", type=Path)
    comparison.add_argument("right", type=Path)
    eviction = commands.add_parser("evict", help="delete a seeded fraction of one identity's units")
    eviction.add_argument("--cache-dir", type=Path, required=True)
    eviction.add_argument("--identity", required=True)
    eviction.add_argument("--fraction", type=float, default=0.5)
    eviction.add_argument("--seed", type=int, default=20260710)
    args = parser.parse_args(argv)
    if args.command == "compare":
        result = compare(args.left, args.right)
        print(json.dumps(result, indent=2))
        return 0 if result["identical"] else 1
    print(json.dumps(evict(args.cache_dir, args.identity, args.fraction, args.seed), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
