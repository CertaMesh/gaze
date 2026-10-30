#!/usr/bin/env python3
"""Audit changed issuer rules and probe a real Gaze binary without model inference.

Python locates changed candidate spans; Gaze's Rust regex/pipeline confirms every
reported match. Reports only locations/lengths, never source values. Generated
positives, public-material negatives and restore use the actual binary too.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PACK_PATH = "crates/gaze-recognizers/embedded/secrets.toml"


def rules(text):
    return {rule["id"]: (re.compile(rule["match"]["pattern"].replace(r"\z", r"\Z")),
                          re.compile(rule["context"]["reject_match_regex"].replace(r"\z", r"\Z"))
                          if rule.get("context", {}).get("reject_match_regex") else None)
            for rule in tomllib.loads(text)["recognizers"]
            if rule["id"] in ("security_token.bare", "security_token.pem")}


def spans(spec, text):
    rx, guard = spec
    at = 0
    while match := rx.search(text, at):
        start, end = next(match.span(g) for g in range(1, rx.groups + 1)
                          if match.group(g) is not None)
        if guard is None or not guard.search(match[0]):
            yield start, end
        at = max(end, at + 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True, help="pre-change Git revision")
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--workspace-root", type=Path, action="append", default=[],
                        help="additional source root; excludes build/dependency directories and binary files")
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
        return data

    baseline = subprocess.check_output(["git", "show", args.base + ":" + PACK_PATH], cwd=ROOT, text=True)
    before, after = rules(baseline), rules((ROOT / PACK_PATH).read_text())
    changed = []
    files = 0
    source_files = [("repository", ROOT, ROOT / relative)
                    for relative in subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0")
                    if relative]
    for root in args.workspace_root:
        root = root.resolve()
        command = ["rg", "--files", "--hidden", "--no-ignore", "-0"]
        for directory in (".git", "target", "node_modules", "vendor", ".venv", "venv", ".cache"):
            command += ["--glob", f"!**/{directory}/**"]
        command.append(str(root))
        discovered = subprocess.run(command, capture_output=True, check=True).stdout.decode().split("\0")
        source_files.extend(("workspace", root, Path(name)) for name in discovered if name)
    seen = set()
    for scope, root, path in source_files:
        if path.is_symlink() or not path.is_file() or path.resolve() in seen:
            continue
        seen.add(path.resolve())
        # Source audit, not generated artifacts, databases or binary corpora.
        if path.stat().st_size > 2_000_000:
            continue
        try:
            text = path.read_text()
        except (OSError, UnicodeError):
            continue
        if "\0" in text:
            continue
        files += 1
        for rid, spec in after.items():
            previous = set(spans(before[rid], text)) if rid in before else set()
            for start, end in spans(spec, text):
                if (start, end) not in previous:
                    value = text[start:end]
                    confirmed = clean(value)
                    assert any(e["raw"] == value for e in confirmed["entries"]), "Rust pipeline did not confirm whole new match"
                    changed.append({"scope": scope, "path": str(path.relative_to(root)),
                                    "line": text.count("\n", 0, start) + 1,
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
    positive_result = clean(positive_text)
    tokens = {entry["raw"]: entry["token"] for entry in positive_result["entries"]}
    expected_parts = []
    for record in positives:
        source = record.text.encode()
        for gold in sorted(record.gold, key=lambda span: span.start, reverse=True):
            assert gold.value in tokens, f"missing whole generated gold: {record.uid}"
            source = source[:gold.start] + tokens[gold.value].encode() + source[gold.end:]
        expected_parts.append(source.decode())
    assert positive_result["clean_text"] == separator.join(expected_parts), "source syntax or whole-gold ownership changed"
    negative_text = separator.join(r.text for r in negatives)
    assert clean(negative_text)["clean_text"] == negative_text, "benign generated text changed"
    print(json.dumps({"base": args.base, "files_scanned": files,
                      "new_matches": changed, "positive_documents": len(positives),
                      "positive_gold": sum(len(r.gold) for r in positives),
                      "negative_documents": len(negatives), "exact_restores": len(changed) + 2}, indent=2))


if __name__ == "__main__":
    main()
