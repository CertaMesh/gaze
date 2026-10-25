#!/usr/bin/env python3
"""Publish replayable synthetic MCP evidence with private invocation paths removed."""
from __future__ import annotations
import argparse
import copy
import gzip
import hashlib
import json
from pathlib import Path
import sys
import tomllib

import mcp_operation_order as mcp


def public_capture(original):
    card = copy.deepcopy(original)
    document = card.pop("policy_document")
    if hashlib.sha256(document.encode()).hexdigest() != card["policy_sha256"]:
        raise ValueError("private policy does not match the capture digest")
    data = tomllib.loads(document)
    paths = ((data["ner"]["model_dir"], "<private-model-root>/gaze/models/davlan-mbert-ner-hrl"),
             (data["safety_net"]["nym"]["model_dir"], "<private-model-root>/gaze/models/nym-small-int8"))
    template = document
    expected = copy.deepcopy(data)
    expected["ner"]["model_dir"], expected["safety_net"]["nym"]["model_dir"] = (pair[1] for pair in paths)
    for private, replacement in paths:
        template = template.replace(private, replacement)
    if tomllib.loads(template) != expected:
        raise ValueError("policy publication altered more than model paths")
    card["policy_template"] = template
    card["policy_template_sha256"] = hashlib.sha256(template.encode()).hexdigest()
    # These are invocation metadata, not observations or runtime inputs.
    card["host_build"]["command"] = ["<private-runtime-path>" if isinstance(part, str) and part.startswith("/") else part for part in card["host_build"]["command"]]
    card["public_release"] = {"private_capture_sha256": None,
        "path_redactions": ["absolute build invocation paths", "setup policy model paths"],
        "policy_digest_scope": "policy_sha256 is the actual captured private policy; policy_template is a path-redacted review copy",
        "observations_unchanged": True}
    if card["records"] != original["records"] or card["scores"] != original["scores"]:
        raise ValueError("publication changed observations or scores")
    if "/Users/" in json.dumps(card, ensure_ascii=False):
        raise ValueError("private user path remains in public evidence")
    return card


def write_json(path, value, compressed=False):
    raw = (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode()
    path.write_bytes(gzip.compress(raw, mtime=0) if compressed else raw)


def space_diagnostic(card):
    def leaves(value, path=()):
        if isinstance(value, str):
            yield path, value
        elif isinstance(value, list):
            for i, item in enumerate(value): yield from leaves(item, path + (i,))
        elif isinstance(value, dict):
            for key, item in value.items(): yield from leaves(item, path + (key,))
    total = {"raw_space_bytes": 0, "other_leaked_utf8_bytes": 0, "space_only_cases": []}
    for case, record in zip(mcp.fixture(), card["records"], strict=True):
        if not mcp.score_case(case, record)["restore_exact"]:
            continue
        entries = mcp.entries_from_snapshot(record["snapshot_hex"])
        protected = record["tool_args"] if case["direction"] == "arguments" else record["response"]
        ownership = mcp.masks(case["expanded"], protected, entries)
        gold = {path: [False] * len(mask) for path, mask in ownership.items()}
        for span in case["gold"]:
            start, end = span["span"]
            gold[tuple(span["path"])][start:end] = [True] * (end - start)
        spaces = other = 0
        for path, text in leaves(case["expanded"]):
            for byte, private, owner in zip(text.encode(), gold[path], ownership[path], strict=True):
                if private and owner is None:
                    if byte == 32: spaces += 1
                    else: other += 1
        total["raw_space_bytes"] += spaces; total["other_leaked_utf8_bytes"] += other
        if spaces and not other: total["space_only_cases"].append(case["id"])
    return total


def uncovered_gold_bytes(card):
    """Describe scored raw bytes without changing coverage or gold."""
    rows = []
    for case, record in zip(mcp.fixture(), card["records"], strict=True):
        if not mcp.score_case(case, record)["restore_exact"]:
            continue
        entries = mcp.entries_from_snapshot(record["snapshot_hex"])
        wire = record["tool_args"] if case["direction"] == "arguments" else record["response"]
        ownership = mcp.masks(case["expanded"], wire, entries)
        gold = {path: [False] * len(mask) for path, mask in ownership.items()}
        for span in case["gold"]:
            start, end = span["span"]
            gold[tuple(span["path"])][start:end] = [True] * (end - start)
        for path, mask in ownership.items():
            text = case["expanded"]
            for edge in path:
                text = text[edge]
            raw = text.encode()
            for offset, (private, owner) in enumerate(zip(gold[path], mask, strict=True)):
                if not private or owner is not None:
                    continue
                start, end = offset, offset + 1
                while start > 0 and mask[start - 1] is None:
                    start -= 1
                while end < len(mask) and mask[end] is None:
                    end += 1
                gap = raw[start:end].decode()
                rows.append({"case": case["id"], "path": list(path), "byte_offset": offset,
                             "byte": raw[offset], "gap": gap, "gap_is_whitespace": gap.isspace(),
                             "owned_before": start > 0 and mask[start - 1] is not None,
                             "owned_after": end < len(mask) and mask[end] is not None})
    return rows


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--identity", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    original = [json.loads(path.read_text()) for path in (args.base, args.candidate)]
    mcp.compare(*original)
    public = [public_capture(card) for card in original]
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for side, source, card in zip(("base", "candidate"), (args.base, args.candidate), public, strict=True):
        card["public_release"]["private_capture_sha256"] = mcp.digest(source)
        write_json(args.output_dir / (side + "-native.json.gz"), card, True)
    mcp.compare(*public)
    identity = json.loads(args.identity.read_text())
    write_json(args.output_dir / "text-path-identity.json.gz", identity, True)
    write_json(args.output_dir / "native-comparison.json", mcp.compare(*public))
    write_json(args.output_dir / "space-diagnostic.json", {"scope": "diagnostic only; both frozen contracts unchanged", "base": space_diagnostic(public[0]), "candidate": space_diagnostic(public[1])})
    write_json(args.output_dir / "whitespace-diagnostic.json", {
        "scope": "diagnostic only; gold, oracle, and both scored contracts unchanged",
        "diagnostic": {"base": space_diagnostic(public[0]), "candidate": space_diagnostic(public[1])},
        "candidate_uncovered_gold": uncovered_gold_bytes(public[1])})
    print(json.dumps({"published": True, "observations_unchanged": True, "private_paths_removed": True}))
