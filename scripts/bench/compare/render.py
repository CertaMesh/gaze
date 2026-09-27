#!/usr/bin/env python3
"""Render the public aggregate comparison block from a measured report."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

START = "<!-- comparison:start -->"
END = "<!-- comparison:end -->"
ORDER = ("gaze", "presidio-en", "presidio-en-de", "gliner", "opf")


def render(report: dict[str, object], source: str) -> str:
    gaze = report["gaze"]
    tools = report["tools"]
    versions = ("v3", "v2", "v1")
    if set(gaze) != set(versions):
        raise ValueError("public comparison needs a Gaze scorecard for v3, v2, and v1")
    layer_ids = list(report["corpus"]["layers"])
    for version in versions:
        for layer in layer_ids:
            if layer not in gaze[version]["layers"]:
                raise ValueError(f"Gaze lacks layer {layer} under {version}")
            for name, tool in tools.items():
                if layer not in tool["contracts"][version]:
                    raise ValueError(f"{name} lacks layer {layer} under {version}")

    lines = [
        START,
        "### Comparison",
        "",
        "Same corpus and scorer; tools run with their documented defaults. "
        "UTF-8 byte counts use the Gaze scorer. For v3, FP is the scorer's "
        "false-positive count after its audited gold-gap credit. CPU p50/p95 "
        "is warm per-document inference/clean time on the same machine. "
        "Presidio English default leaves German documents unprocessed.",
        "",
        f"Aggregate source: [`{source}`]({source}). Raw document outputs are not published.",
        "",
        "| Contract | Layer | Tool | Leaked B | FP B | CPU p50 ms | CPU p95 ms |",
        "|---|---|---|---:|---:|---:|---:|",
    ]
    better = []
    for version in versions:
        for layer in layer_ids:
            gaze_row = gaze[version]["layers"][layer]
            for name in ORDER:
                if name == "gaze":
                    row = gaze_row
                elif name in tools:
                    row = tools[name]["contracts"][version][layer]
                else:
                    continue
                fp = row["false_positive_bytes_after_gold_gap"] if version == "v3" else row["false_positive_bytes"]
                latency = row["latency"]
                lines.append(
                    f"| {version} | {layer} | {name} | {row['leaked_bytes']:,} | {fp:,} | "
                    f"{latency['p50_ms']:.1f} | {latency['p95_ms']:.1f} |"
                )
                if name != "gaze" and row["leaked_bytes"] < gaze_row["leaked_bytes"]:
                    better.append(f"{version}/{layer}: {name} leaks {row['leaked_bytes']:,} B versus Gaze {gaze_row['leaked_bytes']:,} B")
    lines.extend(["", "**Where Gaze leaks more:** " + ("; ".join(better) if better else "none in these measured rows") + "."])
    skipped = report.get("skipped", {})
    if skipped:
        lines.extend(["", "**Skipped:** " + "; ".join(f"{name}: {reason}" for name, reason in skipped.items()) + "."])
    lines.extend([END, ""])
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--readme", type=Path, required=True)
    args = parser.parse_args()
    report = json.loads(args.report.read_text(encoding="utf-8"))
    block = render(report, args.report.name)
    current = args.readme.read_text(encoding="utf-8")
    if START in current:
        if current.count(START) != 1 or current.count(END) != 1:
            raise ValueError("comparison block markers are not unique")
        before, tail = current.split(START)
        _, after = tail.split(END)
        updated = before + block + after.lstrip("\n")
    else:
        updated = current.rstrip() + "\n\n" + block
    args.readme.write_text(updated, encoding="utf-8")


if __name__ == "__main__":
    main()
