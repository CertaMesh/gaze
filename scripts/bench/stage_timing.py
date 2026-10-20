"""Execution evidence kept outside deterministic observation records and scorecards."""

from __future__ import annotations

import contextlib
import contextvars
import functools
import json
import math
import time
from dataclasses import dataclass, field
from pathlib import Path

try:
    import resource
except ImportError:
    resource = None


CURRENT = contextvars.ContextVar("benchmark_stage_evidence", default=None)
LABELS = contextvars.ContextVar("benchmark_stage_labels", default={})
ACTIVE_SCORING = contextvars.ContextVar("benchmark_scoring_clock", default=None)
NATIVE_PREFIX = b"GAZE_BENCH_PROFILE "
NATIVE_STAGES = frozenset({"startup_model_load", "detection", "restore"})


def clock() -> tuple[float, float, float]:
    children = resource.getrusage(resource.RUSAGE_CHILDREN) if resource else None
    return (time.perf_counter(), time.process_time(),
            children.ru_utime + children.ru_stime if children else 0.0)


def elapsed(start: tuple[float, float, float]) -> tuple[float, float, float]:
    return tuple(max(0.0, end - begin) for begin, end in zip(start, clock(), strict=True))


@dataclass
class Evidence:
    events: list[dict] = field(default_factory=list)
    cache: dict = field(default_factory=lambda: {"status": "disabled", "key_sha256": None})
    outcome: str = "running"

    def add(self, stage: str, duration: tuple[float, float, float], **details) -> None:
        wall, own, children = duration
        self.events.append({
            "stage": stage, **LABELS.get(), **details,
            "wall_seconds": wall, "cpu_self_seconds": own,
            "cpu_children_seconds": children if resource else None,
            "cpu_seconds": own + children if resource else None,
        })

    def write(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_suffix(path.suffix + ".tmp")
        temporary.write_text(json.dumps({
            "schema_version": 1, "outcome": self.outcome, "cache": self.cache,
            "timing_semantics": "inclusive spans; native stages use child process CPU; scoring excludes transport",
            "stages": self.events,
        }, indent=2, allow_nan=False) + "\n")
        temporary.replace(path)


@contextlib.contextmanager
def session(path: Path):
    evidence = Evidence()
    token = CURRENT.set(evidence)
    label_token = LABELS.set({})
    try:
        yield evidence
        evidence.outcome = "completed"
    except BaseException:
        evidence.outcome = "failed"
        raise
    finally:
        CURRENT.reset(token)
        LABELS.reset(label_token)
        evidence.write(path)


@contextlib.contextmanager
def labels(**values):
    token = LABELS.set({**LABELS.get(), **values})
    try:
        yield
    finally:
        LABELS.reset(token)


@contextlib.contextmanager
def measure(stage: str, **details):
    evidence = CURRENT.get()
    start = clock() if evidence else None
    try:
        yield
    finally:
        if evidence:
            evidence.add(stage, elapsed(start), **details)


def timed(stage: str, **details):
    def decorate(function):
        @functools.wraps(function)
        def wrapped(*args, **kwargs):
            if CURRENT.get() is None:
                return function(*args, **kwargs)
            attributes = {name: value(*args, **kwargs) if callable(value) else value
                          for name, value in details.items()}
            with measure(stage, **attributes):
                return function(*args, **kwargs)
        return wrapped
    return decorate


class ScoringClock:
    """Parent scoring/bookkeeping time, excluding exchanges and child lifetime boundaries."""

    def __init__(self, enabled: bool):
        self.evidence = CURRENT.get() if enabled else None
        self.start = clock() if self.evidence else None
        self.excluded = [0.0, 0.0, 0.0]

    def exclude_since(self, start) -> None:
        if self.evidence:
            self.excluded = [a + b for a, b in zip(self.excluded, elapsed(start), strict=True)]

    def excluding(self, function):
        @functools.wraps(function)
        def wrapped(*args, **kwargs):
            start = clock() if self.evidence else None
            try:
                return function(*args, **kwargs)
            finally:
                self.exclude_since(start)
        return wrapped

    def finish(self) -> None:
        if self.evidence:
            total = elapsed(self.start)
            wall, own = (max(0.0, total[i] - self.excluded[i]) for i in (0, 1))
            self.evidence.add("scoring", (wall, own, 0.0), mode="capture_accounting")


@contextlib.contextmanager
def scoring():
    observer = ScoringClock(True)
    token = ACTIVE_SCORING.set(observer)
    try:
        yield
    finally:
        ACTIVE_SCORING.reset(token)
        observer.finish()


def transport_call(function):
    """Subtract producer I/O and process lifetime boundaries from parent scoring."""
    @functools.wraps(function)
    def wrapped(*args, **kwargs):
        observer = ACTIVE_SCORING.get()
        if observer is None:
            return function(*args, **kwargs)
        return observer.excluding(function)(*args, **kwargs)
    return wrapped


def validate_native(value: object) -> dict:
    if not isinstance(value, dict) or set(value) != NATIVE_STAGES:
        raise ValueError("invalid native stage profile")
    for row in value.values():
        if not isinstance(row, dict) or set(row) != {"wall_seconds", "cpu_seconds", "samples"}:
            raise ValueError("invalid native stage fields")
        if type(row["samples"]) is not int or row["samples"] < 0:
            raise ValueError("invalid native stage sample count")
        for name in ("wall_seconds", "cpu_seconds"):
            if type(row[name]) not in (int, float) or not math.isfinite(row[name]) or row[name] < 0:
                raise ValueError("invalid native stage duration")
    return value


class NativeProfileReader:
    """Retain only a bounded metadata line; discard arbitrary producer diagnostics."""

    def __init__(self):
        self.pending = bytearray()
        self.discarding = False
        self.result = None

    def feed(self, chunk: bytes) -> None:
        for fragment in chunk.splitlines(keepends=True):
            complete = fragment.endswith(b"\n")
            if not self.discarding:
                self.pending.extend(fragment)
                if len(self.pending) > 4096:
                    if self.pending.startswith(NATIVE_PREFIX):
                        raise ValueError("native stage profile exceeds limit")
                    self.pending.clear()
                    self.discarding = True
                elif complete:
                    if self.pending.startswith(NATIVE_PREFIX):
                        if self.result is not None:
                            raise ValueError("duplicate native stage profile")
                        self.result = validate_native(json.loads(self.pending[len(NATIVE_PREFIX):]))
                    self.pending.clear()
            if complete:
                self.discarding = False


def native(profile: dict | None) -> None:
    evidence = CURRENT.get()
    if evidence is None:
        return
    if profile is None:
        evidence.events.append({"stage": "native_profile", **LABELS.get(), "status": "unavailable"})
        return
    for stage, row in validate_native(profile).items():
        evidence.events.append({
            "stage": stage, **LABELS.get(), **row,
            "cpu_source": "producer_process_clock",
        })
