"""Emit Markdown table headers from one column definition."""

from __future__ import annotations

from collections.abc import Sequence


def table_header(columns: Sequence[tuple[str, bool]]) -> list[str]:
    """Return header and delimiter rows; the bool right-aligns a column."""
    if not columns:
        raise ValueError("a Markdown table needs at least one column")
    return [
        "| " + " | ".join(title for title, _ in columns) + " |",
        "| " + " | ".join("---:" if right else "---" for _, right in columns) + " |",
    ]
