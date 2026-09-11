#!/usr/bin/env python3
"""Re-derive ``real-scorecard-v4.json`` from a full harness scorecard.

The renderer's contract tests need a scorecard whose *shape* is production's,
not a hand-written approximation — a hand-written ``integrity`` block is what
let the corpus digest render as ``n/a`` while 28 tests stayed green. A full
scorecard is ~547 KB and this PR exists to stop committing those, so the
fixture keeps every field the renderer reads verbatim and drops only the bulky
per-label/per-language histograms, which the renderer never touches.

Re-derive after any scorecard schema change::

    git show v0.13.0:docs/reference/benchmarks/v0.12-3025a-cfb3aed-candidate-scorecard-v4.json \
      | python3 scripts/bench/fixtures/make_real_scorecard_fixture.py \
      > scripts/bench/fixtures/real-scorecard-v4.json

Source of the committed fixture:
``v0.13.0:docs/reference/benchmarks/v0.12-3025a-cfb3aed-candidate-scorecard-v4.json``
(gaze revision ``cfb3aed79e5625cc94f4423bfd04283401327d8e``).
"""

from __future__ import annotations

import json
import sys

#: Dropped because the renderer reads none of them and together they are 98% of
#: the bytes. Every *other* key is passed through untouched.
DROP_FROM_RUN = (
    "scored_population",
    "per_language",
    "per_label_recall",
    "validator_recall_by_label",
    "per_negative_category",
    "direct_identifier_recall",
    "contextual_pii_recall",
)
DROP_FROM_DATASET = ("components", "sampling", "validator_gold_census")
DROP_FROM_POPULATION = ("labels", "regions")


def trim(scorecard: dict) -> dict:
    trimmed = dict(scorecard)

    dataset = {
        key: value
        for key, value in trimmed["dataset"].items()
        if key not in DROP_FROM_DATASET
    }
    population = dataset.get("evaluated_population")
    if isinstance(population, dict):
        dataset["evaluated_population"] = {
            key: value
            for key, value in population.items()
            if key not in DROP_FROM_POPULATION
        }
    trimmed["dataset"] = dataset

    trimmed["runs"] = [
        {key: value for key, value in run.items() if key not in DROP_FROM_RUN}
        for run in trimmed["runs"]
    ]
    return trimmed


def main() -> int:
    json.dump(trim(json.load(sys.stdin)), sys.stdout, indent=2, sort_keys=False)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
