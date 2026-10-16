#!/usr/bin/env python3
"""Five alternating raw/view checks on a small fixed diagnostic slice; no clocks."""
import argparse
from collections import defaultdict
import json
import os
from pathlib import Path
import subprocess
from experiment import HERE, dump, mask, normalized_view, source_response, validate

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", required=True)
parser.add_argument("--policy", required=True)
parser.add_argument("--out", required=True)
args = parser.parse_args()
categories = ("email_zero_width", "email_fullwidth", "name_lower", "name_cue",
              "cue_hard_negative", "record_separators")
chosen = []
for split in ("dev", "holdout"):
    rows = json.loads((HERE / f"{split}.json").read_text())
    chosen.extend(next(row for row in rows if row["category"] == category) for category in categories)
jobs = []
for trial in range(5):
    for arm in (("raw", "normalized") if trial % 2 == 0 else ("normalized", "raw")):
        for row in chosen:
            text, mapping = normalized_view(row["text"]) if arm == "normalized" else (row["text"], None)
            sid = f"repeat-{trial}-{arm}-{row['id']}"
            jobs.append((trial, arm, row, mapping, {"session_id": sid, "text": text}))
env = dict(os.environ, OMP_NUM_THREADS="1", OPENBLAS_NUM_THREADS="1", RAYON_NUM_THREADS="2")
output = subprocess.run([args.binary, args.policy],
                        input="".join(json.dumps(j[-1]) + "\n" for j in jobs),
                        capture_output=True, text=True, timeout=900, env=env)
if output.returncode:
    raise RuntimeError(output.stderr[-3000:])
observations = [json.loads(line) for line in output.stdout.split("\n") if line]
assert len(observations) == len(jobs)
trials = defaultdict(lambda: defaultdict(lambda: defaultdict(lambda: defaultdict(int))))
by_document = defaultdict(list)
for (trial, arm, row, mapping, request), response in zip(jobs, observations, strict=True):
    assert response["session_id"] == request["session_id"]
    bucket = trials[row["split"]][arm][trial]
    bucket["docs"] += 1
    if "error" in response:
        bucket["refusals"] += 1
        continue
    if mapping is not None:
        response = source_response(row, dict(row, text=request["text"]), response, mapping)
    spans = validate(row, response)
    g, p = mask(row["gold"]), mask(spans)
    bucket["leaked"] += len(g - p)
    bucket["fp"] += len(p - g)
    bucket["exact_restores"] += 1
    bucket["refusals"] += 0
    by_document[(arm, row["id"])].append(spans)
summary = {}
for split, arms in trials.items():
    summary[split] = {}
    for arm, counts in arms.items():
        values = list(counts.values())
        summary[split][arm] = {"trials": values, "run_count": 5,
                              "leaked_range": [min(x["leaked"] for x in values), max(x["leaked"] for x in values)],
                              "fp_range": [min(x["fp"] for x in values), max(x["fp"] for x in values)]}
summary["coverage_varied_documents"] = sum(any(s != spans[0] for s in spans) for spans in by_document.values())
summary["requested_responses"] = len(jobs)
summary["received_responses"] = len(observations)
summary["categories"] = categories
dump(Path(args.out), summary)
print(json.dumps(summary, indent=2))
