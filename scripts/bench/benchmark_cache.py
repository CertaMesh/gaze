"""Exact-key cache for value-free benchmark observation records."""

from __future__ import annotations

import fcntl
import hashlib
import json
import shutil
import gzip
from pathlib import Path
from typing import Mapping


METADATA = "key.json"
RECORD = "observations-v1.jsonl.gz"


def key_digest(key: Mapping[str, object]) -> str:
    payload = json.dumps(key, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _differences(expected: Mapping[str, object], actual: Mapping[str, object]) -> list[str]:
    return sorted(
        key for key in expected.keys() | actual.keys()
        if expected.get(key) != actual.get(key)
    )


def _read_metadata(path: Path) -> dict[str, object]:
    metadata = json.loads(path.read_text(encoding="utf-8"))
    if (
        not isinstance(metadata, dict)
        or not isinstance(metadata.get("key"), dict)
        or not isinstance(metadata.get("record_sha256"), str)
    ):
        raise ValueError("metadata must contain an object key and record_sha256")
    return metadata


def lookup(root: Path, key: Mapping[str, object]) -> tuple[Path | None, str]:
    digest = key_digest(key)
    entry = root / digest
    metadata_path = entry / METADATA
    record = entry / RECORD
    if metadata_path.is_file():
        try:
            metadata = _read_metadata(metadata_path)
        except (OSError, UnicodeError, json.JSONDecodeError, ValueError) as error:
            return None, f"cache miss {digest}: invalid metadata ({error})"
        differences = _differences(key, metadata.get("key", {}))
        if differences:
            return None, f"cache miss {digest}: key mismatch in {', '.join(differences)}"
        if not record.is_file():
            return None, f"cache miss {digest}: observation record is missing"
        observed = _sha256(record)
        if observed != metadata.get("record_sha256"):
            return None, f"cache miss {digest}: record sha256 mismatch"
        return record, f"cache hit {digest}"

    nearest: list[tuple[int, list[str]]] = []
    if root.is_dir():
        for candidate in root.glob(f"*/{METADATA}"):
            try:
                metadata = _read_metadata(candidate)
                differences = _differences(key, metadata.get("key", {}))
            except (OSError, UnicodeError, json.JSONDecodeError, TypeError, ValueError):
                continue
            nearest.append((len(differences), differences))
    if nearest:
        differences = min(nearest, key=lambda item: (item[0], item[1]))[1]
        return None, f"cache miss {digest}: no exact key; changed {', '.join(differences)}"
    return None, f"cache miss {digest}: cache is empty"


def store(root: Path, key: Mapping[str, object], source: Path) -> Path:
    digest = key_digest(key)
    entry = root / digest
    entry.mkdir(parents=True, exist_ok=True)
    record = entry / RECORD
    with (entry / ".lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        temporary = entry / f".{RECORD}.tmp"
        shutil.copyfile(source, temporary)
        temporary.replace(record)
        metadata = {
            "key": key,
            "record_sha256": _sha256(record),
        }
        metadata_path = entry / METADATA
        metadata_temporary = entry / f".{METADATA}.tmp"
        metadata_temporary.write_text(
            json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        metadata_temporary.replace(metadata_path)
    return record


def copy_verified(source: Path, destination: Path, key: Mapping[str, object]) -> None:
    """Verify the private copy against metadata, including a changed lookup source."""
    metadata = _read_metadata(source.with_name(METADATA))
    if metadata["key"] != key:
        raise ValueError("cache copy key mismatch")
    shutil.copyfile(source, destination)
    if _sha256(destination) != metadata["record_sha256"]:
        destination.unlink(missing_ok=True)
        raise ValueError("cache copy record sha256 mismatch")


def store_layers(root: Path, key: Mapping[str, object], source: Path) -> None:
    """Publish a bound header and independently hashed layer records, metadata last."""
    import scorecard_record as records
    import stage_timing as stages

    header, observations = records._read(source)
    digest = key_digest(key)
    if not key.get("binary_sha256") or header.get("binary_sha256") != key["binary_sha256"]:
        raise ValueError("observation record has no matching capture-time binary SHA-256")
    if header["scorecard"].get("capture_cache") != provenance(key):
        raise ValueError("observation record is not bound to this full cache key")
    entry = root / digest
    entry.mkdir(parents=True, exist_ok=True)
    with (entry / ".lock").open("w") as lock, stages.measure("artifact_write_compress", artifact="layer_cache"):
        fcntl.flock(lock, fcntl.LOCK_EX)
        parts = {"header": [header]}
        for index, row in enumerate(observations):
            layer = row["layer"]
            if layer not in {"C", "A", "D", "R"}:
                raise ValueError("unsupported capture layer")
            parts.setdefault(layer, []).append({"index": index, "observation": row})
        hashes = {}
        for layer, rows in parts.items():
            temporary = entry / f".{layer}.jsonl.gz.tmp"
            records._write_rows(temporary, rows)
            path = entry / f"{layer}.jsonl.gz"
            temporary.replace(path)
            hashes[layer] = _sha256(path)
        metadata = {"format": "layer-capture-v1", "key": key,
                    "record_sha256": _sha256(source), "parts": hashes}
        temporary = entry / ".layers.json.tmp"
        temporary.write_text(json.dumps(metadata, sort_keys=True) + "\n", encoding="utf-8")
        temporary.replace(entry / "layers.json")


def provenance(key: Mapping[str, object]) -> dict:
    # Execution-specific hit/miss lives in the linked evidence, preserving scorecard bytes.
    return {"format": "layer-capture-v1", "key_sha256": key_digest(key),
            "execution_evidence": "run-evidence.json"}


def restore_layers(root: Path, key: Mapping[str, object], destination: Path) -> str:
    """All-or-nothing exact replay. Invalid/missing parts are a miss, never a fallback."""
    import scorecard_record as records
    import stage_timing as stages

    entry = root / key_digest(key)
    if not (entry / "layers.json").is_file():
        return "miss_absent"
    temporary = destination.with_suffix(destination.suffix + ".cache-tmp")
    try:
        with (entry / ".lock").open("r") as lock, stages.measure("cache_verify"):
            fcntl.flock(lock, fcntl.LOCK_SH)
            metadata = _read_metadata(entry / "layers.json")
            if (metadata.get("format") != "layer-capture-v1" or metadata["key"] != key
                    or key_digest(metadata["key"]) != key_digest(key)):
                return "miss_key"
            parts = metadata.get("parts")
            if (not isinstance(parts, dict) or "header" not in parts
                    or not set(parts) <= {"header", "C", "A", "D", "R"}):
                return "miss_metadata"
            rows = {}
            header = None
            for layer, expected in parts.items():
                part = entry / f"{layer}.jsonl.gz"
                if _sha256(part) != expected:
                    return "miss_record_hash"
                with gzip.open(part, "rt", encoding="utf-8") as stream:
                    data = [json.loads(line) for line in stream]
                if layer == "header":
                    if len(data) != 1 or data[0]["scorecard"].get("capture_cache") != provenance(key):
                        return "miss_binding"
                    header = data[0]
                    if not key.get("binary_sha256") or header.get("binary_sha256") != key["binary_sha256"]:
                        return "miss_binary_binding"
                else:
                    for item in data:
                        index = item["index"]
                        if type(index) is not int or index in rows or item["observation"]["layer"] != layer:
                            return "miss_inventory"
                        rows[index] = item["observation"]
            if sorted(rows) != list(range(len(rows))):
                return "miss_inventory"
            records._write_rows(temporary, [header, *(rows[i] for i in range(len(rows)))])
            if _sha256(temporary) != metadata["record_sha256"]:
                return "miss_record_hash"
            records._read(temporary)
            destination.parent.mkdir(parents=True, exist_ok=True)
            temporary.replace(destination)
            return "hit"
    except (OSError, UnicodeError, ValueError, KeyError, TypeError, EOFError):
        return "miss_invalid"
    finally:
        temporary.unlink(missing_ok=True)
