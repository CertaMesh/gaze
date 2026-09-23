#!/usr/bin/env python3
"""Every-Action policy matrix for collision-family token actions (todo #3746).

Scores what a stricter derived family action does to the bytes that leave the
process, for EVERY value of `Action` on every axis the derivation reads:

    custom:iban        in {tokenize, redact, generalize, format_preserve, preserve, unset}
  x custom:credit_card in the same six
  x default rule       in {tokenize, redact, preserve}
  x family rule        in {none, preserve, tokenize}
  x locale             in {de-DE (core + locale-de), en-US (core + locale-en)}
= 648 arms, base binary vs head binary, on the subset of the #3708 IBAN
document set where a family token can occur plus a seeded 1% control sample.

`custom:phone` and `custom:postal_code` are held at `tokenize` in every arm:
they are not members of the payment family, so they cannot change the
derivation, but they keep the phone-sub-run / residual-cell path live. That
path is where review 3746 found the regression the 10-arm matrix
(`family_action_policy_matrix.py`) could not see: it had no `redact` arm, and
a family token deriving `redact` switched residual coverage off, so the
losing IBAN's bytes beside a phone win shipped raw (872 documents).

Oracle: manifest arithmetic CANNOT score this matrix. `redact` and
`generalize` are one-way and an action that ranks stricter can read as a
"loss" of protected manifest bytes while the bytes are gone from the output.
`leaked` therefore counts the IBAN character positions still covered by a
surviving 4-character window of the IBAN in `clean_text` after every
replacement shape (`<hex:Class_N>`, `hex:class_N`, `[REDACTED]`, `[CLASS]`) is
blanked. Action-agnostic; a constant prefix/trailer false positive cancels
because every comparison is per document between the two binaries.

Invariant: `head_leaked <= base_leaked` for EVERY document in EVERY arm
(`regressed_docs == 0`), no missing daemon response, binaries differ. Any
violation exits 1. Every document whose IBAN view or leak changed between the
binaries is written, with its arm and transition, to the changed-docs JSONL
(gzip); every regressed document is also in the report in full.

Usage:
    python3 scripts/bench/family_action_full_matrix.py BASE_BIN HEAD_BIN OUT.json \
        [--subset SUBSET.json] [--changed CHANGED.jsonl.gz] [--workers N]

`--subset` caches the document selection (computed with BASE_BIN under a
members-and-default-tokenize policy when the file does not exist). Run against
immutable copies of the binaries; both SHA-256 digests are recorded.
"""

from __future__ import annotations

import argparse
import gzip
import json
import random
import re
import sys
import tempfile
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import iban_trailing_word_enumeration as base_enum  # noqa: E402

FAMILY_CLASS = "custom:family:payment-card-or-iban"
FAMILY_TOKEN_CLASS = "family:payment-card-or-iban"
MEMBER_ACTIONS = ("tokenize", "redact", "generalize", "format_preserve", "preserve", "unset")
DEFAULTS = ("tokenize", "redact", "preserve")
FAMILY_RULES = ("none", "preserve", "tokenize")
BACKGROUND = (("custom:phone", "tokenize"), ("custom:postal_code", "tokenize"))
LOCALES = {
    "de-de": ('"core", "locale-de"', "de-DE"),
    "en-us": ('"core", "locale-en"', "en-US"),
}
SUBSET_SEED = 37460923
CONTROL_RATE = 0.01
NGRAM = 4

RULE = """
[[rule]]
kind = "class"
class = "{cls}"
action = "{action}"
"""
DEFAULT = """
[[rule]]
kind = "default"
action = "{action}"
"""
# tokenize `<hex:Class_N>`, format_preserve `hex:class_N`, redact `[REDACTED]`,
# generalize `[CLASS]` (and the safety-net `[REDACTED:class]` marker shape).
REPLACEMENT = re.compile(
    r"<[0-9a-f]{8}:[^>]+>|\b[0-9a-f]{8}:[a-z][a-z0-9_:.\-]*_[0-9]+|\[[A-Z][A-Za-z0-9_:.\-]*\]"
)


def policy_text(bundled: str, locale: str, iban: str, card: str, default: str, family: str) -> str:
    text = base_enum.POLICY_HEAD.format(bundled=bundled, locale=locale)
    if family != "none":
        text += RULE.format(cls=FAMILY_CLASS, action=family)
    if iban != "unset":
        text += RULE.format(cls="custom:iban", action=iban)
    if card != "unset":
        text += RULE.format(cls="custom:credit_card", action=card)
    for cls, action in BACKGROUND:
        text += RULE.format(cls=cls, action=action)
    return text + DEFAULT.format(action=default)


def leaked(response: dict | None, doc: dict) -> int:
    """IBAN character positions still readable in the bytes that left the process."""
    if response is None or response.get("clean_text") is None:
        return doc["iban_span"][1] - doc["iban_span"][0]
    stripped = REPLACEMENT.sub("\x00", response["clean_text"])
    start, end = doc["iban_span"]
    iban = doc["text"].encode()[start:end].decode()
    if len(iban) <= NGRAM:
        return len(iban) if iban in stripped else 0
    hit = bytearray(len(iban))
    for i in range(len(iban) - NGRAM + 1):
        if iban[i : i + NGRAM] in stripped:
            for j in range(i, i + NGRAM):
                hit[j] = 1
    return sum(hit)


def view(response: dict | None, doc: dict) -> tuple[str, int]:
    """(classes covering the IBAN span per the manifest, or "raw"; leaked)."""
    cls = "missing" if response is None else base_enum.iban_view(response, doc["iban_span"])[0]
    return cls, leaked(response, doc)


def select_subset(base_bin: str, docs: list[dict], tmp: Path) -> dict:
    """Documents whose IBAN span is (partly) a family token under a members +
    default tokenize policy on the base binary, plus a seeded 1% control."""
    subsets = {}
    for loc, (bundled, locale) in LOCALES.items():
        policy = tmp / f"select-{loc}.toml"
        policy.write_text(policy_text(bundled, locale, "tokenize", "tokenize", "tokenize", "none"))
        responses = base_enum.run(base_bin, policy, docs)
        family = sorted(
            i
            for i, (doc, response) in enumerate(zip(docs, responses))
            if response is None
            or FAMILY_TOKEN_CLASS in base_enum.iban_view(response, doc["iban_span"])[0].split("+")
        )
        rng = random.Random(SUBSET_SEED)
        rest = [i for i in range(len(docs)) if i not in set(family)]
        control = sorted(rng.sample(rest, max(1, round(len(rest) * CONTROL_RATE))))
        subsets[loc] = {"family": family, "control": control}
    return {
        "seed": SUBSET_SEED,
        "control_rate": CONTROL_RATE,
        "documents": len(docs),
        "base_binary": base_enum.binary_identity(base_bin),
        "subsets": subsets,
    }


def arms():
    for loc in LOCALES:
        for iban in MEMBER_ACTIONS:
            for card in MEMBER_ACTIONS:
                for default in DEFAULTS:
                    for family in FAMILY_RULES:
                        yield loc, iban, card, default, family


def score_arm(base_bin, head_bin, docs, tmp, loc, iban, card, default, family):
    bundled, locale = LOCALES[loc]
    arm = f"iban={iban}/card={card}/default={default}/family={family}/{loc}"
    policy = tmp / (arm.replace("/", "_") + ".toml")
    policy.write_text(policy_text(bundled, locale, iban, card, default, family))
    base = base_enum.run(base_bin, policy, docs)
    head = base_enum.run(head_bin, policy, docs)
    stats = Counter(documents=len(docs))
    transitions = Counter()
    changed, regressed = [], []
    for index, (doc, b, h) in enumerate(zip(docs, base, head)):
        if b is None or h is None:
            stats["missing_response"] += 1
        b_cls, b_leak = view(b, doc)
        h_cls, h_leak = view(h, doc)
        stats["base_leaked"] += b_leak
        stats["head_leaked"] += h_leak
        if (b_cls, b_leak) == (h_cls, h_leak):
            continue
        if h_leak > b_leak:
            kind = "regressed"
        elif h_leak < b_leak:
            kind = "improved"
        else:
            kind = "relabelled"
        stats[f"{kind}_docs"] += 1
        reason = f"{b_cls}:{b_leak} -> {h_cls}:{h_leak}"
        transitions[reason] += 1
        row = {
            "arm": arm,
            "doc": index,
            "kind": kind,
            "reason": reason,
            "text": doc["text"],
            "base_view": [b_cls, b_leak],
            "head_view": [h_cls, h_leak],
        }
        changed.append(row)
        if kind == "regressed":
            regressed.append(
                dict(row, base_clean=b and b.get("clean_text"), head_clean=h and h.get("clean_text"))
            )
    failed = stats["regressed_docs"] > 0 or stats["missing_response"] > 0
    return arm, {
        "policy": {"iban": iban, "credit_card": card, "default": default, "family": family, "locale": loc},
        "stats": dict(stats),
        "transitions": dict(transitions),
        "regressed": regressed,
        "failed": failed,
    }, changed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("base_bin")
    parser.add_argument("head_bin")
    parser.add_argument("out")
    parser.add_argument("--subset", help="cache of the document selection")
    parser.add_argument("--changed", help="gzip JSONL of every changed document per arm")
    parser.add_argument("--workers", type=int, default=3)
    args = parser.parse_args()
    out = Path(args.out)
    changed_path = Path(args.changed or out.with_suffix(".changed.jsonl.gz"))

    base_id = base_enum.binary_identity(args.base_bin)
    head_id = base_enum.binary_identity(args.head_bin)
    if base_id["sha256"] == head_id["sha256"]:
        print("base and head binaries are identical; nothing to compare", file=sys.stderr)
        return 1
    all_docs = base_enum.documents()
    started = time.time()
    with tempfile.TemporaryDirectory() as tmp_dir:
        tmp = Path(tmp_dir)
        subset_path = Path(args.subset) if args.subset else None
        if subset_path and subset_path.exists():
            subset = json.loads(subset_path.read_text())
        else:
            subset = select_subset(args.base_bin, all_docs, tmp)
            if subset_path:
                subset_path.write_text(json.dumps(subset))
        docs_by_locale = {
            loc: [all_docs[i] for i in subset["subsets"][loc]["family"] + subset["subsets"][loc]["control"]]
            for loc in LOCALES
        }
        for loc in LOCALES:
            s = subset["subsets"][loc]
            print(f"### {loc}: {len(s['family'])} family + {len(s['control'])} control documents", flush=True)

        report = {
            "documents": len(all_docs),
            "seed": base_enum.SEED,
            "subset": {k: v for k, v in subset.items() if k != "subsets"},
            "subset_sizes": {loc: len(docs) for loc, docs in docs_by_locale.items()},
            "binaries": {"base": base_id, "head": head_id},
            "axes": {
                "member_actions": MEMBER_ACTIONS,
                "defaults": DEFAULTS,
                "family_rules": FAMILY_RULES,
                "background": BACKGROUND,
                "locales": list(LOCALES),
            },
            "arms": {},
        }
        failed = False
        with gzip.open(changed_path, "wt", encoding="utf-8") as changed_out, ThreadPoolExecutor(
            max_workers=args.workers
        ) as pool:
            futures = [
                pool.submit(
                    score_arm, args.base_bin, args.head_bin, docs_by_locale[loc], tmp, loc, iban, card, default, family
                )
                for loc, iban, card, default, family in arms()
            ]
            for future in futures:
                arm, result, changed = future.result()
                failed |= result["failed"]
                report["arms"][arm] = result
                for row in changed:
                    changed_out.write(json.dumps(row, ensure_ascii=False) + "\n")
                stats = result["stats"]
                print(
                    f"{arm:78s} base_leak={stats['base_leaked']:8d} head_leak={stats['head_leaked']:8d} "
                    f"regressed={stats.get('regressed_docs', 0):5d} improved={stats.get('improved_docs', 0):5d} "
                    f"relabelled={stats.get('relabelled_docs', 0):5d}"
                    + ("  FAILED" if result["failed"] else ""),
                    flush=True,
                )
    report["failed"] = failed
    report["arm_count"] = len(report["arms"])
    report["elapsed_s"] = round(time.time() - started, 1)
    report["totals"] = {
        key: sum(arm["stats"].get(key, 0) for arm in report["arms"].values())
        for key in ("base_leaked", "head_leaked", "regressed_docs", "improved_docs", "relabelled_docs", "missing_response")
    }
    report["changed_docs_file"] = str(changed_path)
    out.write_text(json.dumps(report, indent=1, ensure_ascii=False))
    print(f"ARMS={report['arm_count']} FAILED={failed} totals={report['totals']} elapsed={report['elapsed_s']}s")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
