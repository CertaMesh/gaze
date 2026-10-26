#!/usr/bin/env python3
"""Compare default release `gaze clean` behavior using hash-pinned repo inputs.

Build each binary in its own prepared Anvil worktree at the recorded SHA:
  anvil run -- env CARGO_BUILD_JOBS=2 nice -n 10 cargo build --release -p gaze-cli
Then run this script from the candidate checkout, supplying both release binaries,
commit SHAs, and --output target/clean-equality.json. It runs no benchmarks.

The input manifest reconstructs the earlier 1,072-case manual review. The earlier
script was not retained; the two whole-file inputs here are explicitly pinned as
the drift corpus and README. Every case uses both binaries and identical stdin in
an empty working directory, without a policy. Only the random session hex in
emitted token strings is normalized. Session blobs contain random owner secrets
and are excluded; clean_text, entries, stats, leak_report and exit codes are
compared. Failure evidence contains case IDs and field names, never input/output.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "scripts/verification/clean-equality-inputs-v1.json"
FIELDS = ("clean_text", "entries", "stats", "leak_report")
TOKEN_PREFIX = re.compile(r"^<([0-9a-f]{8}):")


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def binary_hashes(parser, binaries):
    if binaries[0] == binaries[1]:
        parser.error("supply two independently built binaries")
    hashes = [sha256(binary.read_bytes()) for binary in binaries]
    if hashes[0] == hashes[1]:
        parser.error("identical binary contents: supply two independently built binaries")
    return hashes


def inputs():
    manifest = json.loads(MANIFEST.read_text())
    cases = []
    for source in manifest["sources"]:
        data = (ROOT / source["path"]).read_bytes()
        if sha256(data) != source["sha256"]:
            raise ValueError(f"input hash changed: {source['path']}")
        text = data.decode("utf-8")
        if source["mode"] == "nonempty-lines":
            rows = [(str(i), line) for i, line in enumerate(text.splitlines(), 1) if line.strip()]
        elif source["mode"] == "jsonl-text":
            rows = [(row["id"], row["text"]) for row in map(json.loads, text.splitlines())]
        elif source["mode"] == "whole-file":
            rows = [("whole", text)]
        else:
            raise ValueError("unknown input mode")
        if len(rows) != source["count"]:
            raise ValueError(f"input count changed: {source['path']}")
        cases.extend((f"{source['path']}:{case_id}", value) for case_id, value in rows)
    if len(cases) != manifest["count"] or len({case[0] for case in cases}) != len(cases):
        raise ValueError("input manifest has duplicate IDs or an incorrect total")
    return cases


def normalize(response):
    prefixes = {match[1] for entry in response["entries"]
                if (match := TOKEN_PREFIX.match(entry["token"]))}

    def visit(value):
        if isinstance(value, str):
            for prefix in prefixes:
                value = value.replace(f"<{prefix}:", "<SESSION:")
            return value
        if isinstance(value, list):
            return [visit(item) for item in value]
        if isinstance(value, dict):
            return {key: visit(item) for key, item in value.items()}
        return value

    return {field: visit(response[field]) for field in FIELDS}


def clean(binary, text, cwd):
    env = os.environ.copy()
    env["GAZE_DISABLE_OS_KEYSTORE"] = "1"
    result = subprocess.run([str(binary), "clean"], input=text.encode(),
                            capture_output=True, cwd=cwd, env=env, timeout=60)
    if result.returncode:
        # Refusals must also agree byte for byte; never write their raw bodies.
        return result.returncode, {"failure_stdout": sha256(result.stdout)}
    return result.returncode, normalize(json.loads(result.stdout))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-bin", type=Path, required=True)
    parser.add_argument("--candidate-bin", type=Path, required=True)
    parser.add_argument("--base-sha", required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    for commit in (args.base_sha, args.candidate_sha):
        if not re.fullmatch(r"[0-9a-f]{40}", commit):
            parser.error("supply full commit SHAs")
        if subprocess.run(["git", "cat-file", "-e", f"{commit}^{{commit}}"], cwd=ROOT,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
            parser.error("a supplied commit does not exist in this repository")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if args.candidate_sha != head:
        parser.error("candidate SHA must match this checkout's HEAD")
    if subprocess.run(["git", "diff", "--quiet", "HEAD"], cwd=ROOT).returncode:
        parser.error("candidate checkout has uncommitted changes")
    cases = inputs()
    binaries = (args.base_bin.resolve(strict=True), args.candidate_bin.resolve(strict=True))
    hashes = binary_hashes(parser, binaries)
    differences, exits, detections = [], [dict(), dict()], [0, 0]
    with tempfile.TemporaryDirectory(prefix="gaze-clean-equality-") as cwd:
        for case_id, text in cases:
            observations = [clean(binary, text, cwd) for binary in binaries]
            for side, (code, body) in enumerate(observations):
                exits[side][str(code)] = exits[side].get(str(code), 0) + 1
                detections[side] += body.get("stats", {}).get("detections", 0)
            if observations[0] != observations[1]:
                differences.append({"id": case_id, "exit_codes": [o[0] for o in observations],
                                    "fields": [field for field in set(observations[0][1]) | set(observations[1][1])
                                               if observations[0][1].get(field) != observations[1][1].get(field)]})
    if not all(detections):
        raise RuntimeError("comparison did not exercise any detections")
    report = {"schema": "gaze-clean-equality-v1", "base_sha": args.base_sha,
              "candidate_sha": args.candidate_sha,
              "binary_sha256": hashes,
              "manifest_sha256": sha256(MANIFEST.read_bytes()),
              "script_sha256": sha256(Path(__file__).read_bytes()),
              "cases": len(cases), "exit_codes": exits, "detections": detections,
              "difference_count": len(differences), "differences": differences}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps({key: report[key] for key in ("cases", "difference_count", "exit_codes", "detections")}))
    return int(bool(differences))


if __name__ == "__main__":
    raise SystemExit(main())
