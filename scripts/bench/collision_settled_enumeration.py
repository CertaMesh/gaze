#!/usr/bin/env python3
"""IBAN class enumeration for the collision-settled resolver state (todo #3709).

Runs a deterministic IBAN document set through two `gaze daemon` binaries
(base, fix) under five policies and compares, per document, the class each
binary gives the IBAN span and the IBAN bytes each binary protects (union of
manifest `raw_span`s).

Document set: 7 IBAN shapes (AT/BE/LU/PL/HU/DE/GB, valid mod-97, seeded),
6 per country whose spaced form carries a Luhn-valid card run and 6 whose
does not, x spaced/compact x 3 prefixes x 6 trailing contexts. The trailing contexts
include a four-digit-group-plus-capitalised-word shape, which postal.at_ch
(PR #613) and the `trailing_group_word` custom recognizer in the `foreign`
policy both match on an IBAN's last group: an unrelated lower-priority overlap.

A document is "settled" when the card variant also fires inside the IBAN (a
Luhn-valid 13-19 digit run, the `card.structural` shape): collision policy
then decides the family. Only settled documents may change class base -> fix,
and only from the family token to `iban`.

Usage:
    python3 scripts/bench/collision_settled_enumeration.py BASE_BIN FIX_BIN OUT.json

Exit status is 1 when any IBAN byte is lost (protected by base, raw in fix),
any never-settled document changes class, any settled document changes in a
direction other than family -> iban, or any daemon response is missing.
"""

from __future__ import annotations

import json
import random
import re
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

SEED = 20260922
PER_COUNTRY = 6
MAX_TRIES = 20_000
# (country, BBAN generator). Lengths follow the national IBAN formats.
COUNTRIES = {
    "AT": lambda r: digits(r, 16),
    "BE": lambda r: digits(r, 12),
    "LU": lambda r: digits(r, 3) + alnum(r, 13),
    "PL": lambda r: digits(r, 24),
    "HU": lambda r: digits(r, 24),
    "DE": lambda r: digits(r, 18),
    "GB": lambda r: letters(r, 4) + digits(r, 14),
}
PREFIXES = ["Zahlung an ", "IBAN: ", "Bitte überweisen auf "]
TRAILERS = [
    " Kontoinhaber Max",
    " Verwendungszweck Miete",
    " bitte bis Freitag",
    ".",
    "",
    " Max Mustermann",
]
FAMILY = "family:payment-card-or-iban"
CARD = re.compile(r"\b\d(?:[\s-]?\d){12,18}\b")

POLICY_HEAD = """schema_version = "0.1.0"

[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = [{bundled}]

[locale]
active = ["{locale}"]
"""
FOREIGN = """
[[policy.custom_recognizers]]
kind = "regex"
name = "trailing_group_word"
pattern = '\\b\\d{4} [A-Z][a-z]+'
class = "custom:group_word"
"""
RULE = """
[[rule]]
kind = "class"
class = "{cls}"
action = "tokenize"
"""
DEFAULT = """
[[rule]]
kind = "default"
action = "preserve"
"""
# name -> (bundled, locale, foreign recognizer, tokenize the family class)
POLICIES = {
    "de-at-preserve": ('"core", "locale-de"', "de-AT", False, False),
    "de-at-preserve-foreign": ('"core", "locale-de"', "de-AT", True, False),
    "core-de-at-family": ('"core"', "de-AT", False, True),
    "de-de-family-foreign": ('"core", "locale-de"', "de-DE", True, True),
    "core-extended-en-us-family": ('"core", "core-extended"', "en-US", False, True),
}


def digits(r: random.Random, n: int) -> str:
    return "".join(r.choice("0123456789") for _ in range(n))


def letters(r: random.Random, n: int) -> str:
    return "".join(r.choice("ABCDEFGHIJKLMNOPQRSTUVWXYZ") for _ in range(n))


def alnum(r: random.Random, n: int) -> str:
    return "".join(r.choice("0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ") for _ in range(n))


def iban(country: str, bban: str) -> str:
    rearranged = bban + country + "00"
    numeric = "".join(str(int(ch, 36)) for ch in rearranged)
    check = 98 - int(numeric) % 97
    return f"{country}{check:02d}{bban}"


def luhn(number: str) -> bool:
    total = 0
    for i, ch in enumerate(reversed(number)):
        d = int(ch)
        if i % 2:
            d = d * 2 - 9 if d > 4 else d * 2
        total += d
    return total % 10 == 0


def settles(text: str, start: int, end: int) -> bool:
    """True when a Luhn-valid card-shaped run overlaps chars start..end."""
    return any(
        m.start() < end and m.end() > start and luhn(re.sub(r"\D", "", m.group()))
        for m in CARD.finditer(text)
    )


def sample_ibans(r: random.Random) -> list[str]:
    """PER_COUNTRY IBANs per country whose spaced form settles, and as many
    that do not, so both sides of the invariant are exercised. A shape that
    cannot carry a 13-digit run (LU's alphanumeric BBAN) yields no settled
    sample within MAX_TRIES; the report counts settled documents per country."""
    picked = []
    for country, bban in COUNTRIES.items():
        want = {True: PER_COUNTRY, False: PER_COUNTRY}
        for _ in range(MAX_TRIES):
            if not any(want.values()):
                break
            compact = iban(country, bban(r))
            spaced = spaced_form(compact)
            key = settles(spaced, 0, len(spaced))
            if want[key]:
                want[key] -= 1
                picked.append(compact)
    return picked


def spaced_form(compact: str) -> str:
    return " ".join(compact[i : i + 4] for i in range(0, len(compact), 4))


def documents() -> list[dict]:
    r = random.Random(SEED)
    docs = []
    for compact in sample_ibans(r):
        for shape in (spaced_form(compact), compact):
            for prefix in PREFIXES:
                for trailer in TRAILERS:
                    text = prefix + shape + trailer
                    docs.append(
                        {
                            "country": compact[:2],
                            "text": text,
                            "iban_span": (
                                len(prefix.encode()),
                                len((prefix + shape).encode()),
                            ),
                            "settled": settles(text, len(prefix), len(prefix) + len(shape)),
                        }
                    )
    return docs


def run(binary: str, policy: Path, docs: list[dict]) -> list[dict]:
    lines = "".join(
        json.dumps({"session_id": f"doc-{i}", "text": d["text"]}) + "\n"
        for i, d in enumerate(docs)
    )
    out = subprocess.run(
        [binary, "daemon", "--policy", str(policy)],
        input=lines,
        capture_output=True,
        text=True,
        check=False,
    )
    by_id = {}
    for line in out.stdout.splitlines():
        value = json.loads(line)
        by_id[value.get("session_id")] = value
    return [by_id.get(f"doc-{i}") for i in range(len(docs))]


def iban_view(response: dict, span: tuple[int, int]) -> tuple[str, int]:
    """Class covering the IBAN span (or "raw") and protected IBAN bytes."""
    start, end = span
    protected = set()
    classes = []
    for entry in response.get("manifest", []):
        raw = entry["raw_span"]
        lo, hi = max(raw["start"], start), min(raw["end"], end)
        if lo < hi:
            protected.update(range(lo, hi))
            cls = entry["class"]
            classes.append(cls["Custom"] if isinstance(cls, dict) else str(cls))
    return ("+".join(sorted(set(classes))) or "raw", len(protected))


def main() -> int:
    base_bin, fix_bin, out_path = sys.argv[1:4]
    docs = documents()
    report = {
        "documents": len(docs),
        "settled": sum(d["settled"] for d in docs),
        "settled_by_country": dict(Counter(d["country"] for d in docs if d["settled"])),
        "policies": {},
    }
    failed = False
    with tempfile.TemporaryDirectory() as tmp:
        for name, (bundled, locale, foreign, family) in POLICIES.items():
            text = POLICY_HEAD.format(bundled=bundled, locale=locale)
            if foreign:
                text += FOREIGN
            for cls in ["custom:iban", "custom:credit_card"] + (
                [f"custom:{FAMILY}"] if family else []
            ):
                text += RULE.format(cls=cls)
            text += DEFAULT
            policy = Path(tmp) / f"{name}.toml"
            policy.write_text(text)
            base = run(base_bin, policy, docs)
            fix = run(fix_bin, policy, docs)
            stats = Counter()
            transitions = Counter()
            examples = []
            for doc, b, f in zip(docs, base, fix):
                if b is None or f is None:
                    stats["missing_response"] += 1
                    continue
                b_cls, b_bytes = iban_view(b, doc["iban_span"])
                f_cls, f_bytes = iban_view(f, doc["iban_span"])
                stats["lost_bytes"] += max(0, b_bytes - f_bytes)
                stats["gained_bytes"] += max(0, f_bytes - b_bytes)
                if b_cls == f_cls:
                    continue
                key = f"{b_cls} -> {f_cls} ({'settled' if doc['settled'] else 'never settled'})"
                transitions[key] += 1
                allowed = doc["settled"] and b_cls in (FAMILY, "raw") and f_cls == "iban"
                if not allowed:
                    stats["disallowed_changes"] += 1
                    if len(examples) < 5:
                        examples.append({"text": doc["text"], "base": b_cls, "fix": f_cls})
            if stats["lost_bytes"] or stats["disallowed_changes"] or stats["missing_response"]:
                failed = True
            report["policies"][name] = {
                "lost_bytes": stats["lost_bytes"],
                "gained_bytes": stats["gained_bytes"],
                "disallowed_changes": stats["disallowed_changes"],
                "missing_responses": stats["missing_response"],
                "transitions": dict(transitions),
                "disallowed_examples": examples,
            }
            print(name, json.dumps(report["policies"][name]), flush=True)
    Path(out_path).write_text(json.dumps(report, indent=2) + "\n")
    print(
        f"documents={report['documents']} settled={report['settled']} "
        f"by_country={report['settled_by_country']} failed={failed}"
    )
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
