#!/usr/bin/env python3
"""IBAN trailing-word enumeration for the `iban.structural` length branches (todo #3708).

Runs a deterministic IBAN document set through two `gaze daemon` binaries (base,
fix) under four policies and compares, per document, the class each binary gives
the IBAN span and the IBAN bytes each binary protects (union of manifest
`raw_span`s).

Why: on base, `iban.structural` was `\\b[A-Z]{2}\\d{2}(?: ?[A-Z0-9]{4}){2,7} ?[A-Z0-9]{1,4}\\b`.
Both the repeated 4-char group and the mandatory 1-4 char tail accept an optional
leading space, so a following upper-case or digit word extends the candidate past
the end of the IBAN. The over-long span then fails `iban_mod97` (which checks the
country's registry length), the candidate is dropped by validator veto, and the
IBAN ships raw -- or, when the digits happen to be Luhn-valid, `card.structural`
claims them as `custom:credit_card`. The fix replaces the open-ended shape with
one alternation branch per ISO 13616 registry length.

Document set: every one of the 89 registry countries x PER_COUNTRY valid mod-97
IBANs x spaced/compact x 3 prefixes x TRAILERS trailing contexts. The trailers
are the invoice/footer shapes that trigger the defect (` BIC`, ` BIC:`, ` SWIFT`,
` EUR`, ` OK`) plus controls that never did (lower-case words, punctuation,
newline, end of text).

The fix is a strict narrowing of the matched language, so the interesting
direction is recall: no document may lose a protected IBAN byte. Every country
the new pattern drops was already rejected by the validator's length gate, so
`lost_bytes` must be 0 in every policy. Gained bytes are reported per policy and
per trailer so the recovered leak is quantified rather than asserted.

Usage:
    python3 scripts/bench/iban_trailing_word_enumeration.py BASE_BIN FIX_BIN OUT.json

Exit status is 1 when any IBAN byte is lost (protected by base, raw in fix), when
any document ends with the IBAN span only PARTIALLY protected in fix (part of the
value tokenized and the rest raw, the shape this defect produced), or when any
daemon response is missing. Documents where neither arm detects the IBAN are
counted and reported, not failed: that is the mandatory-anchor contract, not this
defect.
"""

from __future__ import annotations

import json
import random
import re
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

SEED = 20260922
PER_COUNTRY = 4
# ISO 13616 IBAN Registry lengths, mirroring `gaze_types::iban_registry_length`.
# A country missing here would go unexercised, so the count is asserted below.
LENGTHS = {
    "AD": 24, "AE": 23, "AL": 28, "AT": 20, "AZ": 28, "BA": 20, "BE": 16,
    "BG": 22, "BH": 22, "BI": 27, "BR": 29, "BY": 28, "CH": 21, "CR": 22,
    "CY": 28, "CZ": 24, "DE": 22, "DJ": 27, "DK": 18, "DO": 28, "EE": 20,
    "EG": 29, "ES": 24, "FI": 18, "FK": 18, "FO": 18, "FR": 27, "GB": 22,
    "GE": 22, "GI": 23, "GL": 18, "GR": 27, "GT": 28, "HN": 28, "HR": 21,
    "HU": 28, "IE": 22, "IL": 23, "IQ": 23, "IS": 26, "IT": 27, "JO": 30,
    "KW": 30, "KZ": 20, "LB": 28, "LC": 32, "LI": 21, "LT": 20, "LU": 20,
    "LV": 21, "LY": 25, "MC": 27, "MD": 24, "ME": 22, "MK": 19, "MN": 20,
    "MR": 27, "MT": 31, "MU": 30, "NI": 28, "NL": 18, "NO": 15, "OM": 23,
    "PK": 24, "PL": 28, "PS": 29, "PT": 25, "QA": 29, "RO": 24, "RS": 22,
    "RU": 33, "SA": 24, "SC": 31, "SD": 18, "SE": 24, "SI": 19, "SK": 24,
    "SM": 27, "SO": 23, "ST": 25, "SV": 28, "TL": 23, "TN": 24, "TR": 26,
    "UA": 29, "VA": 22, "VG": 24, "XK": 20, "YE": 30,
}
EXPECTED_COUNTRIES = 89

# `iban.structural` declares `mandatory_anchor = "iban"`, so a document with no IBAN cue is
# deliberately not detected at all -- in BOTH arms. The third prefix carries no cue and is kept on
# purpose: it holds that pre-existing anchor decision constant across base and fix, so a change
# there would show up as lost bytes rather than hiding behind the cue.
PREFIXES = ["IBAN ", "IBAN: ", "Bitte überweisen auf "]
# Trailer -> whether base could over-match into it. An upper-case/digit run can
# be absorbed either as a whole 4-char group or as the 1-4 char tail; a
# lower-case word, punctuation or a line break cannot.
TRAILERS = {
    " BIC": True,
    " BIC: BKAUATWW": True,
    " SWIFT": True,
    " EUR": True,
    " OK": True,
    " A": True,
    " 1234": True,
    " und": False,
    " Bank PKO": False,
    ".": False,
    ",": False,
    "\nBIC": False,
    "": False,
}

POLICY_HEAD = """schema_version = "0.1.0"

[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = [{bundled}]

[locale]
active = ["{locale}"]
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
# name -> (bundled, locale). Every arm carries a locale pack: `[locale.cues.iban]` lives there and
# satisfies the recognizer's mandatory anchor, so a `core`-only arm would detect nothing and score
# a vacuous pass.
POLICIES = {
    "core-de-de": ('"core", "locale-de"', "de-DE"),
    "core-en-us": ('"core", "locale-en"', "en-US"),
    "core-both-locales-de-de": ('"core", "locale-de", "locale-en"', "de-DE"),
    "core-both-locales-en-us": ('"core", "locale-de", "locale-en"', "en-US"),
}


def alnum(r: random.Random, n: int) -> str:
    return "".join(r.choice("0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ") for _ in range(n))


def iban(country: str, bban: str) -> str:
    rearranged = bban + country + "00"
    numeric = "".join(str(int(ch, 36)) for ch in rearranged)
    check = 98 - int(numeric) % 97
    return f"{country}{check:02d}{bban}"


def spaced_form(compact: str) -> str:
    return " ".join(compact[i : i + 4] for i in range(0, len(compact), 4))


def sample_ibans(r: random.Random) -> list[str]:
    """PER_COUNTRY valid IBANs for every registry country.

    The BBAN is alphanumeric everywhere. That is wider than several national
    formats allow, but the recognizer's character class is `[A-Z0-9]` for every
    country, so an alphanumeric BBAN exercises exactly the shape the pattern
    accepts and keeps the sample independent of national BBAN sub-structure.
    """
    assert len(LENGTHS) == EXPECTED_COUNTRIES, (
        f"registry table has {len(LENGTHS)} countries, expected {EXPECTED_COUNTRIES}; "
        "sync with gaze_types::iban_registry_length"
    )
    return [
        iban(country, alnum(r, length - 4))
        for country, length in sorted(LENGTHS.items())
        for _ in range(PER_COUNTRY)
    ]


def documents() -> list[dict]:
    r = random.Random(SEED)
    docs = []
    for compact in sample_ibans(r):
        for shape in (spaced_form(compact), compact):
            for prefix in PREFIXES:
                for trailer, absorbable in TRAILERS.items():
                    text = prefix + shape + trailer
                    docs.append(
                        {
                            "country": compact[:2],
                            "text": text,
                            "spaced": shape != compact,
                            "trailer": trailer,
                            "absorbable": absorbable,
                            "iban_span": (
                                len(prefix.encode()),
                                len((prefix + shape).encode()),
                            ),
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
        "countries": len(LENGTHS),
        "per_country": PER_COUNTRY,
        "seed": SEED,
        "policies": {},
    }
    failed = False
    with tempfile.TemporaryDirectory() as tmp:
        for name, (bundled, locale) in POLICIES.items():
            text = POLICY_HEAD.format(bundled=bundled, locale=locale)
            for cls in ("custom:iban", "custom:credit_card"):
                text += RULE.format(cls=cls)
            text += DEFAULT
            policy = Path(tmp) / f"{name}.toml"
            policy.write_text(text)
            base = run(base_bin, policy, docs)
            fix = run(fix_bin, policy, docs)

            stats = Counter()
            transitions = Counter()
            gained_by_trailer = defaultdict(int)
            lost_examples = []
            partial_examples = []
            for doc, b, f in zip(docs, base, fix):
                if b is None or f is None:
                    stats["missing_response"] += 1
                    continue
                b_cls, b_bytes = iban_view(b, doc["iban_span"])
                f_cls, f_bytes = iban_view(f, doc["iban_span"])
                span_len = doc["iban_span"][1] - doc["iban_span"][0]

                lost = max(0, b_bytes - f_bytes)
                gained = max(0, f_bytes - b_bytes)
                stats["lost_bytes"] += lost
                stats["gained_bytes"] += gained
                gained_by_trailer[doc["trailer"]] += gained
                if lost and len(lost_examples) < 5:
                    lost_examples.append(
                        {"text": doc["text"], "base": b_cls, "fix": f_cls}
                    )
                # A partially covered IBAN is the shape this defect produced: part of
                # the value tokenized, the rest raw on the wire. Full non-detection is
                # NOT counted here -- under a no-cue prefix the mandatory anchor makes
                # that the intended outcome in both arms, and any base/fix difference
                # is already caught by lost_bytes.
                if 0 < f_bytes < span_len:
                    stats["fix_partial"] += 1
                    if len(partial_examples) < 5:
                        partial_examples.append(
                            {
                                "text": doc["text"],
                                "fix": f_cls,
                                "protected": f_bytes,
                                "span": span_len,
                            }
                        )
                if f_bytes == 0:
                    stats["fix_undetected"] += 1
                if b_bytes == 0:
                    stats["base_undetected"] += 1
                if b_cls != f_cls:
                    transitions[f"{b_cls} -> {f_cls}"] += 1

            if stats["lost_bytes"] or stats["fix_partial"] or stats["missing_response"]:
                failed = True
            report["policies"][name] = {
                "lost_bytes": stats["lost_bytes"],
                "gained_bytes": stats["gained_bytes"],
                "fix_partial": stats["fix_partial"],
                "base_undetected": stats["base_undetected"],
                "fix_undetected": stats["fix_undetected"],
                "missing_responses": stats["missing_response"],
                "transitions": dict(transitions),
                "gained_bytes_by_trailer": dict(gained_by_trailer),
                "lost_examples": lost_examples,
                "fix_partial_examples": partial_examples,
            }
            print(name, json.dumps(report["policies"][name]), flush=True)
    Path(out_path).write_text(json.dumps(report, indent=2) + "\n")
    print(
        f"documents={report['documents']} countries={report['countries']} failed={failed}"
    )
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
