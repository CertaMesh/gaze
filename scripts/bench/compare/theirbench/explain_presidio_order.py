#!/usr/bin/env python3
"""Verify Presidio type changes caused by equal-score overlap choices, without exporting text."""
from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import backends
import compare
import loaders


def explain(first: Path, second: Path, data: Path, model: Path) -> dict:
    backends.verify_pinned_comparison()
    splits, identity = loaders.load_piibench_commercial(data)
    documents = splits['test']
    a, b = [list(map(json.loads, path.read_text(encoding='utf-8').splitlines())) for path in (first, second)]
    if len(a) != len(documents) or len(b) != len(documents):
        raise ValueError('predictions do not cover the pinned dataset')
    backend = compare.Presidio({'en': model}, threshold=0.0)
    changed_documents = 0
    transitions = Counter()
    for index, (document, left, right) in enumerate(zip(documents, a, b, strict=True)):
        if left['index'] != index or right['index'] != index:
            raise ValueError('predictions are not in dataset order')
        if left == right:
            continue
        changed_documents += 1
        before = {(start, end): label for start, end, label in left['spans']}
        after = {(start, end): label for start, end, label in right['spans']}
        if before.keys() != after.keys():
            raise ValueError('span coordinates changed; type-order evidence does not explain this pair')
        candidates = backend.analyzer.analyze(text=document.text, language='en', score_threshold=0.0)
        scores = {(result.start, result.end, result.entity_type): result.score for result in candidates}
        for (start, end), label in before.items():
            other = after[start, end]
            if label == other:
                continue
            score = scores.get((start, end, label))
            if score is None or score != scores.get((start, end, other)):
                raise ValueError('changed types are not an equal-score pair in the pinned analyzer')
            transitions[label, other, score] += 1
    return {
        'schema_version': 1, 'benchmark': 'piibench-commercial', 'documents': len(documents),
        'dataset_sha256': identity['test_5k_sha256'], 'model': compare.model_info(model, 'en'),
        'prediction_a_sha256': compare.digest_file(first), 'prediction_b_sha256': compare.digest_file(second),
        'changed_documents': changed_documents, 'coordinate_changes': 0,
        'equal_score_type_transitions': [
            {'before': before, 'after': after, 'score': score, 'count': count}
            for (before, after, score), count in sorted(transitions.items())],
        'limit': 'Verifies the supplied captures only. Historical predictions and hash seed were not retained.',
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--first', required=True, type=Path)
    parser.add_argument('--second', required=True, type=Path)
    parser.add_argument('--data', required=True, type=Path)
    parser.add_argument('--en-model', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    args.output.write_text(json.dumps(explain(args.first, args.second, args.data, args.en_model),
                                     indent=2, sort_keys=True) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
