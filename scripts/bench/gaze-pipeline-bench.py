#!/usr/bin/env python3
"""FROZEN LEGACY v0.9 HISTORY -- NOT AN ACTIVE SCORER.

This path previously generated schema-1 pipeline snapshots and inferred safety
coverage from manifests and leak suspects. That inference can credit protection
which was never applied, so the legacy generator is intentionally disabled.

Use ``openpii_gaze_bench.py`` or ``dataiku_en_de_gaze_bench.py``. Both delegate
all scoring to the strict schema-v4 ``gaze_bench_score`` library and consume
producer-authored ``final_protection_trace`` evidence only.

The historical implementation remains available in git history before PR #373.
It must not be restored as an executable scorer.
"""

raise SystemExit(
    "frozen legacy v0.9 benchmark is disabled; use a schema-v4 benchmark adapter"
)
