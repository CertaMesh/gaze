"""Text-free per-document timers, bound to the predictions measured in the same run."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

from loaders import score


def latency_ns(samples: list[int]) -> dict:
    timers = [ns / 1_000_000 for ns in samples]
    return {'p50_ms': round(score.percentile(timers, .5), 3) if timers else None,
            'p95_ms': round(score.percentile(timers, .95), 3) if timers else None,
            'samples': len(timers)}


def check_samples(path: Path, receipt: dict, latency: dict, prediction_sha256: str) -> None:
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != receipt['sha256']:
        raise ValueError('timing sample digest mismatch')
    if receipt['prediction_sha256'] != prediction_sha256:
        raise ValueError('timing prediction digest mismatch')
    samples = []
    for index, line in enumerate(raw.splitlines()):
        item = json.loads(line)
        if (set(item) != {'index', 'ns'} or type(item['index']) is not int
                or item['index'] != index or type(item['ns']) is not int or item['ns'] < 0):
            raise ValueError('timing samples need ordered document indices and nonnegative integer ns only')
        samples.append(item['ns'])
    if latency_ns(samples) != latency:
        raise ValueError('timing quantiles or sample count differ from retained samples')
