#!/usr/bin/env python3
"""Rebuild the committed Rust boundary probes from application-owned gold."""
import hashlib
import json
from pathlib import Path

import agentic_layers as api

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "crates/gaze-recognizers/tests/fixtures/record_id_boundaries_v27.json"


def fixture():
    records = [row for partition in reversed(api.PARTITIONS)
               for row in api.record_id_boundary.records(api, partition)
               if "-000-" in row.uid or ("-001-" in row.uid and
                                        row.surface.startswith("recordids_boundary_connectors_"))]
    return {
        "generator_version": api.GENERATOR_VERSION,
        "producer_sha256": hashlib.sha256((ROOT / "scripts/bench/record_id_boundary_cells.py").read_bytes()).hexdigest(),
        "selection": "Index 0 all boundary cells plus index 1 uppercase connector cells; both partitions; application-owned UTF-8 gold ranges. Generator v27 includes quote dialects, prior quoted public fields, bounded enclosing strings and unescaped record line boundaries.",
        "records": [{"id": row.uid, "family": row.family, "surface": row.surface,
                     "text": row.text, "gold": [{"start": gold.start, "end": gold.end, "label": gold.label}
                                                for gold in row.gold]} for row in records],
    }


if __name__ == "__main__":
    OUTPUT.write_text(json.dumps(fixture(), ensure_ascii=False, indent=2) + "\n")
