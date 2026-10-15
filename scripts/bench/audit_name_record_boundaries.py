#!/usr/bin/env python3
"""Enumerate record separators inside measured Name spans without exposing values.

Read-only: reconstruct the pinned corpus, verify its identity against each
observation record, and inspect byte ranges from every final Name trace item.
No inference, score arithmetic, or policy changes are involved.
"""
from __future__ import annotations

import argparse
from collections import Counter
import gzip
import hashlib
import json
from pathlib import Path

import agentic_layers as agentic
import dataiku_en_de_gaze_bench as dataiku
import run_no_opf_benchmark as runner

SEPARATORS = frozenset("\n\r\u000b\u000c\u0085\u2028\u2029")


def audit(path: Path, documents: dict, corpus_sha256: str, generated_sha256: str) -> dict:
    counts: Counter = Counter()
    crossings = []
    seen = set()
    with gzip.open(path, "rt", encoding="utf-8") as stream:
        header = json.loads(next(stream))
        assert header["corpus_sha256"] == corpus_sha256, "primary corpus changed"
        assert header["layer_identity"]["generator_corpus_sha256"] == generated_sha256, "generated corpus changed"
        metadata = {row["id"]: row for row in header["documents"]}
        for line in stream:
            row = json.loads(line)
            document_id = row["document_id"]
            key = (row["layer"], row["config"], document_id)
            assert key not in seen, "duplicate observation"
            seen.add(key)
            raw = documents[document_id].text.encode("utf-8")
            assert len(raw) == metadata[document_id]["utf8_bytes"], "document length changed"
            counts["documents"] += 1
            for item in row["response"].get("final_protection_trace", []):
                if item["class"] != "name":
                    continue
                counts["name_spans"] += 1
                provenance = item["provenance"]
                is_nym = "nym-small-int8" in provenance["source_ids"]
                counts["nym_name_spans"] += int(is_nym)
                start, end = item["raw_start"], item["raw_end"]
                assert 0 <= start < end <= len(raw), "invalid Name range"
                value = raw[start:end].decode("utf-8")
                separators = sorted({f"U+{ord(ch):04X}" for ch in value if ch in SEPARATORS})
                if separators:
                    crossings.append({"layer": row["layer"], "document_id": document_id,
                                      "raw_span": [start, end], "stage": provenance["stage"],
                                      "source_ids": provenance["source_ids"], "nym": is_nym,
                                      "separators": separators})
        assert {key[2] for key in seen} == set(metadata), "missing documents"
    return {"record_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "documents": counts["documents"], "name_spans": counts["name_spans"],
            "nym_name_spans": counts["nym_name_spans"], "crossing_name_spans": len(crossings),
            "crossing_nym_name_spans": sum(row["nym"] for row in crossings),
            "crossings": crossings}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, default=Path("target/bench-data/dataiku-en-de/test.parquet"))
    parser.add_argument("--record", action="append", required=True, help="Public arm label=record path")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    dataiku.verify_dataset(args.dataset)
    positive, positive_report = dataiku.load_documents(args.dataset)
    negative, negative_report = runner.load_negative_documents(root / runner.NEGATIVE_CORPUS)
    _, report = runner.composite_dataset_report(positive_report, negative_report)
    generated = agentic.prepare(root)
    all_documents = positive + negative + generated.identifiers + generated.lookalikes + generated.repeats
    documents = {doc.uid: doc for doc in all_documents}
    assert len(documents) == len(all_documents), "duplicate corpus document IDs"
    result = {"primary_corpus_sha256": report["integrity"]["sha256"],
              "generated_corpus_sha256": generated.manifest["corpus_sha256"], "arms": {}}
    for specification in args.record:
        label, path = specification.split("=", 1)
        assert label not in result["arms"], "duplicate arm label"
        result["arms"][label] = audit(Path(path), documents, result["primary_corpus_sha256"],
                                     result["generated_corpus_sha256"])
    result["crossing_name_spans"] = sum(row["crossing_name_spans"] for row in result["arms"].values())
    result["crossing_nym_name_spans"] = sum(row["crossing_nym_name_spans"] for row in result["arms"].values())
    unique = {}
    crossing_sets = []
    for label, arm in result["arms"].items():
        keys = []
        for crossing in arm.pop("crossings"):
            key = json.dumps(crossing, sort_keys=True, separators=(",", ":"))
            keys.append(key)
            unique.setdefault(key, {**crossing, "arms": []})["arms"].append(label)
        crossing_sets.append(set(keys))
    result["crossings_identical_across_arms"] = all(keys == crossing_sets[0] for keys in crossing_sets)
    result["unique_crossings"] = list(unique.values())
    result["audit_script_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({label: {key: value for key, value in row.items() if key != "crossings"}
                      for label, row in result["arms"].items()}, indent=2))


if __name__ == "__main__":
    main()
