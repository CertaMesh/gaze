"""Replay v30 releases with two harnesses; never recapture or relabel history.

Use an exact, separately prepared baseline checkout. Compare the complete
replayed scorecards for contracts v1-v4, including all aggregate/per-cell
numbers and integrity counts. Retained record SHA-256s bind every result.
"""
import argparse
import ast
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


REPLAY = '''
import hashlib,json,sys
from pathlib import Path
sys.path.insert(0,sys.argv[1])
import gaze_bench_score as s
import scorecard_record as r
p=Path(sys.argv[2]); root=Path(sys.argv[3])
result={}
for version in (1,2,3,4):
    c=s.SCORED_LABEL_CONTRACT_V1 if version==1 else s.load_scored_label_contract(root/f'scored-labels-v{version}.json')
    card=r.rescore(p,c,max_workers=1)
    result[str(version)]=hashlib.sha256(json.dumps(card,sort_keys=True,separators=(',',':')).encode()).hexdigest()
print(json.dumps(result,sort_keys=True))
'''


def prove(baseline_root, root):
    def definitions(path):
        result = {}
        for node in ast.parse(path.read_text()).body:
            if isinstance(node, (ast.FunctionDef, ast.ClassDef)):
                result[node.name] = ast.dump(node, include_attributes=False)
            elif isinstance(node, ast.Assign):
                for target in node.targets:
                    if isinstance(target, ast.Name):
                        result[target.id] = ast.dump(node.value, include_attributes=False)
        return result
    baseline = baseline_root / "scripts/bench/gaze_bench_score.py"
    candidate = root / "scripts/bench/gaze_bench_score.py"
    before, after = definitions(baseline), definitions(candidate)
    changed = sorted(k for k in before.keys() | after.keys() if before.get(k) != after.get(k))
    if changed != ["PIPELINE_FAILURE_REASONS", "_validate_final_protection_trace", "validate_response"]:
        raise ValueError("scorer changes exceed typed response validation")
    output = {"schema": "gaze-refusal-history-proof/v1", "releases": {},
              "baseline_revision": subprocess.check_output(["git", "-C", str(baseline_root), "rev-parse", "HEAD"], text=True).strip(),
              "baseline_scorer_sha256": digest(baseline), "candidate_scorer_sha256": digest(candidate),
              "changed_scorer_definitions": changed}
    for version in ("v0.17.0", "v0.16.0"):
        path = root / "docs/reference/benchmarks/history-generator-v30" / version / "observations-v1.jsonl.gz"
        with gzip.open(path, "rt") as stream:
            header = json.loads(next(stream))
            rows = [json.loads(line) for line in stream]
        if header["scorecard"]["layers"]["generator"]["generator_version"] != 30:
            raise ValueError("historical generator mismatch")
        if any("pipeline_error_code" in row["response"] for row in rows):
            raise ValueError("historical refusals require fresh release measurement")
        results = []
        for harness in (baseline_root, root):
            completed = subprocess.run([sys.executable, "-c", REPLAY, str(harness / "scripts/bench"),
                                       str(path), str(root / "docs/reference/benchmarks")],
                                      check=True, capture_output=True, text=True)
            results.append(json.loads(completed.stdout))
        if results[0] != results[1]:
            raise ValueError("displayed historical scoring changed; release captures required")
        output["releases"][version] = {"record_sha256": digest(path), "observations": len(rows),
                                       "refusals": 0, "contracts": results[0],
                                       "complete_scorecards_identical": True}
    return output


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    value = prove(args.baseline_root.resolve(), root)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    print("Historical v30 scorecards identical under v1-v4; fresh captures unnecessary.")
