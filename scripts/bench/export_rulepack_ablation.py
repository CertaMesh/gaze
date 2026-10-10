#!/usr/bin/env python3
"""Split a committed rulepack into two exact disjoint recognizer subsets.

The base policy uses the remainder; the candidate appends the selected subset.
No recognizer is rewritten. Source bytes and IDs are pinned in the receipt so a
mechanism arm cannot silently substitute hand-authored copies of runtime rules.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import re
import tomllib
from pathlib import Path


def export(source: Path, selected: list[str], output: Path) -> dict:
    raw = source.read_bytes()
    document = tomllib.loads(raw.decode())
    if not selected or len(set(selected)) != len(selected):
        raise ValueError("select a non-empty unique recognizer list")
    blocks = re.split(r"(?m)^\[\[recognizers\]\]\s*$", raw.decode())
    header, chunks = blocks[0], blocks[1:]
    identifiers = [tomllib.loads("[[recognizers]]\n" + chunk)["recognizers"][0]["id"] for chunk in chunks]
    if len(set(identifiers)) != len(identifiers) or not set(selected) <= set(identifiers):
        raise ValueError("source IDs must be unique and contain every selected recognizer")
    output.mkdir(parents=True, exist_ok=True)
    for name, include in (("base-core.toml", False), ("mechanism.toml", True)):
        text = header + "".join("[[recognizers]]\n" + chunk for key, chunk in zip(identifiers, chunks, strict=True) if (key in selected) == include)
        parsed = tomllib.loads(text)
        expected = {**document, "recognizers": [rule for rule in document["recognizers"] if (rule["id"] in selected) == include]}
        if parsed != expected:
            raise ValueError("split changed rulepack metadata or recognizer contents")
        (output / name).write_text(text)
    receipt = {
        "source_sha256": hashlib.sha256(raw).hexdigest(),
        "recognizers": selected,
        "files": {name: hashlib.sha256((output / name).read_bytes()).hexdigest() for name in ("base-core.toml", "mechanism.toml")},
    }
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--recognizer", action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(export(args.source, args.recognizer, args.output), indent=2))


if __name__ == "__main__":
    main()
