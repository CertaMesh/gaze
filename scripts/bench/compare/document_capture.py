"""Private, exact-input competitor observations. Reuse never measures latency."""
from __future__ import annotations

import json
import math
import os
import tempfile
import time
from pathlib import Path

from capture_identity import digest
import subject_identity as identity


def private_root(path: Path, repository: Path) -> Path:
    path = path.expanduser().resolve()
    if path.is_relative_to(repository.resolve()):
        raise ValueError("private captures must stay outside the repository")
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    path.chmod(0o700)
    return path


def atomic_json(path: Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    descriptor, temporary = tempfile.mkstemp(dir=path.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(value, stream, sort_keys=True, allow_nan=False)
            stream.write("\n")
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def validate_predictions(value, document, mapping):
    if not isinstance(value, list):
        raise ValueError("invalid capture predictions")
    for item in value:
        if (not isinstance(item, dict) or set(item) != {"start", "end", "label"}
                or type(item["start"]) is not int or type(item["end"]) is not int
                or not 0 <= item["start"] < item["end"] <= len(document.text.encode("utf-8"))
                or not isinstance(item["label"], str) or item["label"] not in mapping):
            raise ValueError("invalid capture prediction")


class Session:
    """One independently planned subject. Publish only after endpoint rechecks."""
    def __init__(self, root, spec, read_subject, mapping, *, gliner_plan=None, read_only=False):
        self.root, self.spec, self.read_subject, self.mapping = root, spec, read_subject, mapping
        self.subject = read_subject()
        self.gliner_plan = gliner_plan
        self.read_only = read_only
        self.pending = []
        self.keys = []
        self.observations = []
        self.hits = self.misses = 0

    def recheck(self):
        if digest(self.read_subject()) != digest(self.subject):
            raise ValueError("competitor inputs changed during capture")

    def capture(self, document, predict, *, receipt=None):
        def encode(document):
            return [{"start": s.start, "end": s.end, "label": s.label} for s in predict(document)]
        value, elapsed = self.capture_payload(document, encode, validate_predictions, receipt=receipt)
        import gaze_bench_score as score
        return [score.Span(**span) for span in value], elapsed

    def capture_payload(self, document, predict, validate, *, receipt=None):
        key = identity.document_key(self.spec, self.subject, document)
        self.keys.append(key)
        path = self.root / (digest(key) + ".json")
        if path.exists():
            entry = json.loads(path.read_text(encoding="utf-8"))
            if (set(entry) != {"key", "observation", "gliner_receipt", "sha256"}
                    or digest(entry["key"]) != digest(key)
                    or entry["sha256"] != digest({k: v for k, v in entry.items() if k != "sha256"})):
                raise ValueError("tampered competitor capture")
            identity.verify_gliner(self.subject, entry["gliner_receipt"], self.gliner_plan)
            observation = entry["observation"]
            self.hits += 1
        else:
            if self.read_only:
                raise ValueError("private admission requires a complete capture")
            started = time.perf_counter()
            predictions = predict(document)
            observation = {"predictions": predictions,
                           "elapsed_ms": (time.perf_counter() - started) * 1000}
            receipt = receipt() if callable(receipt) else receipt
            identity.verify_gliner(self.subject, receipt, self.gliner_plan)
            entry = {"key": key, "observation": observation, "gliner_receipt": receipt}
            entry["sha256"] = digest(entry)
            self.pending.append((path, entry))
            self.misses += 1
        if (not isinstance(observation, dict) or set(observation) != {"predictions", "elapsed_ms"}
                or type(observation["elapsed_ms"]) not in (int, float)
                or not math.isfinite(observation["elapsed_ms"]) or observation["elapsed_ms"] < 0):
            raise ValueError("invalid capture observation")
        validate(observation["predictions"], document, self.mapping)
        self.observations.append(entry["sha256"])
        return observation["predictions"], observation["elapsed_ms"]

    def finish(self):
        self.recheck()
        for path, entry in self.pending:
            atomic_json(path, entry)
        self.pending.clear()
        return identity.public_binding(self.observations)

    def retain_row(self, row, contention):
        """Rescored metrics must reproduce the retained row, including original latency."""
        path = self.root / ("row-" + digest(self.keys) + ".json")
        if path.exists():
            original = json.loads(path.read_text())
            if (set(original) != {"row", "contention", "sha256"}
                    or original["sha256"] != digest({k: v for k, v in original.items() if k != "sha256"})
                    or digest(original["row"]["contracts"]) != digest(row["contracts"])
                    or digest(original["row"]["provenance"]) != digest(row["provenance"])
                    or original["row"].get("capture_binding") != identity.public_binding(self.observations)
                    or original["row"]["tool"] != self.subject["name"]):
                raise ValueError("private aggregate replay mismatch")
            return original["row"], original["contention"]
        if self.read_only:
            raise ValueError("private admission requires original aggregate evidence")
        original = {"row": row, "contention": contention}
        original["sha256"] = digest(original)
        atomic_json(path, original)
        return row, contention
