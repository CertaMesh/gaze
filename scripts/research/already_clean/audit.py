"""Source-byte accounting, paired near misses and evidence integrity."""

import argparse
import hashlib
import json
import subprocess
from pathlib import Path

import numpy as np


def token_metrics(rows, records):
    total = {"documents": 0, "gold_bytes": 0, "missed_bytes": 0, "false_positive_bytes": 0, "fully_covered_occurrences": 0, "partial_occurrences": 0}
    by_family = {}
    for row, record in zip(rows, records, strict=True):
        assert row["id"] == record["id"]
        if row["evaluation"] != "test":
            continue
        text = row["text"]
        gold = set(range(*row["byte_span"])) if row["label"] else set()
        predicted = set()
        for a, b in record["spans"]:
            assert 0 <= a < b <= len(text)
            predicted.update(range(len(text[:a].encode()), len(text[:b].encode())))
        counts = {"documents": 1, "gold_bytes": len(gold), "missed_bytes": len(gold - predicted), "false_positive_bytes": len(predicted - gold), "fully_covered_occurrences": int(bool(gold) and gold <= predicted), "partial_occurrences": int(bool(gold & predicted) and not gold <= predicted)}
        family = by_family.setdefault(row["family"], dict.fromkeys(total, 0))
        for key, value in counts.items():
            total[key] += value
            family[key] += value
    return {"scope": "raw model token argmax+mass>=0.3, target label families only; no Gaze decoding/rules/neutral view", "total": total, "by_family": by_family}


def run(directory, output):
    rows = [json.loads(line) for line in (directory / "fixtures.jsonl").read_text().splitlines()]
    scores = np.load(directory / "scores.npz")
    lookup = {(r["evaluation"], r["language"], r["family"], r["id"].split('/')[3]): i for i, r in enumerate(rows)}
    result = {"contract": "custom candidate rubric; native token coverage is NOT the setup pipeline", "native_token_coverage": {}, "paired_near_misses": {}, "integrity": {}}
    # Predictions frozen before measurements: person > organisation, private ID > counter,
    # and ambiguous person-name > the same spelling used as a vocabulary item.
    pairs = [("person", "organisation"), ("ambiguous_person", "ordinary_word"), ("private_id", "technical_number")]
    for arm in scores.files:
        counts = {}
        for positive, negative in pairs:
            wins = ties = total = 0
            for language in ["en", "de", "es", "fr"]:
                for variant in map(str, range(8)):
                    a = lookup[("test", language, positive, variant)]
                    b = lookup[("test", language, negative, variant)]
                    assert rows[a]["value"] == rows[b]["value"]
                    total += 1
                    wins += int(scores[arm][a] > scores[arm][b])
                    ties += int(scores[arm][a] == scores[arm][b])
            counts[f"{positive}>{negative}"] = {"wins": wins, "ties": ties, "total": total}
        result["paired_near_misses"][arm] = counts
    fixture_hash = hashlib.sha256((directory / "fixtures.jsonl").read_bytes()).hexdigest()
    for model in ["davlan", "nym", "minilm"]:
        metadata = json.loads((directory / f"{model}-metadata.json").read_text())
        assert metadata["fixture_sha256"] == fixture_hash and metadata["documents"] == len(rows) and metadata["failures"] == 0
        features = np.load(directory / f"{model}-features.npz")
        assert features["ids"].tolist() == [r["id"] for r in rows]
        for key in features.files:
            if key != "ids":
                assert len(features[key]) == len(rows) and np.isfinite(features[key]).all()
        if model != "minilm":
            records = json.loads((directory / f"{model}-tokens.json").read_text())
            result["native_token_coverage"][model] = token_metrics(rows, records)
    # Identical text paired with opposite owner requirements cannot be distinguished
    # by any deterministic text-only arm. This is a structural control, not task success.
    policy_rows = [i for i, r in enumerate(rows) if r["evaluation"] == "test" and r["family"] == "person"]
    result["authority_control"] = {"identical_text_pairs": len(policy_rows), "text_only_max_accuracy": 0.5, "reason": "one protect and one preserve instruction held outside text; every text-only score is identical within pair", "claim": "information boundary only; no real-person existence or policy inference measured"}
    result["integrity"] = {"documents": len(rows), "fixture_sha256": fixture_hash, "finite_features": True, "model_failures": 0, "ids_aligned": True}
    output.write_text(json.dumps(result, indent=2))
    print(json.dumps(result["integrity"]))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    run(args.directory, args.output)
