#!/usr/bin/env python3
"""Compare synthetic spelling pairs through two real daemon binaries.

This is an outside-corpus regression probe, not the full-corpus gain gate.
The report contains counts and binary hashes, never input or manifest values.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import itertools
import json
from pathlib import Path
import subprocess
import tempfile


POLICY = '''schema_version = "0.1.0"
[session]
scope = "conversation"
[policy.rulepacks]
bundled = []
[[policy.custom_recognizers]]
kind = "regex"
name = "synthetic_probe_sentinel"
pattern = 'SYNTHETIC_SENTINEL_DO_NOT_MATCH'
class = "custom:tag"
safety_tier = "safe_default"
[[rule]]
kind = "default"
action = "tokenize"
'''


class Daemon:
    def __init__(self, binary: Path, policy: Path):
        self.process = subprocess.Popen(
            [str(binary.resolve()), "daemon", "--policy", str(policy)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            text=True,
        )

    def exchange(self, request: dict) -> dict:
        assert self.process.stdin and self.process.stdout
        self.process.stdin.write(json.dumps(request, ensure_ascii=False) + "\n")
        self.process.stdin.flush()
        response = self.process.stdout.readline()
        if not response:
            raise RuntimeError("daemon exited before responding")
        return json.loads(response)

    def close(self):
        self.process.terminate()
        self.process.wait(timeout=10)


def spellings(value: str) -> list[str]:
    """All half/fullwidth masks over the first five characters, with/without ZWSP."""
    return [
        "".join(
            (chr(ord(ch) + 0xFEE0) if index < 5 and mask & (1 << index) else ch)
            + ("\u200b" if invisible and index == 2 else "")
            for index, ch in enumerate(value)
        )
        for invisible in (False, True)
        for mask in range(32)
    ]


def contexts(value: str):
    yield "dictionary", {"dictionaries": {"tag": {"terms": [value], "case_sensitive": True}},
                         "class_map": {"tag": "custom:tag"}}
    yield "record", {"record": {"tag": value}, "field_map": {"/tag": "custom:tag"},
                     "record_match_kinds": {"custom:tag": ["exact"]}}


def cases():
    for term, text in itertools.product(spellings("Qx7Lm2Rt"), repeat=2):
        for path, context in contexts(term):
            yield path, context, text, True
    # Preserved combining marks, fold expansion, every mapped space and removed range.
    ignored = [0xAD, 0x34F, 0x61C, 0x115F, 0x17B4, 0x180B, 0x200B, 0x202A,
               0x2060, 0x3164, 0xFE0F, 0xFEFF, 0xFFA0, 0xFFF0, 0x1BCA0, 0x1D173, 0xE0067]
    for char in map(chr, ignored):
        for term, text in [(f"Qx{char}7Lm2Rt", "Qx7Lm2Rt"), (f"Qx{char}7Lm2Rt", f"Qx{char}7Lm2Rt")]:
            for path, context in contexts(term):
                yield path, context, text, True
    for value in ["Qe\u03017Lm2Rt", "Qx7\u2011Lm2Rt", "Qx7\u2007Lm2Rt"]:
        for path, context in contexts(value):
            yield path, context, value, True
    # Precision: unrelated values, embedded identifiers, and significant combining marks.
    for term in spellings("Qx7Lm2Rt"):
        for path, context in contexts(term):
            for text in ["Qx8Lm2Rt", "AQx7Lm2Rt", "Qx7Lm2RtA", "Qx7Lm2R\u0301t"]:
                yield path + "_negative", context, text, False
    for value in ["Qorvex Ryntel", "Ｑorvex Ryntel", "Qorvex\u200b Ryntel", "Qorveß Ryntel"]:
        context = {"record": {"name": value}}
        for text in [value, value.lower(), value.upper()]:
            yield "name_multi", context, text, True
    # Preserve the intentional exclusive-kind policy for single names.
    for text, protected in [("Qorvex", False), ("qorvex", True), ("QORVEX", True)]:
        yield "name_single_policy", {"record": {"name": "Qorvex"}}, text, protected


def check(binary: Path, policy: Path) -> dict[int, bool]:
    daemon = Daemon(binary, policy)
    results = {}
    try:
        for index, (path, context, text, expected) in enumerate(cases()):
            session = "normalization-probe"
            raw = f"found {text} here"
            response = daemon.exchange({"session_id": session, "text": raw, "context": context})
            if "error" in response:
                raise AssertionError(f"unexpected refusal in {path} case {index}")
            clean = response["clean_text"]
            protected = clean.startswith("found <") and clean.endswith("> here")
            results[index] = protected
            if protected:
                restored = daemon.exchange({"version": 1, "op": "restore", "session_id": session, "text": clean})
                if restored.get("text") != raw:
                    raise AssertionError(f"inexact restore in {path} case {index}")
            # Close the legacy session to prevent the repeat sweep hiding a miss.
            daemon.exchange({"version": 1, "op": "close", "session_id": session})
    finally:
        daemon.close()
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="gaze-normalization-") as directory:
        policy = Path(directory) / "policy.toml"
        policy.write_text(POLICY)
        base = check(args.base, policy)
        candidate = check(args.candidate, policy)
    counts = Counter()
    for index, (path, context, text, expected) in enumerate(cases()):
        counts[f"{path}.cases"] += 1
        counts["old_protected"] += base[index]
        counts["new_protected"] += candidate[index]
        counts["recall_regressions"] += expected and base[index] and not candidate[index]
        counts["new_misses"] += expected and not candidate[index]
        counts["new_false_positives"] += not expected and candidate[index]
        counts["improved"] += expected and candidate[index] and not base[index]
    report = {
        "schema_version": 1,
        "base_binary_sha256": hashlib.sha256(args.base.read_bytes()).hexdigest(),
        "candidate_binary_sha256": hashlib.sha256(args.candidate.read_bytes()).hexdigest(),
        "counts": dict(sorted(counts.items())),
        "all_protected_cases_restore_exactly": True,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
    assert counts["improved"] > 0
    assert not counts["recall_regressions"]
    assert not counts["new_misses"]
    assert not counts["new_false_positives"]


if __name__ == "__main__":
    main()
