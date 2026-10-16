#!/usr/bin/env python3
"""Pin small native synthetic slices; score A/D/R under v1 and v2 byte views.

This never opens Dataiku/Kiji or any external/sealed per-document corpus.
Coverage-loop samples are native synthetic fixtures, not release layer C.
"""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import sys

from experiment import HERE, ROOT, dump, expansions, mask, validate
sys.path.insert(0, str(ROOT / "scripts/bench"))
import agentic_layers as agentic
import gaze_bench_score as score


def rank(uid):
    return hashlib.sha256(("r2-20261016-" + uid).encode()).digest()


def fixtures(out):
    contract = score.load_scored_label_contract(ROOT / "docs/reference/benchmarks/scored-labels-v2.json")
    ruled = contract.scored_labels | contract.excluded_labels
    native = ROOT / "crates/gaze-recognizers/testdata/coverage-loop/corpus"
    paths = sorted(native.glob("*.labels.json"), key=lambda p: rank(p.name))
    for split, partition in [("dev", "dev"), ("holdout", "test")]:
        rows = []
        records = agentic.generate(partition)
        for layer in ("A", "D", "R"):
            candidates = [r for r in records if r.layer == layer and r.language in {"en", "de"}
                          and len(r.text.encode()) <= 600 and all(g.label in ruled for g in r.gold)]
            # Family round robin avoids selecting only the largest family.
            buckets = defaultdict(list)
            for record in sorted(candidates, key=lambda r: rank(r.uid)):
                buckets[record.family].append(record)
            selected = []
            while len(selected) < 8:
                for family in sorted(buckets, key=rank):
                    if buckets[family] and len(selected) < 8:
                        selected.append(buckets[family].pop(0))
            for record in selected:
                document = record.to_document()
                rows.append(dict(id=document.uid, session=document.uid, text=document.text,
                                 split=split, category=layer, layer=layer,
                                 language=document.language, region=document.region,
                                 gold=[[s.start, s.end] for s in document.spans],
                                 labelled_gold=[[s.start, s.end, s.label] for s in document.spans]))
        # Disjoint 4 dev/4 held-back fixture samples, never used for fitting.
        for path in paths[:4] if split == "dev" else paths[4:8]:
            metadata = json.loads(path.read_text())
            text_path = path.with_name(path.name.replace(".labels.json", ".txt"))
            text = text_path.read_text()
            rows.append(dict(id=metadata["fixture_id"], session=metadata["fixture_id"], text=text,
                             split=split, category="native", layer="native",
                             source_sha256=hashlib.sha256(text_path.read_bytes()).hexdigest(),
                             gold=[[s["byte_start"], s["byte_end"]] for s in metadata["spans"]]))
        dump(out / f"repo-{split}.json", rows)


def canonical(cls):
    return "custom:" + cls["Custom"] if isinstance(cls, dict) else cls.lower()


def rescore(fixture_path, observation_path):
    rows, observations = json.loads(fixture_path.read_text()), json.loads(observation_path.read_text())
    contracts = {"v1": score.SCORED_LABEL_CONTRACT_V1,
                 "v2": score.load_scored_label_contract(ROOT / "docs/reference/benchmarks/scored-labels-v2.json")}
    result = {}
    for version, contract in contracts.items():
        layers = defaultdict(lambda: defaultdict(int))
        for row, response in zip(rows, observations, strict=True):
            if row["layer"] == "native":
                continue
            layer = layers[row["layer"]]
            layer["docs"] += 1
            if "error" in response:
                layer["refusals"] += 1
                continue
            validate(row, response)
            doc = score.Document(uid=row["id"], text=row["text"], language=row["language"],
                                 region=row["region"], source_dataset="gaze-synthetic-r2-slice",
                                 spans=tuple(score.Span(*g) for g in row["labelled_gold"]))
            doc = score.apply_scored_label_contract([doc], contract)[0]
            predictions = [score.Span(e["raw_span"]["start"], e["raw_span"]["end"], canonical(e["class"]))
                           for e in response["manifest"]]
            gold, ignored, predictions = score.contract_scoring_view(doc, predictions)
            g, i, p = mask(gold), mask(ignored), mask((s.start, s.end) for s in predictions)
            layer["leaked"] += len(g - p)
            layer["fp"] += len(p - g - i)
            layer["gold"] += len(g)
            layer["exact_restores"] += 1
            layer["valid_manifests"] += 1
            layer["refusals"] += 0
        result[version] = dict(layers)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["generate", "score"])
    parser.add_argument("path", type=Path)
    parser.add_argument("--observations", type=Path)
    args = parser.parse_args()
    fixtures(args.path) if args.command == "generate" else rescore(args.path, args.observations)
