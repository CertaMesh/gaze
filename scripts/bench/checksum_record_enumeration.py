#!/usr/bin/env python3
"""Old vs new acceptance of IBAN and card shapes when the checksum fails (todo 3906).

Runs one deterministic value x context grid through two `gaze daemon` binaries
(base, candidate) and compares, per document, the bytes of the value each
binary protects. The candidate keeps IBAN and card candidates whose mod-97 or
Luhn check fails (`on_fail = "record"` on `iban.structural`, the new
`iban.cued` and `card.cued`), so this lists exactly what newly tokenizes.

Values:
- registry IBANs, every ISO 13616 country, checksum-valid and one-digit-off
  invalid twins, spaced and compact;
- IBAN-structured values the registry rule cannot see: non-IBAN country codes
  (AU, CA, NZ, US) and a registry country with a dropped digit;
- cards in every layout `scan_card_run` knows (4-4-4-4 with space, dash and
  NBSP; 4-4-4-4-3; 4-6-5; 4-6-4; compact 13/15/16/19), Luhn-valid and invalid,
  across issuer and non-issuer leading digits;
- benign lookalikes: 20-digit and 12-digit grouped references, a phone number,
  a timestamp, a version string, an amount, a UPS tracking number.

Contexts: IBAN cues, card cues (EN, DE, brand, JSON key, log key), and no cue
(order, voucher, reference, tracking, invoice labels, bare).

Exit status 1 when:
- any document loses a value byte base protected;
- any benign lookalike newly tokenizes, in any context;
- a Luhn-failing card newly tokenizes without a card cue (the `card.structural`
  veto must stand);
- a non-registry IBAN shape newly tokenizes without an IBAN cue;
- any daemon response is missing.

Usage:
    python3 scripts/bench/checksum_record_enumeration.py BASE_GAZE CAND_GAZE OUT.json

Build both `gaze` binaries (`cargo build -p gaze-cli`) from the two commits and
pass their paths; nothing may rebuild them while this runs.
"""

from __future__ import annotations

import collections
import json
import random
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import iban_trailing_word_enumeration as base_enum  # noqa: E402

SEED = 20260927
NBSP = " "

POLICY = """schema_version = "0.1.0"

[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = ["core", "locale-de", "locale-en"]

[locale]
active = ["en-US", "de-DE"]

[[rule]]
kind = "class"
class = "custom:iban"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:credit_card"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"""

# (context kind, prefix, trailer)
CONTEXTS = [
    ("iban_cue", "IBAN ", "."),
    ("iban_cue", "Our IBAN is ", " and the BIC is NWBKGB2L."),
    ("iban_cue", 'data: {"iban": "', '"}'),
    ("card_cue", "credit card number ", "."),
    ("card_cue", "Kreditkartennummer: ", " (gesperrt)"),
    ("card_cue", "paid with Visa ", " today"),
    ("card_cue", 'data: {"card_number": "', '"}'),
    ("card_cue", "event=charge cardNumber=", " status=declined"),
    ("no_cue", "Order ", " shipped."),
    ("no_cue", "Voucher code ", "."),
    ("no_cue", "Ref: ", ""),
    ("no_cue", "Tracking ", " delivered"),
    ("no_cue", "Rechnung Nr. ", " bezahlt."),
    ("no_cue", "", ""),
    ("no_cue", 'data: {"orderRef": "', '"}'),
]


# Checksum-failed values the candidate must protect whole (review of #694, round 3): a nested
# JSON key, a `label:` after a copula or a parenthetical, `Karte, Nummer`, a cued 4-4-4-4-3 whose
# first 16 digits pass Luhn, and the `UK` mistyping of a GB IBAN. (kind, prefix, value, trailer)
RECALL_ROWS = [
    ("nested json card", '{"credit_card": {"number": "', "4111 1111 1111 1112", '"}}'),
    ("nested json card", '{"card": {"number": "', "4111111111111112", '", "cvc": "123"}}'),
    ("nested json iban", '{"bank": {"iban": {"value": "', "US12345678901234567", '"}}}'),
    ("copula label card", "Card number is: ", "4111 1111 1111 1112", ""),
    ("parenthetical label card", "Card number (see below): ", "4111 1111 1111 1112", ""),
    ("copula label iban", "IBAN is: ", "US12 3456 7890 1234 5678", ""),
    ("parenthetical label iban", "IBAN (USD account): ", "US12 3456 7890 1234 5678", ""),
    ("polite label iban", "IBAN, bitte: ", "US12 3456 7890 1234 5678", ""),
    ("karte comma nummer", "Karte, Nummer ", "4111 1111 1111 1112", ""),
    ("cued 4-4-4-4-3", "Maestro card ", "6759 6498 2643 8453 012", ""),
    ("uk iban", "IBAN ", "UK12 3456 7890 1234 5678", ""),
]


def luhn_ok(digits: str) -> bool:
    total = 0
    for index, char in enumerate(reversed(digits)):
        value = int(char)
        if index % 2:
            value *= 2
            value -= 9 if value > 9 else 0
        total += value
    return total % 10 == 0


def with_luhn(body: str, valid: bool) -> str:
    """`body` plus a check digit that makes it pass (valid) or fail Luhn."""
    for check in "0123456789":
        if luhn_ok(body + check) == valid:
            return body + check
    raise AssertionError("unreachable")


def group(digits: str, widths: tuple[int, ...], sep: str) -> str:
    parts, at = [], 0
    for width in widths:
        parts.append(digits[at : at + width])
        at += width
    assert at == len(digits)
    return sep.join(parts)


CARD_LAYOUTS = {
    "4-4-4-4 space": ((4, 4, 4, 4), " "),
    "4-4-4-4 dash": ((4, 4, 4, 4), "-"),
    "4-4-4-4 nbsp": ((4, 4, 4, 4), NBSP),
    "4-4-4-4-3": ((4, 4, 4, 4, 3), " "),
    "4-6-5": ((4, 6, 5), " "),
    "4-6-4": ((4, 6, 4), " "),
    "compact 13": ((13,), ""),
    "compact 15": ((15,), ""),
    "compact 16": ((16,), ""),
    "compact 19": ((19,), ""),
}
CARD_LEADS = ["4", "51", "55", "34", "37", "6011", "35", "1", "9"]


def mod97_ok(value: str) -> bool:
    compact = value.replace(" ", "").upper()
    rearranged = compact[4:] + compact[:4]
    return int("".join(str(int(ch, 36)) for ch in rearranged)) % 97 == 1


def values(r: random.Random) -> list[dict]:
    out = []
    for compact in base_enum.sample_ibans(r)[:: base_enum.PER_COUNTRY]:
        bumped = compact[:3] + str((int(compact[3]) + 1) % 10) + compact[4:]
        for valid, value in ((True, compact), (False, bumped)):
            assert mod97_ok(value) == valid
            for shape in (value, base_enum.spaced_form(value)):
                out.append({"kind": "iban_registry", "valid": valid, "value": shape})
    for country in ("AU", "CA", "NZ", "US"):
        for length in (18, 22, 26, 30):
            compact = country + "".join(r.choice("0123456789") for _ in range(length - 2))
            for shape in (compact, base_enum.spaced_form(compact)):
                out.append({"kind": "iban_unknown_country", "valid": mod97_ok(shape), "value": shape})
    de = base_enum.iban("DE", "".join(r.choice("0123456789") for _ in range(18)))
    for shape in (de[:-2], base_enum.spaced_form(de[:-2])):
        out.append({"kind": "iban_wrong_length", "valid": False, "value": shape})
    for layout, (widths, sep) in CARD_LAYOUTS.items():
        length = sum(widths)
        for lead in CARD_LEADS:
            body = lead + "".join(r.choice("0123456789") for _ in range(length - len(lead) - 1))
            for valid in (True, False):
                digits = with_luhn(body, valid)
                if set(digits) == {"0"}:
                    continue
                out.append({"kind": f"card {layout}", "valid": valid, "value": group(digits, widths, sep)})
    lookalikes = {
        "ref 5x4": " ".join("".join(r.choice("0123456789") for _ in range(4)) for _ in range(5)),
        "ref 3x4": " ".join("".join(r.choice("0123456789") for _ in range(4)) for _ in range(3)),
        "phone": "+49 30 1234 5678",
        "timestamp": "2026-04-17 08:03:51",
        "version": "1.2.3.4567",
        "amount": "1 234 567,89",
        "ups tracking": "1Z999AA10123456784",
        # Review of #694, F1: compact numbers that are no card.
        "epoch ms": "1695827361000",
        "compact phone 13": "4915123456789",
        "compact phone 15": "491761234567890",
    }
    for kind, value in lookalikes.items():
        out.append({"kind": f"benign {kind}", "valid": None, "value": value})
    return out


def documents() -> list[dict]:
    r = random.Random(SEED)
    docs = []
    for value in values(r):
        for context, prefix, trailer in CONTEXTS:
            text = prefix + value["value"] + trailer
            start = len(prefix.encode())
            docs.append({
                **value,
                "context": context,
                "prefix": prefix,
                "text": text,
                "span": (start, start + len(value["value"].encode())),
            })
    return docs


def protected_bytes(response: dict, span: tuple[int, int]) -> int:
    return base_enum.iban_view(response, span)[1]


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    base_bin, cand_bin, out_path = sys.argv[1:]
    identities = {"base": base_enum.binary_identity(base_bin), "candidate": base_enum.binary_identity(cand_bin)}
    if identities["base"]["sha256"] == identities["candidate"]["sha256"]:
        print("refusing: base and candidate binaries are identical", file=sys.stderr)
        return 2
    docs = documents()
    with tempfile.TemporaryDirectory() as tmp:
        policy = Path(tmp) / "policy.toml"
        policy.write_text(POLICY, encoding="utf-8")
        base = base_enum.run(base_bin, policy, docs)
        cand = base_enum.run(cand_bin, policy, docs)

    # (reason, value kind, context kind): never a document text or value.
    failures: list[tuple[str, str, str]] = []
    newly: collections.Counter = collections.Counter()
    newly_bytes: collections.Counter = collections.Counter()
    unchanged: collections.Counter = collections.Counter()
    for doc, b, c in zip(docs, base, cand, strict=True):
        if b is None or c is None:
            failures.append(("missing response", doc["kind"], doc["context"]))
            continue
        before, after = protected_bytes(b, doc["span"]), protected_bytes(c, doc["span"])
        key = (doc["kind"], doc["valid"], doc["context"])
        if after < before:
            failures.append(("lost bytes", doc["kind"], doc["context"]))
        if after > before:
            newly[key] += 1
            newly_bytes[key] += after - before
            if doc["kind"].startswith("benign"):
                failures.append(("benign lookalike newly tokenized", doc["kind"], doc["context"]))
            if doc["kind"].startswith("card") and doc["valid"] is False and doc["context"] != "card_cue":
                failures.append(("Luhn-failing card tokenized without a card cue", doc["kind"], doc["context"]))
            if doc["kind"] == "iban_unknown_country" and doc["context"] != "iban_cue":
                failures.append(("non-registry IBAN shape tokenized without an IBAN cue", doc["kind"], doc["context"]))
        else:
            unchanged[key] += 1

    recall_docs = [
        {"kind": kind, "context": "recall", "text": prefix + value + trailer,
         "span": (len(prefix.encode()), len((prefix + value).encode()))}
        for kind, prefix, value, trailer in RECALL_ROWS
    ]
    with tempfile.TemporaryDirectory() as tmp:
        policy = Path(tmp) / "policy.toml"
        policy.write_text(POLICY, encoding="utf-8")
        recall = base_enum.run(cand_bin, policy, recall_docs)
    for doc, response in zip(recall_docs, recall, strict=True):
        span = doc["span"][1] - doc["span"][0]
        if response is None or protected_bytes(response, doc["span"]) != span:
            failures.append(("recall row not protected whole", doc["kind"], doc["context"]))

    rows = [
        {"kind": kind, "checksum_valid": valid, "context": context,
         "newly_tokenized_documents": newly[(kind, valid, context)],
         "newly_protected_bytes": newly_bytes[(kind, valid, context)]}
        for (kind, valid, context) in sorted(newly, key=lambda k: (k[0], str(k[1]), k[2]))
    ]
    report = {
        "binaries": identities,
        "seed": SEED,
        "documents": len(docs),
        "newly_tokenized": rows,
        "newly_tokenized_documents": sum(newly.values()),
        "unchanged_documents": sum(unchanged.values()),
        "failures": [list(failure) for failure in failures],
    }
    Path(out_path).write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{len(docs)} documents; newly tokenized {sum(newly.values())}; failures {len(failures)}")
    for row in rows:
        print(f"  {row['kind']:<24} valid={str(row['checksum_valid']):<5} {row['context']:<9} "
              f"+{row['newly_tokenized_documents']} docs, +{row['newly_protected_bytes']} B")
    for reason, kind, context in sorted(set(failures)):
        print(f"FAIL {reason}: {kind} / {context}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
