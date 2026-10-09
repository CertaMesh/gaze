#!/usr/bin/env python3
"""Render Perplexity PII-Tracer's own-corpus measurement next to the committed competitors.

Reads docs/reference/benchmarks/comparison-pii-tracer.json (written by pii_tracer.py) and
comparison.json, and writes docs/reference/benchmarks/competitors-pii-tracer.md. The page is
generated: `--check` fails when it drifts. The two reports must describe the same corpus and the
same v1-v3 scored-label contracts or rendering refuses, so every row is comparable to the others.
Gaze rows are not shown here: only tagged releases are published, on competitors.md.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Mapping

REPO = Path(__file__).resolve().parents[3]
BENCHMARKS = REPO / "docs/reference/benchmarks"
REPORT = BENCHMARKS / "comparison-pii-tracer.json"
COMMITTED = BENCHMARKS / "comparison.json"
PAGE = BENCHMARKS / "competitors-pii-tracer.md"
TOOL = "pii-tracer"
LAYERS = ("C", "A", "D", "R")
CONTRACTS = ("v3", "v2", "v1")
LAYER_NAMES = {"C": "C (prose holdout)", "A": "A (generated identifiers)", "D": "D (benign lookalikes)",
               "R": "R (repeated values)"}


def check_comparable(report: Mapping[str, Any], committed: Mapping[str, Any]) -> None:
    for key in ("corpus", "common_intersection_labels"):
        if report[key] != committed[key]:
            raise ValueError(f"{key} differs from comparison.json; the rows are not comparable")
    if any(
        version not in report["contracts"]
        or version not in committed["contracts"]
        or report["contracts"][version] != committed["contracts"][version]
        for version in CONTRACTS
    ):
        raise ValueError("v1-v3 contracts differ from comparison.json; the rows are not comparable")
    if report["harness_dirty"] or set(report["tools"]) != {TOOL}:
        raise ValueError("the PII-Tracer report must be a clean single-tool measurement")


def cell(tool: Mapping[str, Any], version: str, layer: str) -> Mapping[str, Any]:
    return tool["contracts"][version][layer]


def full(tool: Mapping[str, Any], version: str, layer: str) -> Mapping[str, Any]:
    return cell(tool, version, layer)["metrics"]["product_coverage"]["full"]


def foreign_cpu_max(report: Mapping[str, Any]) -> float:
    return max(sample["foreign_cpu_percent"] for sample in report["contention"]["foreign_cpu_samples"])


def render(report: Mapping[str, Any], committed: Mapping[str, Any]) -> str:
    check_comparable(report, committed)
    tool = report["tools"][TOOL]
    provenance = tool["provenance"]
    runtime = provenance["runtime"]
    generator = report["corpus"]["agentic"]["generator_version"]
    peak = foreign_cpu_max(report)
    lines = [
        "# PII-Tracer on Gaze's corpus",
        "",
        "Perplexity's [PII-Tracer](https://huggingface.co/perplexity-ai/PII-Tracer) is a 0.6B on-device PII "
        "detector. This page scores it on the same corpus, scorer and scored-label contracts as the other "
        "competitors in [competitors.md](competitors.md), full-corpus layer C. Byte leaks, false positives "
        "and character F2 use the shared scorer. Only aggregates are published. Gaze rows appear on "
        "competitors.md, where only tagged releases are shown.",
        "",
        f"**Declared configuration** (fixed from the model card before any result): model "
        f"`{provenance['model_repo']}` at revision `{provenance['revision']}` (MIT); {runtime['device']}, "
        f"stored {runtime['dtype']}, `predict()` decoding, non-overlapping 4080-token windows for longer "
        f"documents, no threshold (the card documents none, so there is no sweep). torch {runtime['torch']}, "
        f"transformers {runtime['transformers']}. The checkpoint's own Python code runs under "
        "`trust_remote_code`; the worker refuses to load unless five pinned file hashes match.",
        "",
        f"**Corpus.** Identical to `comparison.json` (measured on agentic generator v{generator}, "
        f"{sum(v['documents'] for v in report['corpus']['layers'].values()):,} documents). "
        "Later generator versions add documents that are not measured here.",
        "",
        "## Layer C, scored-label contract v3",
        "",
        "| Tool | Char F2 | Leaked B | FP B | Documents leaking |",
        "| --- | ---: | ---: | ---: | ---: |",
    ]
    rows = {name: full(row, "v3", "C") for name, row in committed["tools"].items()}
    rows[TOOL] = full(tool, "v3", "C")
    for name in sorted(rows, key=lambda n: (rows[n]["leaked_bytes"], n)):
        m = rows[name]
        lines.append(f"| {name} | {m['char_level']['f2']:.3f} | {m['leaked_bytes']:,} | "
                     f"{m['false_positive_bytes']:,} | {m['document_leak_rate'] * 100:.1f}% |")
    lines += ["", "## PII-Tracer by layer and contract (leaked B / false-positive B)", "",
              "| Layer | " + " | ".join(CONTRACTS) + " |", "| --- | " + " | ".join("---:" for _ in CONTRACTS) + " |"]
    for layer in LAYERS:
        cells = " | ".join(f"{cell(tool, v, layer)['leaked_bytes']:,} / {cell(tool, v, layer)['false_positive_bytes']:,}"
                           for v in CONTRACTS)
        lines.append(f"| {LAYER_NAMES[layer]} | {cells} |")
    lines += [
        "",
        "Layers A, D and R are generated. Layer D has no gold: every byte is a false positive.",
        "",
        "## Latency",
        "",
        ("Latency is withheld. Other processes used up to " f"{peak:.0f}% of a core while this ran "
         "(the rule is at most 100%), so no timing is comparable."
         if peak > 100 else "Foreign CPU stayed at or under 100% of a core during the run."),
        "",
        f"Harness `{report['harness_revision'][:8]}`, {report['hardware']}. Worker "
        f"`{runtime['worker_sha256'][:12]}`, dependency lock `{runtime['requirements_sha256'][:12]}`.",
        "",
    ]
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    body = render(json.loads(REPORT.read_text(encoding="utf-8")), json.loads(COMMITTED.read_text(encoding="utf-8")))
    if args.check:
        if not PAGE.exists() or PAGE.read_text(encoding="utf-8") != body:
            print(f"{PAGE}: stale; run render_pii_tracer.py", file=sys.stderr)
            return 1
        return 0
    PAGE.write_text(body, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
