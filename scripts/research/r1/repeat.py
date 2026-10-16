#!/usr/bin/env python3
"""Audit deterministic spread and actual outputs; never measure elapsed time.

Interleave dev/test, capturing five complete trials per split by default.
Random session token bytes are expected to change; scored counts must not.
This is evidence replication, not a new corpus or an algorithm search.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import subprocess
import sys

from model import simulate
from run import HERE, REPO, native, layer_rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--output-dir", type=Path, default=REPO/"target/research/r1/repeats")
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    collected = {split: [] for split in ("dev","test")}
    for i in range(args.repetitions):
        for split in collected:
            path = args.output_dir/f"{split}-{i}.json"
            subprocess.run([sys.executable,str(HERE/"run.py"),"--split",split,"--output",str(path)],check=True,stdout=subprocess.DEVNULL)
            collected[split].append(json.loads(path.read_text()))
    spread = {}
    for split, runs in collected.items():
        spread[split] = {}
        for arm in runs[0]["totals"]:
            spread[split][arm] = {metric:{"min":min(r["totals"][arm].get(metric,0) for r in runs),
                                        "max":max(r["totals"][arm].get(metric,0) for r in runs)}
                for metric in ("missed_pii_bytes","false_positive_bytes","correct_links","wrong_links","restoration_errors","turns","task_points")}
    rows = [json.loads(line) for line in (HERE/"scenarios.jsonl").read_text().splitlines()]
    observed = []
    binary = REPO/"target/debug/examples/research_r1"
    # Show source, protected output, root choices and restored-source equality.
    # Inspect dev examples only; no algorithm changes depend on held-out output.
    for family in ("given","surname_collision","identical_names","typo","road_abbreviation"):
        row = next(r for r in rows if r["split"] == "dev" and r["language"] == "de" and r["family"] == family)
        n = native([row],binary)[0]
        variants = {}
        for arm in ("exact","structured","evidence","scoped_parts","canonical_restore"):
            p = simulate(row,arm)
            variants[arm] = [{"source":row["turns"][i]["text"], "restore_ok":p["turns"][i]["restore_ok"],
                              "links":[{"surface":row["turns"][i]["text"].encode()[s["start"]:s["end"]].decode(),"root":s["root"]} for s in p["turns"][i]["spans"]]}
                             for i in (1,2,3,4)]
        observed.append({"family":family,"entities":row["entities"],"variants":variants,"native":n["turns"]})
    residuals = Counter()
    aux = layer_rows("test")
    for row, pred in zip(aux,native(aux,binary)):
        raw, p = row["turns"][1], pred["turns"][1]
        covered = {i for s in p["spans"] for i in range(s["start"],s["end"])}
        residuals[row["family"]] += sum(len(set(range(g["start"],g["end"]))-covered) for g in raw["gold"])
    results = {"repetitions":args.repetitions,"timing_measured":False,"spread":spread,
               "primary_work_per_trial":{"conversations_per_split":64,"turns_per_arm_per_split":384,"link_targets_per_arm_per_split":256},
               "observed_dev_outputs":observed,"auxiliary_native_missed_bytes_by_family":dict(residuals),
               "claim_limit":"No LLM planner trials, production timing, or population inference. Zero spread checks determinism, not generalization."}
    path = args.output_dir/"audit.json"
    path.write_text(json.dumps(results,ensure_ascii=False,sort_keys=True,indent=2)+"\n")
    print(json.dumps({"audit":str(path),"repetitions":args.repetitions,"timing_measured":False}))


if __name__ == "__main__": main()
