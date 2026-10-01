#!/usr/bin/env python3
"""How much the test half shares with the validation half the tuning was fitted on.

The split (`comparison_metrics.split_for_id`) holds out document ids only. Both
halves come from the same generators, and the custom recognizers were written
after reading validation-half gold, so a test document can repeat a validation
template, generator group or exact gold value. This script counts that, per
layer, and how much layer A test gold the tuned F2 choice's custom pattern units
cover on their own (no NLP-engine NER, no extra NER). It measures dependence; it
does not say how much of any result is memorization.

    overlap.py --dataset PARQUET --pool POOL

Writes docs/reference/benchmarks/presidio-tuned-overlap.json. Runs in the
comparison environment with PYTHONHASHSEED=0; model-free (replays the recorded
pool).
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import corpus  # noqa: E402
import search  # noqa: E402
import tune  # noqa: E402

OVERLAP = tune.BENCH_DOCS / "presidio-tuned-overlap.json"


def gold_values(document) -> list[str]:
    raw = document.text.encode("utf-8")
    return [raw[span.start:span.end].decode("utf-8") for span in document.spans]


def share(count: int, total: int) -> dict[str, object]:
    return {"count": count, "of": total, "percent": round(100 * count / total, 1) if total else None}


def layer_overlap(documents, records) -> dict[str, object]:
    """Test-half documents that repeat something of the validation half of the same layer."""
    halves = defaultdict(list)
    for document in documents:
        halves[corpus.split_for_id(document.uid)].append(document)
    validation, test = halves["validation"], halves["test"]
    seen_values = {value for document in validation for value in gold_values(document)}
    seen_texts = {document.text for document in validation}
    reused = [any(value in seen_values for value in gold_values(document)) for document in test]
    spans = [value in seen_values for document in test for value in gold_values(document)]
    result = {
        "test_documents": len(test),
        "reuse_a_validation_gold_value": share(sum(reused), len(test)),
        "gold_spans_repeating_a_validation_value": share(sum(spans), len(spans)),
        "identical_to_a_validation_document": share(sum(d.text in seen_texts for d in test), len(test)),
    }
    if records is not None:
        for field in ("template", "group"):
            seen = {getattr(records[d.uid], field) for d in validation}
            result[f"share_a_validation_{field}"] = share(
                sum(getattr(records[d.uid], field) in seen for d in test), len(test))
    return result


def custom_only(config: dict) -> dict:
    """The chosen configuration with everything but its custom pattern units switched off."""
    narrowed = json.loads(json.dumps(config))
    narrowed["artifact_ner"] = "none"
    narrowed["extra"] = {key: "off" for key in narrowed["extra"]}
    narrowed["scope"] = {unit: (scope if unit in search.CUSTOM_UNITS else "off")
                         for unit, scope in narrowed["scope"].items()}
    return narrowed


def pattern_coverage(layers, pool_dir: Path, comparison: dict, selection: dict) -> dict[str, object]:
    """Layer A test gold bytes the F2 choice covers with its custom pattern units alone."""
    test_a = {"A": [d for d in layers["A"] if corpus.split_for_id(d.uid) == "test"]}
    with corpus.measured_agentic_contract(comparison):
        found = search.read_pool(pool_dir, tune.POOL_PASSES, "test")
        docs = search.make_docs(test_a, found, tune.contracts())
    mapping = tune.tuned_mapping()
    replayer = search.Replayer(docs, mapping, corpus.compare.typed_mapping_for_contract(mapping, "v3"))
    choice = selection["choices"]["f2"]
    config = choice["finals"][choice["start"]]["config"]
    nothing = custom_only(config)
    nothing["scope"] = {unit: "off" for unit in nothing["scope"]}

    def leaked(candidate: dict) -> int:
        return sum(replayer.metrics(index, candidate)[0] for index in range(len(docs)))

    gold = leaked(nothing)
    narrowed = custom_only(config)
    units = sorted(unit for unit, scope in narrowed["scope"].items() if scope != "off")
    return {
        "configuration": "tuned F2 choice with NLP-engine NER and extra NER off and only its custom pattern units on",
        "units": units,
        "test_documents": len(docs),
        "scored_gold_bytes": gold,
        "covered_by_custom_patterns_only": share(gold - leaked(narrowed), gold),
        "leaked_by_full_choice": leaked(config),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--pool", type=Path, required=True, help="produce.py pool output directory")
    args = parser.parse_args()
    tune.require_hash_seed()
    comparison = corpus.read_comparison()
    selection = json.loads(tune.SELECTION.read_text(encoding="utf-8"))
    layers = corpus.load_measured(args.dataset, comparison)
    agentic = corpus.compare.agentic
    version = comparison["corpus"]["agentic"]["generator_version"]
    records = {r.uid: r for r in agentic.records_as_of(version, agentic.generate(agentic.PUBLISHED_PARTITION))}
    report = {
        "schema_version": 1,
        "description": "What the test half shares with the validation half; the split holds out document ids only",
        "split": "SHA-256 first byte of the document id < 128 is validation (comparison_metrics.split_for_id)",
        "gold": "gold values are the gold spans' text; layer C before any contract, layers A/D/R under the "
                "measured agentic contract",
        "comparison_sha256": tune.sha256(corpus.COMPARISON),
        "selection_sha256": tune.sha256(tune.SELECTION),
        "pool_test_sha256": {name: tune.sha256(args.pool / f"{name}.test.jsonl") for name in tune.POOL_PASSES},
        "code_sha256": {"overlap.py": tune.sha256(HERE / "overlap.py"), "space.py": tune.sha256(HERE / "space.py")},
        "layers": {layer: layer_overlap(layers[layer], None if layer == "C" else records)
                   for layer in corpus.LAYERS},
        "layer_a_pattern_coverage": pattern_coverage(layers, args.pool, comparison, selection),
    }
    OVERLAP.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report["layers"], indent=1), file=sys.stderr)
    print(json.dumps(report["layer_a_pattern_coverage"], indent=1), file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
