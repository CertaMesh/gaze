"""What a live-verification record proves, shared by the page renderers (stdlib only)."""

from __future__ import annotations

from typing import Mapping


def label_only(difference: Mapping[str, list]) -> bool:
    """Live and replay cover the same bytes and differ only in an entity label."""
    return (sorted((start, end) for start, end, _ in difference["live_only"])
            == sorted((start, end) for start, end, _ in difference["replay_only"]))


def byte_identical(live: Mapping[str, object] | None) -> bool:
    """Every difference that persisted on rerun changed a label, never a byte."""
    if not live:
        return False
    return all(label_only(live["differences"][uid]) for uid in live["persistent"])


def label_only_count(live: Mapping[str, object]) -> int:
    return sum(1 for uid in live["persistent"] if label_only(live["differences"][uid]))
