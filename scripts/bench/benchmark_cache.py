"""Exact-key cache for value-free benchmark observation records."""

from __future__ import annotations

import fcntl
import hashlib
import json
import shutil
from pathlib import Path
from typing import Mapping


METADATA = "key.json"
RECORD = "observations-v1.jsonl.gz"


def key_digest(key: Mapping[str, object]) -> str:
    payload = json.dumps(key, sort_keys=True, separators=(",", ":")).encode("utf-8")
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
