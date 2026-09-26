#!/usr/bin/env python3
"""Gaze-native agentic benchmark layers A (identifiers) and D (benign lookalikes).

Layer A renders identifiers into the shapes agents send (prose with and without
a cue, NBSP and NARROW NBSP spacing, log `key=value`, CSV, proxy-shaped
tool-call JSON). Every checksum family also gets a checksum-invalid twin in the
same shape; the twin stays scored gold, and the validator gold census reports
the split. Layer D renders benign lookalikes (amounts, SKUs, colours, versions,
order and tracking IDs, dates) that must stay untouched.

Held-out protocol: every template, cue, key, name and seed is assigned to the
`dev` or `test` partition before anything is generated, and each perturbation
(NBSP variants, invalid twin) is derived from its parent inside the parent's
partition. The published arm scores `test` only; `dev` is for rule work.

The checksum code below is written from the published standards, not from
Gaze's validators; `test_agentic_layers.py` pins it to standard test vectors
and the benchmark's validator probe cross-checks it against Gaze on every run.
Gold spans come from the inserted values, never from Gaze output. Every value
is synthetic.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable, Mapping, Sequence

import gaze_bench_score as score


GENERATOR_VERSION = 1
PARTITIONS = ("dev", "test")
PUBLISHED_PARTITION = "test"
PARTITION_SEEDS = {"dev": 2026092601, "test": 2026092602}
DOCS_PER_FAMILY = 10
SCORED_LABELS_PATH = Path("docs/reference/benchmarks/scored-labels-agentic.json")
LAYER_IDENTIFIERS = "A"
LAYER_LOOKALIKES = "D"
SOURCE_DATASET = "gaze-agentic-layers"

NBSP = "\u00a0"
NARROW_NBSP = "\u202f"

SURFACES = (
    "prose_cue",
    "prose_nocue",
    "nbsp",
    "narrow_nbsp",
    "log_kv",
    "csv",
    "tool_json",
)
LOOKALIKE_SURFACES = ("prose", "log_kv", "tool_json")
# Perturbed surfaces: the prose_cue parent with its spaces swapped for this.
PERTURBED_SURFACES = {"nbsp": NBSP, "narrow_nbsp": NARROW_NBSP}
VALID = "valid"
INVALID = "invalid"
UNCHECKED = "unchecked"
BENIGN = "benign"


class LayerError(ValueError):
    """The generated corpus or its contract is inconsistent; fail closed."""


# --------------------------------------------------------------------------
# Deterministic randomness: sha256 over (seed, stream, counter). Independent of
# Python's `random` implementation, so a pinned corpus hash survives upgrades.


class Rng:
    def __init__(self, seed: int, stream: str) -> None:
        self._prefix = f"{seed}:{stream}:".encode()
        self._counter = 0

    def _next64(self) -> int:
        digest = hashlib.sha256(self._prefix + str(self._counter).encode()).digest()
        self._counter += 1
        return int.from_bytes(digest[:8], "big")

    def below(self, bound: int) -> int:
        if bound <= 0:
            raise ValueError("bound must be positive")
        limit = (1 << 64) - ((1 << 64) % bound)
        while True:
            value = self._next64()
            if value < limit:
                return value % bound

    def between(self, low: int, high: int) -> int:
        return low + self.below(high - low + 1)

    def choice(self, items: Sequence[str]) -> str:
        return items[self.below(len(items))]

    def digits(self, count: int) -> str:
        return "".join(str(self.below(10)) for _ in range(count))

    def shuffled(self, items: Sequence[str]) -> list[str]:
        result = list(items)
        for index in range(len(result) - 1, 0, -1):
            other = self.below(index + 1)
            result[index], result[other] = result[other], result[index]
        return result


# --------------------------------------------------------------------------
# Checksums, from the standards.


def _only_digits(value: str) -> str:
    return "".join(character for character in value if character.isdigit())


def luhn_valid(value: str) -> bool:
    """ISO/IEC 7812-1 Annex B (Luhn)."""
    digits = _only_digits(value)
    if len(digits) < 2:
        return False
    total = 0
    for position, character in enumerate(reversed(digits)):
        digit = int(character)
        if position % 2 == 1:
            digit *= 2
            if digit > 9:
                digit -= 9
        total += digit
    return total % 10 == 0


def luhn_check_digit(payload: str) -> str:
    for candidate in "0123456789":
        if luhn_valid(payload + candidate):
            return candidate
    raise AssertionError("unreachable: one Luhn digit always exists")


IBAN_LENGTHS = {"AT": 20, "DE": 22, "FR": 27, "GB": 22, "NL": 18}


def _iban_numeric(rearranged: str) -> int:
    return int("".join(str(int(character, 36)) for character in rearranged))


def iban_valid(value: str) -> bool:
    """ISO 13616: registry length plus ISO 7064 MOD 97-10 remainder 1."""
    compact = value.replace(" ", "").replace(NBSP, "").replace(NARROW_NBSP, "").upper()
    if len(compact) < 5 or not compact.isalnum() or not compact.isascii():
        return False
    if IBAN_LENGTHS.get(compact[:2]) != len(compact):
        return False
    return _iban_numeric(compact[4:] + compact[:4]) % 97 == 1


def iban_check_digits(country: str, bban: str) -> str:
    remainder = _iban_numeric(bban + country + "00") % 97
    return f"{98 - remainder:02d}"


def fr_rib_key(bank: str, branch: str, account: str) -> str:
    """French RIB key over an all-digit account number."""
    return f"{97 - (89 * int(bank) + 15 * int(branch) + 3 * int(account)) % 97:02d}"


def steuer_id_check_digit(first_ten: str) -> str:
    """ISO 7064 MOD 11,10 as used by the German Steuer-ID."""
    product = 10
    for character in first_ten:
        total = (int(character) + product) % 10
        if total == 0:
            total = 10
        product = (total * 2) % 11
    check = 11 - product
    return "0" if check == 10 else str(check)


def steuer_id_valid(value: str) -> bool:
    digits = _only_digits(value)
    if len(digits) != 11 or digits[0] == "0":
        return False
    counts = [digits[:10].count(str(digit)) for digit in range(10)]
    repeated = [count for count in counts if count > 1]
    if len(repeated) != 1 or repeated[0] not in (2, 3):
        return False
    return steuer_id_check_digit(digits[:10]) == digits[10]


def bsn_valid(value: str) -> bool:
    """Dutch BSN eleven-test: weights 9..2 and -1 on the last digit."""
    digits = _only_digits(value)
    if len(digits) != 9 or len(set(digits)) == 1:
        return False
    total = sum(int(digit) * weight for digit, weight in zip(digits[:8], range(9, 1, -1)))
    return (total - int(digits[8])) % 11 == 0


def nhs_check_digit(first_nine: str) -> str | None:
    """NHS number modulus 11; None when the payload has no valid check digit."""
    total = sum(int(digit) * weight for digit, weight in zip(first_nine, range(10, 1, -1)))
    check = 11 - total % 11
    if check == 11:
        return "0"
    if check == 10:
        return None
    return str(check)


def nhs_valid(value: str) -> bool:
    digits = _only_digits(value)
    if len(digits) != 10 or len(set(digits)) == 1:
        return False
    return nhs_check_digit(digits[:9]) == digits[9]


def _cpf_digit(payload: str) -> str:
    weight = len(payload) + 1
    total = sum(int(digit) * (weight - index) for index, digit in enumerate(payload))
    return str((total * 10) % 11 % 10)


def cpf_valid(value: str) -> bool:
    """Brazilian CPF: two modulus-11 check digits; repeated digits are invalid."""
    digits = _only_digits(value)
    if len(digits) != 11 or len(set(digits)) == 1:
        return False
    first = _cpf_digit(digits[:9])
    return digits[9] == first and digits[10] == _cpf_digit(digits[:9] + first)


CHECKSUMS: dict[str, Callable[[str], bool]] = {
    "luhn": luhn_valid,
    "iban": iban_valid,
    "steuer_id": steuer_id_valid,
    "bsn": bsn_valid,
    "nhs": nhs_valid,
    "cpf": cpf_valid,
}


# --------------------------------------------------------------------------
# Identifier values. `groups` is the display grouping; the separator between
# groups is what the NBSP surfaces perturb.


@dataclass(frozen=True)
class Value:
    groups: tuple[str, ...]
    joiner: str = " "

    def render(self, separator: str | None = None) -> str:
        if self.joiner != " ":
            return self.joiner.join(self.groups)
        return (separator if separator is not None else " ").join(self.groups)


def _chunks(value: str, sizes: Sequence[int]) -> tuple[str, ...]:
    result = []
    start = 0
    for size in sizes:
        result.append(value[start : start + size])
        start += size
    if start != len(value):
        raise AssertionError("chunk sizes do not cover the value")
    return tuple(result)


def _by_four(value: str) -> tuple[str, ...]:
    return tuple(value[index : index + 4] for index in range(0, len(value), 4))


def _card(rng: Rng) -> str:
    prefix = rng.choice(("4", "51", "52", "53", "54", "55"))
    payload = prefix + rng.digits(15 - len(prefix))
    return payload + luhn_check_digit(payload)


def _iban(country: str, bban: str) -> str:
    return country + iban_check_digits(country, bban) + bban


def _iban_de(rng: Rng) -> str:
    return _iban("DE", str(rng.between(1, 9)) + rng.digits(17))


def _iban_at(rng: Rng) -> str:
    return _iban("AT", str(rng.between(1, 9)) + rng.digits(15))


def _iban_nl(rng: Rng) -> str:
    bank = rng.choice(("ABNA", "RABO", "INGB", "TRIO", "SNSB", "KNAB"))
    return _iban("NL", bank + rng.digits(10))


def _iban_fr(rng: Rng) -> str:
    bank = str(rng.between(10000, 99999))
    branch = rng.digits(5)
    account = rng.digits(11)
    return _iban("FR", bank + branch + account + fr_rib_key(bank, branch, account))


def _iban_gb(rng: Rng) -> str:
    bank = rng.choice(("NWBK", "BARC", "LOYD", "HBUK", "MIDL"))
    return _iban("GB", bank + rng.digits(14))


def _steuer_id(rng: Rng) -> str:
    while True:
        digits = rng.shuffled("0123456789")
        repeated = digits[rng.below(9)]
        first_ten = rng.shuffled(digits[:9] + [repeated])
        if first_ten[0] == "0":
            continue
        payload = "".join(first_ten)
        value = payload + steuer_id_check_digit(payload)
        if steuer_id_valid(value):
            return value


def _bsn(rng: Rng) -> str:
    while True:
        payload = str(rng.between(1, 9)) + rng.digits(7)
        total = sum(int(d) * w for d, w in zip(payload, range(9, 1, -1)))
        check = total % 11
        if check <= 9 and bsn_valid(payload + str(check)):
            return payload + str(check)


def _nhs(rng: Rng) -> str:
    while True:
        payload = str(rng.between(4, 7)) + rng.digits(8)
        check = nhs_check_digit(payload)
        if check is not None and nhs_valid(payload + check):
            return payload + check


def _cpf(rng: Rng) -> str:
    while True:
        payload = rng.digits(9)
        if len(set(payload)) == 1:
            continue
        first = _cpf_digit(payload)
        return payload + first + _cpf_digit(payload + first)


def _bump_last_digit(value: str) -> str:
    """Change the final digit: every checksum here detects a single-digit change."""
    return value[:-1] + str((int(value[-1]) + 1) % 10)


def _bump_iban_check(value: str) -> str:
    return value[:3] + str((int(value[3]) + 1) % 10) + value[4:]


@dataclass(frozen=True)
class IdentifierFamily:
    name: str
    label: str
    language: str
    region: str
    checksum: str | None
    make: Callable[[Rng], str]
    display: Callable[[str], Value]
    twin: Callable[[str], str] | None


IDENTIFIER_FAMILIES: tuple[IdentifierFamily, ...] = (
    IdentifierFamily("card", "CREDITCARDNUMBER", "en", "US", "luhn", _card,
                     lambda v: Value(_by_four(v)), _bump_last_digit),
    IdentifierFamily("iban_de", "IBAN", "de", "DE", "iban", _iban_de,
                     lambda v: Value(_by_four(v)), _bump_iban_check),
    IdentifierFamily("iban_de_compact", "IBAN", "de", "DE", "iban", _iban_de,
                     lambda v: Value((v,)), _bump_iban_check),
    IdentifierFamily("iban_at", "IBAN", "de", "AT", "iban", _iban_at,
                     lambda v: Value(_by_four(v)), _bump_iban_check),
    IdentifierFamily("iban_nl", "IBAN", "nl", "NL", "iban", _iban_nl,
                     lambda v: Value(_by_four(v)), _bump_iban_check),
    IdentifierFamily("iban_fr", "IBAN", "fr", "FR", "iban", _iban_fr,
                     lambda v: Value(_by_four(v)), _bump_iban_check),
    IdentifierFamily("iban_gb", "IBAN", "en", "GB", "iban", _iban_gb,
                     lambda v: Value(_by_four(v)), _bump_iban_check),
    IdentifierFamily("steuer_id", "TAXNUM", "de", "DE", "steuer_id", _steuer_id,
                     lambda v: Value(_chunks(v, (2, 3, 3, 3))), _bump_last_digit),
    IdentifierFamily("bsn", "BSN", "nl", "NL", "bsn", _bsn,
                     lambda v: Value((v,)), _bump_last_digit),
    IdentifierFamily("nhs", "NHSNUMBER", "en", "GB", "nhs", _nhs,
                     lambda v: Value(_chunks(v, (3, 3, 4))), _bump_last_digit),
    IdentifierFamily("cpf", "CPF", "pt", "BR", "cpf", _cpf,
                     lambda v: Value((f"{v[:3]}.{v[3:6]}.{v[6:9]}-{v[9:]}",)),
                     _bump_last_digit),
)
IDENTIFIER_FAMILY_BY_NAME = {family.name: family for family in IDENTIFIER_FAMILIES}


# --------------------------------------------------------------------------
# Name pools and every other partitioned vocabulary. Nothing here is shared
# between partitions; `test_agentic_layers.py` asserts disjointness.

GIVEN_NAMES = {
    "dev": ("Lena", "Jonas", "Mia", "Elias", "Laura", "Tim", "Nora", "Ben",
            "Grace", "Oliver", "Ruby", "Samuel"),
    "test": ("Anna", "Lukas", "Sophie", "Felix", "Marie", "Paul", "Hannah",
             "Leon", "Clara", "Henry", "Olivia", "Charlotte"),
}
SURNAMES = {
    "dev": ("Meyer", "Schulz", "Koch", "Richter", "Klein", "Wolf", "Schröder",
            "Neumann", "Walker", "Turner", "Parker", "Collins"),
    "test": ("Müller", "Schmidt", "Schneider", "Fischer", "Weber", "Wagner",
             "Becker", "Hoffmann", "Carter", "Bennett", "Hughes", "Foster"),
}
EMAIL_DOMAINS = {
    "dev": ("web.de", "outlook.com", "posteo.de"),
    "test": ("gmx.de", "proton.me", "yahoo.co.uk"),
}
US_AREA_CODES = {"dev": ("206", "303", "512", "617"), "test": ("212", "312", "415", "702")}
DE_MOBILE_PREFIXES = {"dev": ("151", "157", "176"), "test": ("152", "159", "178")}

# Cue words for prose_cue (and its NBSP perturbations), per family.
CUES: dict[str, dict[str, tuple[str, ...]]] = {
    "card": {"dev": ("card number", "credit card"), "test": ("Card no.", "card")},
    "iban": {"dev": ("IBAN", "bank account (IBAN)"), "test": ("IBAN", "account IBAN")},
    "steuer_id": {"dev": ("Steuer-ID", "tax ID"), "test": ("Steuer-ID", "Steueridentifikationsnummer")},
    "bsn": {"dev": ("BSN", "burgerservicenummer"), "test": ("BSN", "BSN-nummer")},
    "nhs": {"dev": ("NHS number", "NHS no."), "test": ("NHS number", "NHS No")},
    "cpf": {"dev": ("CPF", "CPF nº"), "test": ("CPF", "número do CPF")},
    "email": {"dev": ("email", "e-mail address"), "test": ("Email", "contact email")},
    "phone": {"dev": ("phone", "Tel."), "test": ("Phone", "mobile")},
    "dob": {"dev": ("DOB", "Geburtsdatum"), "test": ("Date of birth", "born on")},
}
# Machine keys for log_kv, csv and tool_json.
KEYS: dict[str, dict[str, tuple[str, ...]]] = {
    "card": {"dev": ("card_number", "pan"), "test": ("cardNumber", "credit_card")},
    "iban": {"dev": ("iban", "payout_iban"), "test": ("IBAN", "bank_iban")},
    "steuer_id": {"dev": ("steuer_id", "tax_id"), "test": ("steuerId", "steuer-id")},
    "bsn": {"dev": ("bsn", "citizen_bsn"), "test": ("BSN", "bsn_nummer")},
    "nhs": {"dev": ("nhs_number", "nhs"), "test": ("nhsNumber", "NHS_NO")},
    "cpf": {"dev": ("cpf", "cpf_numero"), "test": ("CPF", "documento_cpf")},
    "email": {"dev": ("email", "user_email"), "test": ("emailAddress", "contact_email")},
    "phone": {"dev": ("phone", "msisdn"), "test": ("phoneNumber", "mobile")},
    "dob": {"dev": ("dob", "birth_date"), "test": ("dateOfBirth", "geburtsdatum")},
    "name": {"dev": ("customer", "recipient"), "test": ("fullName", "account_holder")},
}

# {C} cue, {S} separator after the cue, {V} value, {K} key. The NBSP surfaces
# reuse the prose_cue template of their parent with {S} and the value's group
# separator replaced.
TEMPLATES: dict[str, dict[str, tuple[str, ...]]] = {
    "prose_cue": {
        "dev": (
            "Please update my records, my {C}:{S}{V}. Thanks!",
            "Can you check whether the {C}{S}{V} is still on file?",
        ),
        "test": (
            "Hi, for the refund use {C}:{S}{V} and confirm by reply.",
            "Following up on the ticket. {C}{S}{V} was entered twice.",
        ),
    },
    "prose_nocue": {
        "dev": (
            "I copied {V} from the old form, please keep it.",
            "Forwarding {V} as requested yesterday.",
        ),
        "test": (
            "The value {V} came through in the last message.",
            "Noted {V} in the handover, nothing else changed.",
        ),
    },
    "log_kv": {
        "dev": (
            "2026-03-02T10:14:22Z INFO profile.update {K}={V} status=ok",
            "level=warn msg=\"retrying\" {K}={V} attempt=2",
        ),
        "test": (
            "2026-04-17T08:03:51Z DEBUG sync.worker {K}={V} result=queued",
            "ts=1713341031 svc=billing {K}={V} outcome=accepted",
        ),
    },
    "csv": {
        "dev": ("row,{K},status\n1,{V},active\n", "{K};updated\n{V};2026-03-02\n"),
        "test": ("record_no,{K},state\n7,{V},pending\n", "{K},source\n{V},import\n"),
    },
    "tool_json": {
        "dev": ('{"action": "update_profile", "{K}": "{V}"}', '{"{K}": "{V}", "notify": true}'),
        "test": ('{"operation": "lookup", "{K}": "{V}", "limit": 1}', '{"{K}": "{V}"}'),
    },
}

NAME_TEMPLATES: dict[str, dict[str, tuple[str, ...]]] = {
    "prose_cue": {
        "dev": ("From: {G}{S}{N} <{E}>\nSubject: Invoice question\n\nHello, see below.",),
        "test": ("From: {G}{S}{N} <{E}>\nSubject: Delivery update\n\nHi team, quick note.",),
    },
    "prose_nocue": {
        "dev": ("Yesterday {G} {N} called about the delivery.",),
        "test": ("The parcel was signed for by {G} {N} this morning.",),
    },
    "log_kv": {
        "dev": ("level=info event=login {K}=\"{G} {N}\" ok=true",),
        "test": ("2026-04-17T08:03:51Z INFO mail.send {K}=\"{G} {N}\" queued=1",),
    },
    "csv": {
        "dev": ("row,{K}\n1,{G} {N}\n",),
        "test": ("record_no,{K},state\n7,{G} {N},pending\n",),
    },
    "tool_json": {
        "dev": ('{"action": "send_mail", "{K}": "{G} {N}"}',),
        "test": ('{"operation": "notify", "{K}": "{G} {N}", "channel": "email"}',),
    },
}

DOB_FORMATS = {
    "de": lambda y, m, d: f"{d:02d}.{m:02d}.{y}",
    "en": lambda y, m, d: f"{m:02d}/{d:02d}/{y}",
    "iso": lambda y, m, d: f"{y}-{m:02d}-{d:02d}",
}


# --------------------------------------------------------------------------
# Layer D lookalike families.


def _amount_eur(rng: Rng) -> str:
    return f"EUR {rng.between(10000, 99999)},{rng.digits(2)}"


def _amount_usd(rng: Rng) -> str:
    return f"${rng.between(10, 99)},{rng.digits(3)}.{rng.digits(2)}"


def _sku(rng: Rng) -> str:
    # A random 16-digit string passes Luhn one time in ten and is then
    # indistinguishable from a card, so SKUs are forced Luhn-invalid.
    while True:
        digits = rng.digits(16)
        if not luhn_valid(digits):
            return "-".join(_by_four(digits))


def _hex_colour(rng: Rng) -> str:
    return "#" + "".join(rng.choice("0123456789ABCDEF") for _ in range(6))


def _letter_digits(rng: Rng) -> str:
    return f"{rng.choice('ABCDEFGHJKLMNPRSTUVWXYZ')}{rng.digits(2)} {rng.digits(4)}"


def _version(rng: Rng) -> str:
    return f"v{rng.between(1, 9)}.{rng.between(0, 30)}.{rng.between(0, 20)}"


def _order_id(rng: Rng) -> str:
    return f"ORD-2026-{rng.digits(6)}"


def _tracking(rng: Rng) -> str:
    return "1Z" + "".join(rng.choice("0123456789ABCDEFGHJKLMNPRSTUVWXYZ") for _ in range(6)) + rng.digits(10)


def _uuid_fragment(rng: Rng) -> str:
    hex_digits = "0123456789abcdef"
    return "".join(rng.choice(hex_digits) for _ in range(8)) + "-" + "".join(
        rng.choice(hex_digits) for _ in range(4)
    )


def _room(rng: Rng) -> str:
    return f"Room {rng.between(1, 9)}.{rng.digits(3)}"


def _seat(rng: Rng) -> str:
    return f"Seat {rng.between(1, 45)}{rng.choice('ABCDEF')}"


def _invoice_date(rng: Rng) -> str:
    return f"2026-{rng.between(1, 12):02d}-{rng.between(1, 28):02d}"


def _log_timestamp(rng: Rng) -> str:
    return (
        f"2026-{rng.between(1, 12):02d}-{rng.between(1, 28):02d}T"
        f"{rng.between(0, 23):02d}:{rng.between(0, 59):02d}:{rng.between(0, 59):02d}Z"
    )


LOOKALIKE_FAMILIES: dict[str, tuple[str, str, Callable[[Rng], str]]] = {
    # family: (language, region, generator)
    "amount_eur": ("en", "US", _amount_eur),
    "amount_usd": ("en", "US", _amount_usd),
    "sku_4x4": ("en", "US", _sku),
    "hex_colour": ("en", "US", _hex_colour),
    "letter_digits": ("en", "GB", _letter_digits),
    "version": ("en", "US", _version),
    "order_id": ("de", "DE", _order_id),
    "tracking_id": ("en", "US", _tracking),
    "uuid_fragment": ("en", "US", _uuid_fragment),
    "room_number": ("de", "DE", _room),
    "seat_number": ("en", "GB", _seat),
    "invoice_date": ("de", "DE", _invoice_date),
    "log_timestamp": ("en", "US", _log_timestamp),
}
LOOKALIKE_KEYS = {
    "dev": {
        "amount_eur": "total", "amount_usd": "amount", "sku_4x4": "sku",
        "hex_colour": "color", "letter_digits": "part_no", "version": "version",
        "order_id": "order", "tracking_id": "tracking", "uuid_fragment": "trace",
        "room_number": "location", "seat_number": "seat", "invoice_date": "invoice_date",
        "log_timestamp": "timestamp",
    },
    "test": {
        "amount_eur": "grandTotal", "amount_usd": "price", "sku_4x4": "itemCode",
        "hex_colour": "accentColour", "letter_digits": "partNumber", "version": "release",
        "order_id": "orderRef", "tracking_id": "trackingNumber", "uuid_fragment": "requestId",
        "room_number": "meetingRoom", "seat_number": "seatAssignment",
        "invoice_date": "invoiceDate", "log_timestamp": "occurredAt",
    },
}
LOOKALIKE_TEMPLATES = {
    "prose": {
        "dev": ("The dashboard shows {V} for this item.", "We logged {V} in the report."),
        "test": ("According to the sheet the entry is {V} today.", "Reference: {V}, nothing else to add."),
    },
    "log_kv": {
        "dev": ("2026-03-02T10:14:22Z INFO shop.cart {K}={V} ok=1",),
        "test": ("level=info svc=orders {K}={V} status=done",),
    },
    "tool_json": {
        "dev": ('{"action": "update_item", "{K}": "{V}"}',),
        "test": ('{"operation": "fetch", "{K}": "{V}", "page": 1}',),
    },
}


# --------------------------------------------------------------------------
# Document construction with gold offsets tracked as text is appended.


@dataclass(frozen=True)
class Gold:
    start: int
    end: int
    label: str
    value: str


class _Builder:
    def __init__(self) -> None:
        self._parts: list[str] = []
        self._length = 0
        self._gold: list[tuple[int, int, str, str]] = []

    def text(self, value: str) -> None:
        self._parts.append(value)
        self._length += len(value)

    def gold(self, value: str, label: str) -> None:
        self._gold.append((self._length, self._length + len(value), label, value))
        self.text(value)

    def build(self) -> tuple[str, tuple[Gold, ...]]:
        text = "".join(self._parts)
        offsets = score.char_to_byte_offsets(text)
        return text, tuple(
            Gold(offsets[start], offsets[end], label, value)
            for start, end, label, value in self._gold
        )


def _fill(template: str, fields: Mapping[str, tuple[str, str | None]]) -> tuple[str, tuple[Gold, ...]]:
    """Expand {X} placeholders; fields map X -> (text, gold label or None)."""
    builder = _Builder()
    index = 0
    while index < len(template):
        opening = template.find("{", index)
        if opening == -1:
            builder.text(template[index:])
            break
        closing = template.find("}", opening)
        name = template[opening + 1 : closing]
        if closing == -1 or name not in fields:
            # A literal brace (JSON) rather than a placeholder.
            builder.text(template[index : opening + 1])
            index = opening + 1
            continue
        builder.text(template[index:opening])
        value, label = fields[name]
        if label is None:
            builder.text(value)
        else:
            builder.gold(value, label)
        index = closing + 1
    return builder.build()


@dataclass(frozen=True)
class Record:
    uid: str
    partition: str
    layer: str
    family: str
    surface: str
    validity: str
    group: str
    template: str
    language: str
    region: str
    text: str
    gold: tuple[Gold, ...]

    @property
    def cell(self) -> str:
        return f"{self.layer}|{self.family}|{self.surface}|{self.validity}"

    def to_json(self) -> dict[str, object]:
        return {
            "id": self.uid,
            "partition": self.partition,
            "layer": self.layer,
            "family": self.family,
            "surface": self.surface,
            "validity": self.validity,
            "group": self.group,
            "template": self.template,
            "language": self.language,
            "region": self.region,
            "text": self.text,
            "gold": [
                {"start": g.start, "end": g.end, "label": g.label, "value": g.value}
                for g in self.gold
            ],
        }

    def to_document(self) -> score.Document:
        return score.Document(
            uid=self.uid,
            text=self.text,
            language=self.language,
            region=self.region,
            source_dataset=SOURCE_DATASET,
            spans=tuple(score.Span(g.start, g.end, g.label) for g in self.gold),
            negative_category=self.family if self.layer == LAYER_LOOKALIKES else None,
            cell=self.cell,
        )


def _template_id(surface: str, partition: str, template: str, pool: Mapping[str, Mapping[str, Sequence[str]]]) -> str:
    return f"{surface}/{partition}/{pool[surface][partition].index(template)}"


def _cue_family(family: str) -> str:
    if family.startswith("iban"):
        return "iban"
    if family.startswith("phone"):
        return "phone"
    return family


def _identifier_records(partition: str, seed: int) -> list[Record]:
    records: list[Record] = []
    for family in IDENTIFIER_FAMILIES:
        rng = Rng(seed, f"A/{family.name}")
        cue_family = _cue_family(family.name)
        for index in range(DOCS_PER_FAMILY):
            raw = family.make(rng)
            checker = CHECKSUMS[family.checksum] if family.checksum else None
            if checker is not None and not checker(raw):
                raise LayerError(f"generated {family.name} value fails its checksum")
            variants = [(VALID, raw)]
            if family.twin is not None:
                twin = family.twin(raw)
                if checker is not None and checker(twin):
                    raise LayerError(f"{family.name} twin unexpectedly passes its checksum")
                variants.append((INVALID, twin))
            group = f"{partition}-A-{family.name}-{index:03d}"
            choices = {
                surface: rng.choice(TEMPLATES[surface][partition])
                for surface in SURFACES
                if surface not in PERTURBED_SURFACES
            }
            cue = rng.choice(CUES[cue_family][partition])
            key = rng.choice(KEYS[cue_family][partition])
            for validity, raw_value in variants:
                value = family.display(raw_value)
                for surface in SURFACES:
                    base_surface = "prose_cue" if surface in PERTURBED_SURFACES else surface
                    template = choices[base_surface]
                    separator = PERTURBED_SURFACES.get(surface, " ")
                    rendered = value.render(separator)
                    text, gold = _fill(
                        template,
                        {
                            "C": (cue, None),
                            "S": (separator, None),
                            "K": (key, None),
                            "V": (rendered, family.label),
                        },
                    )
                    records.append(
                        Record(
                            uid=f"agentic-{partition}-A-{family.name}-{index:03d}-{surface}-{validity}",
                            partition=partition,
                            layer=LAYER_IDENTIFIERS,
                            family=family.name,
                            surface=surface,
                            validity=validity,
                            group=group,
                            template=_template_id(base_surface, partition, template, TEMPLATES),
                            language=family.language,
                            region=family.region,
                            text=text,
                            gold=gold,
                        )
                    )
    records.extend(_unchecked_records(partition, seed))
    return records


def _person(rng: Rng, partition: str) -> tuple[str, str]:
    given = rng.choice(GIVEN_NAMES[partition])
    surname = rng.choice(SURNAMES[partition])
    if rng.below(3) == 0:
        second = rng.choice([name for name in SURNAMES[partition] if name != surname])
        surname = f"{surname}-{second}"
    return given, surname


def _ascii_fold(value: str) -> str:
    table = {"ä": "ae", "ö": "oe", "ü": "ue", "ß": "ss", "Ä": "Ae", "Ö": "Oe", "Ü": "Ue"}
    return "".join(table.get(character, character) for character in value)


def _email(rng: Rng, partition: str) -> str:
    given, surname = _person(rng, partition)
    local = _ascii_fold(f"{given}.{surname}").lower()
    if rng.below(2):
        local += rng.digits(2)
    return f"{local}@{rng.choice(EMAIL_DOMAINS[partition])}"


def _phone_de(rng: Rng, partition: str) -> Value:
    return Value(("+49", rng.choice(DE_MOBILE_PREFIXES[partition]), rng.digits(4), rng.digits(4)))


def _phone_us(rng: Rng, partition: str) -> Value:
    exchange = str(rng.between(2, 9)) + rng.digits(2)
    while exchange[1:] == "11" or exchange == "555":
        exchange = str(rng.between(2, 9)) + rng.digits(2)
    return Value(("+1", rng.choice(US_AREA_CODES[partition]), exchange, rng.digits(4)))


def _unchecked_records(partition: str, seed: int) -> list[Record]:
    """Families without a checksum: no invalid twin exists for them."""
    records: list[Record] = []
    plans = (
        ("email", "EMAIL", "email"),
        ("phone_de", "TELEPHONENUM", "phone"),
        ("phone_us", "TELEPHONENUM", "phone"),
        ("dob", "DATEOFBIRTH", "dob"),
        ("header_name", None, "name"),
    )
    locales = {
        "email": ("en", "US"), "phone_de": ("de", "DE"), "phone_us": ("en", "US"),
    }
    for family, label, cue_family in plans:
        rng = Rng(seed, f"A/{family}")
        for index in range(DOCS_PER_FAMILY):
            group = f"{partition}-A-{family}-{index:03d}"
            language, region = locales.get(family, ("de", "DE") if index % 2 else ("en", "US"))
            if family == "header_name":
                given, surname = _person(rng, partition)
                email = _email(rng, partition)
                pool = NAME_TEMPLATES
                choices = {s: rng.choice(NAME_TEMPLATES[s][partition]) for s in NAME_TEMPLATES}
                key = rng.choice(KEYS["name"][partition])
                cue = ""
            else:
                pool = TEMPLATES
                if family == "email":
                    value = Value((_email(rng, partition),), joiner="")
                elif family == "phone_de":
                    value = _phone_de(rng, partition)
                elif family == "phone_us":
                    value = _phone_us(rng, partition)
                else:
                    year, month, day = rng.between(1940, 2005), rng.between(1, 12), rng.between(1, 28)
                    style = "de" if language == "de" else "en"
                    date_text = DOB_FORMATS[style](year, month, day)
                    iso_text = DOB_FORMATS["iso"](year, month, day)
                choices = {s: rng.choice(TEMPLATES[s][partition]) for s in TEMPLATES}
                cue = rng.choice(CUES[cue_family][partition])
                key = rng.choice(KEYS[cue_family][partition])
            for surface in SURFACES:
                base_surface = "prose_cue" if surface in PERTURBED_SURFACES else surface
                template = choices[base_surface]
                separator = PERTURBED_SURFACES.get(surface, " ")
                if family == "header_name":
                    fields = {
                        "G": (given, "GIVENNAME"),
                        "N": (surname, "SURNAME"),
                        "E": (email, "EMAIL"),
                        "S": (separator, None),
                        "K": (key, None),
                    }
                elif family == "dob":
                    # Machine surfaces carry ISO dates; prose keeps the locale style.
                    machine = base_surface in ("log_kv", "csv", "tool_json")
                    fields = {
                        "C": (cue, None),
                        "S": (separator, None),
                        "K": (key, None),
                        "V": (iso_text if machine else date_text, label),
                    }
                else:
                    fields = {
                        "C": (cue, None),
                        "S": (separator, None),
                        "K": (key, None),
                        "V": (value.render(separator), label),
                    }
                text, gold = _fill(template, fields)
                records.append(
                    Record(
                        uid=f"agentic-{partition}-A-{family}-{index:03d}-{surface}-{UNCHECKED}",
                        partition=partition,
                        layer=LAYER_IDENTIFIERS,
                        family=family,
                        surface=surface,
                        validity=UNCHECKED,
                        group=group,
                        template=_template_id(base_surface, partition, template, pool),
                        language=language,
                        region=region,
                        text=text,
                        gold=gold,
                    )
                )
    return records


def _lookalike_records(partition: str, seed: int) -> list[Record]:
    records: list[Record] = []
    for family, (language, region, make) in LOOKALIKE_FAMILIES.items():
        rng = Rng(seed, f"D/{family}")
        key = LOOKALIKE_KEYS[partition][family]
        for index in range(DOCS_PER_FAMILY):
            value = make(rng)
            group = f"{partition}-D-{family}-{index:03d}"
            for surface in LOOKALIKE_SURFACES:
                template = rng.choice(LOOKALIKE_TEMPLATES[surface][partition])
                text, gold = _fill(template, {"K": (key, None), "V": (value, None)})
                records.append(
                    Record(
                        uid=f"agentic-{partition}-D-{family}-{index:03d}-{surface}-{BENIGN}",
                        partition=partition,
                        layer=LAYER_LOOKALIKES,
                        family=family,
                        surface=surface,
                        validity=BENIGN,
                        group=group,
                        template=_template_id(surface, partition, template, LOOKALIKE_TEMPLATES),
                        language=language,
                        region=region,
                        text=text,
                        gold=gold,
                    )
                )
    return records


def generate(partition: str) -> list[Record]:
    if partition not in PARTITIONS:
        raise LayerError(f"unknown partition {partition!r}")
    seed = PARTITION_SEEDS[partition]
    records = _identifier_records(partition, seed) + _lookalike_records(partition, seed)
    for record in records:
        encoded = record.text.encode("utf-8")
        for gold in record.gold:
            if encoded[gold.start : gold.end].decode("utf-8") != gold.value:
                raise LayerError(f"{record.uid}: gold offsets do not select the inserted value")
    if len({record.uid for record in records}) != len(records):
        raise LayerError("generated document IDs are not unique")
    return records


def corpus_bytes(records: Iterable[Record]) -> bytes:
    return b"".join(
        json.dumps(record.to_json(), ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
        + b"\n"
        for record in records
    )


def manifest(partition: str, records: Sequence[Record]) -> dict[str, object]:
    layers: dict[str, int] = defaultdict(int)
    for record in records:
        layers[record.layer] += 1
    return {
        "generator": "scripts/bench/agentic_layers.py",
        "generator_version": GENERATOR_VERSION,
        "partition": partition,
        "seed": PARTITION_SEEDS[partition],
        "docs_per_family": DOCS_PER_FAMILY,
        "documents": len(records),
        "documents_by_layer": dict(sorted(layers.items())),
        "corpus_sha256": hashlib.sha256(corpus_bytes(records)).hexdigest(),
        "synthetic_only": True,
    }


# --------------------------------------------------------------------------
# Scored-label contract for the generated corpus.


def load_contract(repo_root: Path, path: Path | None = None) -> score.ScoredLabelContract:
    relative = path or SCORED_LABELS_PATH
    resolved = relative if relative.is_absolute() else repo_root / relative
    try:
        display = resolved.resolve().relative_to(repo_root.resolve()).as_posix()
    except ValueError:
        display = resolved.as_posix()
    try:
        contract = score.load_scored_label_contract(resolved, display_path=display)
        raw = json.loads(resolved.read_text(encoding="utf-8"))
    except (score.ScoredLabelContractError, OSError, json.JSONDecodeError) as error:
        raise LayerError(str(error)) from error
    corpus = raw.get("corpus")
    if not isinstance(corpus, dict) or corpus.get("generator_version") != GENERATOR_VERSION:
        raise LayerError(
            f"{display} rules on generator_version {corpus.get('generator_version') if isinstance(corpus, dict) else None!r}, "
            f"but the generator is version {GENERATOR_VERSION}"
        )
    return contract


def apply_contract(
    documents: Sequence[score.Document], contract: score.ScoredLabelContract
) -> list[score.Document]:
    """Fail closed on an unruled label and on a ruling for a label never generated."""
    assert contract.scored_labels is not None
    generated = {span.label for document in documents for span in document.spans}
    stale = sorted((contract.scored_labels | contract.excluded_labels) - generated)
    if stale:
        raise LayerError(f"{contract.contract_id} rules on labels the generator never emits: {stale}")
    try:
        return score.apply_scored_label_contract(documents, contract)
    except score.ScoredLabelContractError as error:
        raise LayerError(str(error)) from error


@dataclass(frozen=True)
class PreparedLayers:
    manifest: dict[str, object]
    contract: score.ScoredLabelContract
    identifiers: list[score.Document]
    lookalikes: list[score.Document]


def prepare(repo_root: Path, contract_path: Path | None = None) -> PreparedLayers:
    records = generate(PUBLISHED_PARTITION)
    contract = load_contract(repo_root, contract_path)
    documents = apply_contract([record.to_document() for record in records], contract)
    identifiers = [d for d in documents if d.cell and d.cell.startswith(LAYER_IDENTIFIERS + "|")]
    lookalikes = [d for d in documents if d.cell and d.cell.startswith(LAYER_LOOKALIKES + "|")]
    if any(d.spans for d in lookalikes):
        raise LayerError("a layer D lookalike document carries gold")
    return PreparedLayers(manifest(PUBLISHED_PARTITION, records), contract, identifiers, lookalikes)


# --------------------------------------------------------------------------
# Reading results back: the coverage grid and the rule gate.


def _layer_run(scorecard: Mapping[str, object], layer: str, config: str) -> Mapping[str, object]:
    if layer == "C":
        runs = scorecard.get("runs")
    else:
        layers = scorecard.get("layers")
        if not isinstance(layers, dict) or layer not in layers:
            raise LayerError(f"scorecard has no layer {layer}")
        runs = layers[layer].get("runs")
    for run in runs or []:
        if run.get("config") == config:
            return run
    raise LayerError(f"layer {layer} has no run for config {config!r}")


def production_config(scorecard: Mapping[str, object]) -> str:
    configs = scorecard.get("parameters", {}).get("configs", [])
    if configs == ["policy-file"]:
        return "policy-file"
    return score.PRODUCTION_CONFIG


def coverage_grid(scorecard: Mapping[str, object], config: str | None = None) -> str:
    """Markdown byte-recall grid, family x surface, for layer A (valid or unchecked
    gold, then checksum-invalid twins) plus layer D false-positive bytes."""
    config = config or production_config(scorecard)
    lines = [f"Config `{config}`. Cells: byte recall of gold (1.00 = every byte protected); `-` = not generated.", ""]
    run_a = _layer_run(scorecard, LAYER_IDENTIFIERS, config)
    cells = run_a.get("per_cell", {})
    for title, validities in (("Valid or unchecked gold", (VALID, UNCHECKED)), ("Checksum-invalid twins", (INVALID,))):
        families = [f.name for f in IDENTIFIER_FAMILIES] + ["email", "phone_de", "phone_us", "dob", "header_name"]
        lines += [f"**Layer A, {title}**", "", "| Family | " + " | ".join(SURFACES) + " |", "| --- |" + " ---: |" * len(SURFACES)]
        for family in families:
            row = []
            present = False
            for surface in SURFACES:
                block = next(
                    (cells[f"A|{family}|{surface}|{v}"] for v in validities if f"A|{family}|{surface}|{v}" in cells),
                    None,
                )
                if block is None:
                    row.append("-")
                    continue
                present = True
                row.append(f"{block['utf8_bytes']['recall']:.2f}")
            if present:
                lines.append(f"| {family} | " + " | ".join(row) + " |")
        lines.append("")
    run_d = _layer_run(scorecard, LAYER_LOOKALIKES, config)
    d_cells = run_d.get("per_cell", {})
    lines += ["**Layer D, false-positive bytes**", "", "| Family | " + " | ".join(LOOKALIKE_SURFACES) + " |", "| --- |" + " ---: |" * len(LOOKALIKE_SURFACES)]
    for family in LOOKALIKE_FAMILIES:
        row = []
        for surface in LOOKALIKE_SURFACES:
            block = d_cells.get(f"D|{family}|{surface}|{BENIGN}")
            row.append("-" if block is None else str(block["utf8_bytes"]["false_positive"]))
        lines.append(f"| {family} | " + " | ".join(row) + " |")
    return "\n".join(lines) + "\n"


GATE_LAYERS = ("C", LAYER_IDENTIFIERS, LAYER_LOOKALIKES)


def _layer_identity(scorecard: Mapping[str, object]) -> dict[str, object]:
    layers = scorecard.get("layers")
    if not isinstance(layers, dict):
        raise LayerError(
            "scorecard has no agentic layers: it predates them or was run with "
            "--no-agentic-layers; measure the base again on this harness"
        )
    return {
        "kiji_contract": score.scorecard_scored_label_contract_identity(scorecard),
        "kiji_dataset": scorecard.get("dataset", {}).get("integrity"),
        "corpus_sha256": layers.get("generator", {}).get("corpus_sha256"),
        "layer_contract": layers.get("scored_label_contract", {}).get("file_sha256"),
        "configs": scorecard.get("parameters", {}).get("configs"),
        "policy_sha256": scorecard.get("parameters", {}).get("policy_sha256"),
    }


def gate(base: Mapping[str, object], candidate: Mapping[str, object], config: str | None = None) -> dict[str, object]:
    """Rule gate: no layer leaks more, none fails closed on more documents, and
    at least one layer leaks less; or, for an FP-only fix, leaks are unchanged
    everywhere and at least one layer's false-positive bytes fall."""
    base_identity = _layer_identity(base)
    candidate_identity = _layer_identity(candidate)
    if base_identity != candidate_identity:
        differing = sorted(k for k in base_identity if base_identity[k] != candidate_identity[k])
        return {"verdict": "not_comparable", "differing": differing, "layers": {}}
    config = config or production_config(candidate)
    rows: dict[str, dict[str, int]] = {}
    for layer in GATE_LAYERS:
        before = _layer_run(base, layer, config)
        after = _layer_run(candidate, layer, config)
        rows[layer] = {
            "leaked_base": before["metrics"]["utf8_bytes"]["leaked"],
            "leaked_candidate": after["metrics"]["utf8_bytes"]["leaked"],
            "false_positive_base": before["metrics"]["utf8_bytes"]["false_positive"],
            "false_positive_candidate": after["metrics"]["utf8_bytes"]["false_positive"],
            "failed_closed_base": before["pipeline_availability"]["failed_closed_documents"],
            "failed_closed_candidate": after["pipeline_availability"]["failed_closed_documents"],
        }
    leak_rise = [l for l, r in rows.items() if r["leaked_candidate"] > r["leaked_base"]]
    refusal_rise = [l for l, r in rows.items() if r["failed_closed_candidate"] > r["failed_closed_base"]]
    leak_fall = [l for l, r in rows.items() if r["leaked_candidate"] < r["leaked_base"]]
    fp_fall = [l for l, r in rows.items() if r["false_positive_candidate"] < r["false_positive_base"]]
    if leak_rise or refusal_rise:
        verdict = "fail"
        reason = f"leaked bytes rose in {leak_rise}" if leak_rise else f"failed-closed documents rose in {refusal_rise}"
    elif leak_fall:
        verdict, reason = "pass", f"leaked bytes fell in {leak_fall}"
    elif fp_fall:
        verdict, reason = "pass", f"false-positive-only fix: FP bytes fell in {fp_fall}"
    else:
        verdict, reason = "fail", "no layer's leaked or false-positive bytes fell"
    return {"verdict": verdict, "reason": reason, "config": config, "layers": rows}


def gate_markdown(result: Mapping[str, object]) -> str:
    lines = [f"Verdict: **{result['verdict']}** ({result.get('reason', result.get('differing'))})", ""]
    if result["layers"]:
        lines += ["| Layer | Leaked base | Leaked cand | FP base | FP cand | Failed closed base | Failed closed cand |",
                  "| --- | ---: | ---: | ---: | ---: | ---: | ---: |"]
        for layer, r in result["layers"].items():
            lines.append(
                f"| {layer} | {r['leaked_base']} | {r['leaked_candidate']} | {r['false_positive_base']} | "
                f"{r['false_positive_candidate']} | {r['failed_closed_base']} | {r['failed_closed_candidate']} |"
            )
    return "\n".join(lines) + "\n"


# --------------------------------------------------------------------------


def _load_json(path: Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    generate_cmd = commands.add_parser("generate", help="write one partition as JSONL")
    generate_cmd.add_argument("--partition", choices=PARTITIONS, required=True)
    generate_cmd.add_argument("--output", type=Path, required=True)
    commands.add_parser("manifest", help="print both partitions' manifests")
    grid_cmd = commands.add_parser("grid", help="print the coverage grid of a scorecard")
    grid_cmd.add_argument("scorecard", type=Path)
    grid_cmd.add_argument("--config")
    gate_cmd = commands.add_parser("gate", help="apply the rule gate to a base/candidate pair")
    gate_cmd.add_argument("--base", type=Path, required=True)
    gate_cmd.add_argument("--candidate", type=Path, required=True)
    gate_cmd.add_argument("--config")
    args = parser.parse_args(argv)
    try:
        if args.command == "generate":
            records = generate(args.partition)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_bytes(corpus_bytes(records))
            print(json.dumps(manifest(args.partition, records), indent=2))
        elif args.command == "manifest":
            print(json.dumps({p: manifest(p, generate(p)) for p in PARTITIONS}, indent=2))
        elif args.command == "grid":
            print(coverage_grid(_load_json(args.scorecard), args.config), end="")
        else:
            result = gate(_load_json(args.base), _load_json(args.candidate), args.config)
            print(gate_markdown(result), end="")
            return {"pass": 0, "fail": 1}.get(str(result["verdict"]), 2)
    except LayerError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
