#!/usr/bin/env python3
"""Audit changed issuer rules and probe a real Gaze binary without model inference.

Python locates changed candidate spans; Gaze's Rust regex/pipeline confirms every
reported match. Reports only locations/lengths, never source values. Generated
positives, public-material negatives and restore use the actual binary too.
"""
import argparse
import collections
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PACK_PATH = "crates/gaze-recognizers/embedded/secrets.toml"


def rules(text):
    return {rule["id"]: re.compile(rule["match"]["pattern"].replace(r"\z", r"\Z"))
            for rule in tomllib.loads(text)["recognizers"]
            if rule["id"] in ("security_token.bare", "security_token.pem")}


def spans(rx, text):
    at = 0
    while match := rx.search(text, at):
        start, end = next(match.span(g) for g in range(1, rx.groups + 1)
                          if match.group(g) is not None)
        yield start, end
        at = max(end, at + 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True, help="pre-change Git revision")
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    env = {**os.environ, "GAZE_DISABLE_OS_KEYSTORE": "1"}

    def clean(raw):
        result = subprocess.run([binary, "clean", "--rulepack-bundled", "none",
                                 "--rulepack-path", str(ROOT / PACK_PATH), "--locale", "global",
                                 "--safety-net", "none"], input=raw, text=True,
                                capture_output=True, env=env, check=True)
        data = json.loads(result.stdout)
        restored = subprocess.run([binary, "restore"], input=json.dumps(
            {"session_blob": data["session_blob"], "text": data["clean_text"]}),
            text=True, capture_output=True, env=env, check=True)
        assert json.loads(restored.stdout)["text"] == raw, "exact restore"
        return data["clean_text"]

    baseline = subprocess.check_output(["git", "show", args.base + ":" + PACK_PATH], cwd=ROOT, text=True)
    before, after = rules(baseline), rules((ROOT / PACK_PATH).read_text())
    changed = []
    files = 0
    for relative in subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0"):
        path = ROOT / relative
        if not relative or not path.is_file():
            continue
        try:
            text = path.read_text()
        except (OSError, UnicodeError):
            continue
        files += 1
        for rid, rx in after.items():
            previous = set(spans(before[rid], text)) if rid in before else set()
            for start, end in spans(rx, text):
                if (start, end) not in previous:
                    value = text[start:end]
                    assert value not in clean(value), "Rust pipeline did not confirm new match"
                    changed.append({"path": relative, "line": text.count("\n", 0, start) + 1,
                                    "rule": rid, "bytes": len(value.encode())})
    sys.path.insert(0, str(ROOT / "scripts/bench"))
    import agentic_layers as api
    import issuer_credential_cells as cells
    records = cells.records(api, "test")
    positives = [r for r in records if r.gold]
    negatives = [r for r in records if not r.gold]
    # One real pipeline call per side preserves syntax and permits exact negative comparison.
    separator = "\n!DOCUMENT!\n"
    positive_text = separator.join(r.text for r in positives)
    positive_clean = clean(positive_text)
    for record in positives:
        for gold in record.gold:
            assert gold.value not in positive_clean, f"exposed generated gold: {record.uid}"
    negative_text = separator.join(r.text for r in negatives)
    assert clean(negative_text) == negative_text, "benign generated text changed"
    print(json.dumps({"base": args.base, "files_scanned": files,
                      "new_matches": changed, "positive_documents": len(positives),
                      "positive_gold": sum(len(r.gold) for r in positives),
                      "negative_documents": len(negatives), "exact_restores": len(changed) + 2}, indent=2))


if __name__ == "__main__":
    main()
