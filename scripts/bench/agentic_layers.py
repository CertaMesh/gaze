#!/usr/bin/env python3
"""Gaze-native agentic benchmark layers A (identifiers) and D (benign lookalikes).

Layer A renders identifiers into the shapes agents send (prose with and without
a cue, NBSP and NARROW NBSP spacing, log `key=value`, CSV, proxy-shaped
tool-call JSON). Every checksum family also gets a checksum-invalid twin in the
same shape; the twin stays scored gold, and the validator gold census reports
the split. Layer D renders benign lookalikes (amounts, SKUs, colours, versions,
order and tracking IDs, dates) that must stay untouched. Both layers also
carry labelled postcodes and phones inside benign-looking structures (A) and
the same structures with no cue anywhere (D), which price a benign-lookalike
veto in both directions.

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
import tomllib
from collections import defaultdict
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Callable, Iterable, Mapping, Sequence

import gaze_bench_score as score
import government_id_cells as government_ids


GENERATOR_VERSION = 9
PARTITIONS = ("dev", "test")
PUBLISHED_PARTITION = "test"
PARTITION_SEEDS = {"dev": 2026092601, "test": 2026092602}
DOCS_PER_FAMILY = 10
SCORED_LABELS_PATH = Path("docs/reference/benchmarks/scored-labels-agentic.json")
LAYER_IDENTIFIERS = "A"
LAYER_LOOKALIKES = "D"
LAYER_REPEATS = "R"
REPEAT = "repeat"
DECOY_PREFIX = "decoy:"
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
# Gate credit contract v2 (user rulings 2026-09-27 and 2026-09-28).
GATE_CREDIT_VERSION = 2


class CreditScope(str, Enum):
    ALL = "all"
    CUED = "cued"


CREDIT_SCOPE_BY_LABEL: dict[str, CreditScope] = {
    "IBAN": CreditScope.ALL,
    "CREDITCARDNUMBER": CreditScope.ALL,
    "PHONENUMBER": CreditScope.CUED,
    "TAXNUM": CreditScope.CUED,
    "CPF": CreditScope.CUED,
    "BSN": CreditScope.CUED,
    "NHSNUMBER": CreditScope.CUED,
}
CUED_SURFACES = (frozenset(SURFACES) - {"prose_nocue"}) | {
    "gov_prose", "gov_log_kv", "gov_tool_json", "gov_tool_result",
}


def invalid_twin_credited(label: str | None, surface: str) -> bool:
    """Layer A credit requires a cued surface except for IBAN and card."""
    scope = CREDIT_SCOPE_BY_LABEL.get(label or "")
    return scope == CreditScope.ALL or (scope == CreditScope.CUED and surface in CUED_SURFACES)


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
    "iban": {"dev": ("iban", "payout_iban"), "test": ("ibanNumber", "bank_iban")},
    "steuer_id": {"dev": ("steuer_id", "tax_id"), "test": ("steuerId", "taxIdentNr")},
    "bsn": {"dev": ("bsn", "citizen_bsn"), "test": ("bsnNumber", "bsn_nummer")},
    "nhs": {"dev": ("nhs_number", "nhs"), "test": ("nhsNumber", "NHS_NO")},
    "cpf": {"dev": ("cpf", "cpf_numero"), "test": ("cpfNumber", "documento_cpf")},
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


def _reference_digits(rng: Rng, count: int, invalid_for: Sequence[Callable[[str], bool]]) -> str:
    """A reference number no checksum of a same-length identifier accepts."""
    while True:
        digits = str(rng.between(1, 9)) + rng.digits(count - 1)
        if not any(check(digits) for check in invalid_for):
            return digits


def _ref_number_9(rng: Rng, index: int) -> str:
    return _reference_digits(rng, 9, (bsn_valid,))


def _ref_number_10(rng: Rng, index: int) -> str:
    digits = _reference_digits(rng, 10, (nhs_valid,))
    return digits if index % 2 == 0 else " ".join(_chunks(digits, (3, 3, 4)))


def _ref_number_11(rng: Rng, index: int) -> str:
    digits = _reference_digits(rng, 11, (steuer_id_valid, cpf_valid))
    shape = index % 3
    if shape == 0:
        return digits
    if shape == 1:
        return " ".join(_chunks(digits, (2, 3, 3, 3)))
    return f"{digits[:3]}.{digits[3:6]}.{digits[6:9]}-{digits[9:]}"


def _ref_number_16(rng: Rng, index: int) -> str:
    """A space-grouped 16-digit voucher code that fails Luhn, the card twin's shape."""
    while True:
        digits = str(rng.between(1, 9)) + rng.digits(15)
        if not luhn_valid(digits):
            return " ".join(_by_four(digits))


def _local_date(rng: Rng, index: int) -> str:
    """A non-birth date in a DOB display format: a recent or near-future year."""
    year, month, day = rng.between(2024, 2027), rng.between(1, 12), rng.between(1, 28)
    style = "de" if index % 2 == 0 else "en"
    return DOB_FORMATS[style](year, month, day)


# Counterweights take the document index, so each display shape of the gold
# they balance is generated deterministically.
INDEXED_LOOKALIKE_FAMILIES: dict[str, tuple[str, str, Callable[[Rng, int], str]]] = {
    "ref_number_9": ("nl", "NL", _ref_number_9),
    "ref_number_10": ("en", "GB", _ref_number_10),
    "ref_number_11": ("de", "DE", _ref_number_11),
    "ref_number_16": ("en", "US", _ref_number_16),
    "local_date": ("de", "DE", _local_date),
}

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
        "log_timestamp": "timestamp", "ref_number_9": "customer_no",
        "ref_number_10": "ticket", "ref_number_11": "invoice_no", "ref_number_16": "voucher",
        "local_date": "delivery_date",
    },
    "test": {
        "amount_eur": "grandTotal", "amount_usd": "price", "sku_4x4": "itemCode",
        "hex_colour": "accentColour", "letter_digits": "partNumber", "version": "release",
        "order_id": "orderRef", "tracking_id": "trackingNumber", "uuid_fragment": "requestId",
        "room_number": "meetingRoom", "seat_number": "seatAssignment",
        "invoice_date": "invoiceDate", "log_timestamp": "occurredAt",
        "ref_number_9": "customerNumber", "ref_number_10": "caseId",
        "ref_number_11": "invoiceNumber", "ref_number_16": "voucherCode",
        "local_date": "dueDate",
    },
}
# Prose for the date counterweight: a delivery or due date, never a birth date.
LOCAL_DATE_PROSE = {
    "dev": ("Die Lieferung kommt am {V}.", "Payment is due {V}."),
    "test": ("Lieferung am {V} bestätigt.", "The invoice is due {V}."),
}

# M1: gold that only a context-free rule can reach (no cue, and no checksum or
# a failing one) needs a layer D family of the same display shape. Otherwise a
# rule that tags every such shape lowers layer A's leak at no visible FP cost.
# Keys are (family, surface, validity) cells of layer A.
COUNTERWEIGHTS: dict[tuple[str, str, str], str] = {
    ("bsn", "prose_nocue", INVALID): "ref_number_9",
    ("nhs", "prose_nocue", INVALID): "ref_number_10",
    ("steuer_id", "prose_nocue", INVALID): "ref_number_11",
    ("cpf", "prose_nocue", INVALID): "ref_number_11",
    ("card", "prose_nocue", INVALID): "ref_number_16",
    ("dob", "prose_nocue", UNCHECKED): "local_date",
}
# Context-free cells with no counterweight, and why none is needed.
COUNTERWEIGHT_EXEMPT: dict[tuple[str, str, str], str] = {
    (family, "prose_nocue", INVALID): (
        "an IBAN shape (country code, check digits, BBAN) that fails mod-97 has no "
        "common benign use, so a shape-only IBAN rule has no FP to measure"
    )
    for family in ("iban_de", "iban_de_compact", "iban_at", "iban_nl", "iban_fr", "iban_gb")
}


# The counterweight families that price a checksum-less rule for each label in
# CREDIT_SCOPE_BY_LABEL. Crediting invalid gold must never pay for false
# positives on its benign twin shape, so `decide` fails any rise there with no
# net-bytes offset. Derived from COUNTERWEIGHTS; IBAN and phone have no layer D
# counterweight (IBAN is exempt; phone has no layer A family).
CREDIT_GUARD_FAMILIES: dict[str, tuple[str, ...]] = {
    label: tuple(sorted({
        counterweight
        for (family, _surface, validity), counterweight in COUNTERWEIGHTS.items()
        if validity == INVALID
        and next(f.label for f in IDENTIFIER_FAMILIES if f.name == family) == label
    }))
    for label in sorted(CREDIT_SCOPE_BY_LABEL)
}


def is_context_free_only(family: str, surface: str, validity: str) -> bool:
    """Layer A gold that no cue and no passing validator can anchor."""
    return surface == "prose_nocue" and (validity == INVALID or family == "dob")


def display_shape(value: str) -> str:
    """Digits as 9, letters as A, every other character as written.

    Separators stay exact: a rule for `9999 9999` never sees `9999-9999`, so a
    counterweight has to carry the gold's own separators.
    """
    return "".join(
        "9" if character.isdigit() else "A" if character.isalpha() else character
        for character in value
    )


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
        self._decoys: list[tuple[int, int, str, str]] = []

    def text(self, value: str) -> None:
        self._parts.append(value)
        self._length += len(value)

    def gold(self, value: str, label: str) -> None:
        self._gold.append((self._length, self._length + len(value), label, value))
        self.text(value)

    def decoy(self, value: str, kind: str) -> None:
        self._decoys.append((self._length, self._length + len(value), kind, value))
        self.text(value)

    def build(self) -> tuple[str, tuple[Gold, ...], tuple[Gold, ...]]:
        text = "".join(self._parts)
        offsets = score.char_to_byte_offsets(text)

        def spans(items: list[tuple[int, int, str, str]]) -> tuple[Gold, ...]:
            return tuple(
                Gold(offsets[start], offsets[end], label, value)
                for start, end, label, value in items
            )

        return text, spans(self._gold), spans(self._decoys)


def _fill(template: str, fields: Mapping[str, tuple[str, str | None]]) -> tuple[str, tuple[Gold, ...]]:
    """Expand {X} placeholders; fields map X -> (text, gold label or None)."""
    text, gold, decoys = _fill_with_decoys(template, fields)
    assert not decoys
    return text, gold


def _fill_with_decoys(
    template: str, fields: Mapping[str, tuple[str, str | None]]
) -> tuple[str, tuple[Gold, ...], tuple[Gold, ...]]:
    """As `_fill`; a label starting with DECOY_PREFIX marks a recorded non-gold span."""
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
        elif label.startswith(DECOY_PREFIX):
            builder.decoy(value, label[len(DECOY_PREFIX) :])
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
    # Recorded non-gold spans (layer R): ordinary words and digit runs that
    # collide with a gold value. Any coverage of them is a false positive.
    decoys: tuple[Gold, ...] = ()

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
            **(
                {
                    "decoys": [
                        {"start": d.start, "end": d.end, "kind": d.label, "value": d.value}
                        for d in self.decoys
                    ]
                }
                if self.decoys
                else {}
            ),
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
    makers: dict[str, tuple[str, str, Callable[[Rng, int], str]]] = {
        family: (language, region, lambda rng, index, make=make: make(rng))
        for family, (language, region, make) in LOOKALIKE_FAMILIES.items()
    }
    makers.update(INDEXED_LOOKALIKE_FAMILIES)
    for family, (language, region, make) in makers.items():
        rng = Rng(seed, f"D/{family}")
        key = LOOKALIKE_KEYS[partition][family]
        for index in range(DOCS_PER_FAMILY):
            value = make(rng, index)
            group = f"{partition}-D-{family}-{index:03d}"
            for surface in LOOKALIKE_SURFACES:
                pool = (
                    LOCAL_DATE_PROSE
                    if family == "local_date" and surface == "prose"
                    else LOOKALIKE_TEMPLATES[surface]
                )
                template = rng.choice(pool[partition])
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
                        template=(
                            f"local_date_prose/{partition}/{pool[partition].index(template)}"
                            if pool is LOCAL_DATE_PROSE
                            else _template_id(surface, partition, template, LOOKALIKE_TEMPLATES)
                        ),
                        language=language,
                        region=region,
                        text=text,
                        gold=gold,
                    )
                )
    return records


# A consuming regex guard can eat the only separator before its neighbour.
# These cases extend the published split without changing any v3 document.
# The setup policy excludes `secrets`, so `password.field` has no gold here.
@dataclass(frozen=True)
class AdjacentValue:
    value: str
    label: str | None = None
    prefix: str = ""
    suffix: str = ""


@dataclass(frozen=True)
class AdjacencyCase:
    family: str
    language: str
    region: str
    values: tuple[AdjacentValue, ...]


ADJACENT_SEPARATORS = {"space": " ", "comma": ",", "tab": "\t", "nbsp": NBSP}
ADJACENT_TEMPLATES = {
    "adjacent_prose": {
        "dev": "Check these adjacent values: {VALUES}.",
        "test": "The handover lists adjacent values: {VALUES}.",
    },
    "adjacent_log_kv": {
        "dev": 'level=info event=inspect values="{VALUES}" result=queued',
        "test": 'level=debug event=handover values="{VALUES}" result=stored',
    },
    "adjacent_csv": {
        "dev": 'row,adjacent_values\n1,"{VALUES}"\n',
        "test": 'record,adjacent_values\n7,"{VALUES}"\n',
    },
    "adjacent_json_array": {
        "dev": '{"operation":"inspect","items":["{VALUES}"]}',
        "test": '{"operation":"handover","items":["{VALUES}"]}',
    },
}


ADJACENT_GOLD = {
    "dev": (
        AdjacencyCase("ip_v6", "en", "US", (AdjacentValue("fd42:1::d1", "IPADDRESS"), AdjacentValue("fd42:1::d2", "IPADDRESS"))),
        AdjacencyCase("ip_v6_mapped", "en", "US", (AdjacentValue("fd42:1::d3", "IPADDRESS"), AdjacentValue("::ffff:10.42.0.2", "IPADDRESS"))),
        AdjacencyCase("ip_v4", "en", "US", (AdjacentValue("10.42.0.6", "IPADDRESS"), AdjacentValue("10.42.0.7", "IPADDRESS"))),
        AdjacencyCase("ip_v4_v6", "en", "US", (AdjacentValue("10.42.0.4", "IPADDRESS"), AdjacentValue("fd42:1::d4", "IPADDRESS"))),
        AdjacencyCase("ip_documentation_neighbor", "en", "US", (AdjacentValue("2001:db8::d5"), AdjacentValue("fd42:1::d5", "IPADDRESS"))),
        AdjacencyCase("phone_structural", "en", "US", (AdjacentValue("+12025550100", "TELEPHONENUM"), AdjacentValue("+12025550101", "TELEPHONENUM"))),
        AdjacencyCase("phone_e164_spaced", "en", "GB", (AdjacentValue("+44 7700 900123", "TELEPHONENUM"), AdjacentValue("+44 7700 900124", "TELEPHONENUM"))),
        AdjacencyCase("phone_national_de", "de", "DE", (AdjacentValue("+49 1555 0112233", "TELEPHONENUM"), AdjacentValue("+49 1555 0112234", "TELEPHONENUM"))),
        AdjacencyCase("phone_national_us", "en", "US", (AdjacentValue("+1 555 0100", "TELEPHONENUM"), AdjacentValue("+1 555 0101", "TELEPHONENUM"))),
        AdjacencyCase("postal_at_ch", "de", "AT", (AdjacentValue("0000", "ZIPCODE", suffix=" Narnia"), AdjacentValue("0001", "ZIPCODE", suffix=" Utopia"))),
        AdjacencyCase("postal_ca", "en", "CA", (AdjacentValue("Z1Z 9Z9", "ZIPCODE"), AdjacentValue("Z2Z 8Z8", "ZIPCODE"))),
        AdjacencyCase("postal_gb", "en", "GB", (AdjacentValue("ZZ9 9ZZ", "ZIPCODE"), AdjacentValue("ZZ8 8ZZ", "ZIPCODE"))),
        AdjacencyCase("birth_date_cue", "en", "US", (AdjacentValue("1980-02-03", "DATEOFBIRTH", prefix="DOB: "), AdjacentValue("1981-02-04", "DATEOFBIRTH", prefix="DOB: "))),
        AdjacencyCase("ip_v6_triple", "en", "US", (AdjacentValue("fd42:1::d6", "IPADDRESS"), AdjacentValue("fd42:1::d7", "IPADDRESS"), AdjacentValue("fd42:1::d8", "IPADDRESS"))),
    ),
    "test": (
        AdjacencyCase("ip_v6", "en", "US", (AdjacentValue("fd42:2::a1", "IPADDRESS"), AdjacentValue("fd42:2::a2", "IPADDRESS"))),
        AdjacencyCase("ip_v6_mapped", "en", "US", (AdjacentValue("fd42:2::a3", "IPADDRESS"), AdjacentValue("::ffff:10.43.0.3", "IPADDRESS"))),
        AdjacencyCase("ip_v4", "en", "US", (AdjacentValue("10.43.0.8", "IPADDRESS"), AdjacentValue("10.43.0.9", "IPADDRESS"))),
        AdjacencyCase("ip_v4_v6", "en", "US", (AdjacentValue("10.43.0.5", "IPADDRESS"), AdjacentValue("fd42:2::a4", "IPADDRESS"))),
        AdjacencyCase("ip_documentation_neighbor", "en", "US", (AdjacentValue("2001:db8::a5"), AdjacentValue("fd42:2::a5", "IPADDRESS"))),
        AdjacencyCase("phone_structural", "en", "US", (AdjacentValue("+12025550102", "TELEPHONENUM"), AdjacentValue("+12025550103", "TELEPHONENUM"))),
        AdjacencyCase("phone_e164_spaced", "en", "GB", (AdjacentValue("+44 7700 900125", "TELEPHONENUM"), AdjacentValue("+44 7700 900126", "TELEPHONENUM"))),
        AdjacencyCase("phone_national_de", "de", "DE", (AdjacentValue("+49 1555 0112235", "TELEPHONENUM"), AdjacentValue("+49 1555 0112236", "TELEPHONENUM"))),
        AdjacencyCase("phone_national_us", "en", "US", (AdjacentValue("+1 555 0102", "TELEPHONENUM"), AdjacentValue("+1 555 0103", "TELEPHONENUM"))),
        AdjacencyCase("postal_at_ch", "de", "AT", (AdjacentValue("0002", "ZIPCODE", suffix=" Arcadia"), AdjacentValue("0003", "ZIPCODE", suffix=" Eloria"))),
        AdjacencyCase("postal_ca", "en", "CA", (AdjacentValue("Z3Z 7Z7", "ZIPCODE"), AdjacentValue("Z4Z 6Z6", "ZIPCODE"))),
        AdjacencyCase("postal_gb", "en", "GB", (AdjacentValue("ZZ7 7ZZ", "ZIPCODE"), AdjacentValue("ZZ6 6ZZ", "ZIPCODE"))),
        AdjacencyCase("birth_date_cue", "en", "US", (AdjacentValue("1990-02-03", "DATEOFBIRTH", prefix="DOB: "), AdjacentValue("1991-02-04", "DATEOFBIRTH", prefix="DOB: "))),
        AdjacencyCase("ip_v6_triple", "en", "US", (AdjacentValue("fd42:2::a6", "IPADDRESS"), AdjacentValue("fd42:2::a7", "IPADDRESS"), AdjacentValue("fd42:2::a8", "IPADDRESS"))),
    ),
}


ADJACENT_LOOKALIKES = {
    "dev": (
        AdjacencyCase("adjacent_versions", "en", "US", (AdjacentValue("1.2.3"), AdjacentValue("4.5.6"))),
        AdjacencyCase("adjacent_hex_hashes", "en", "US", (AdjacentValue("0xdeadbeef"), AdjacentValue("0xcafebabe"))),
        AdjacencyCase("adjacent_times", "en", "US", (AdjacentValue("08:03"), AdjacentValue("09:04"))),
        AdjacencyCase("adjacent_rooms", "en", "US", (AdjacentValue("4711", prefix="Room "), AdjacentValue("4722", prefix="Room "))),
        AdjacencyCase("adjacent_due_dates", "en", "US", (AdjacentValue("2025-05-06", prefix="Due: "), AdjacentValue("2025-05-07", prefix="Due: "))),
        AdjacencyCase("adjacent_word_paths", "en", "US", (AdjacentValue("x::2"), AdjacentValue("y::3"))),
        AdjacencyCase("adjacent_documentation_ips", "en", "US", (AdjacentValue("2001:db8::d9"), AdjacentValue("2001:db8::da"))),
        AdjacencyCase("adjacent_loopback_ips", "en", "US", (AdjacentValue("127.0.0.6"), AdjacentValue("127.0.0.7"))),
        AdjacencyCase("adjacent_link_local_ips", "en", "US", (AdjacentValue("fe80::d1"), AdjacentValue("fe80::d2"))),
        AdjacencyCase("adjacent_mapped_loopback_ips", "en", "US", (AdjacentValue("::ffff:127.0.0.2"), AdjacentValue("::ffff:127.0.0.4"))),
    ),
    "test": (
        AdjacencyCase("adjacent_versions", "en", "US", (AdjacentValue("2.3.4"), AdjacentValue("5.6.7"))),
        AdjacencyCase("adjacent_hex_hashes", "en", "US", (AdjacentValue("0x1a2b3c4d"), AdjacentValue("0x5e6f7a8b"))),
        AdjacencyCase("adjacent_times", "en", "US", (AdjacentValue("10:05"), AdjacentValue("11:06"))),
        AdjacencyCase("adjacent_rooms", "en", "US", (AdjacentValue("4833", prefix="Room "), AdjacentValue("4844", prefix="Room "))),
        AdjacencyCase("adjacent_due_dates", "en", "US", (AdjacentValue("2026-06-08", prefix="Due: "), AdjacentValue("2026-06-09", prefix="Due: "))),
        AdjacencyCase("adjacent_word_paths", "en", "US", (AdjacentValue("m::4"), AdjacentValue("n::5"))),
        AdjacencyCase("adjacent_documentation_ips", "en", "US", (AdjacentValue("2001:db8::a9"), AdjacentValue("2001:db8::aa"))),
        AdjacencyCase("adjacent_loopback_ips", "en", "US", (AdjacentValue("127.0.0.8"), AdjacentValue("127.0.0.9"))),
        AdjacencyCase("adjacent_link_local_ips", "en", "US", (AdjacentValue("fe80::a1"), AdjacentValue("fe80::a2"))),
        AdjacencyCase("adjacent_mapped_loopback_ips", "en", "US", (AdjacentValue("::ffff:127.0.0.3"), AdjacentValue("::ffff:127.0.0.5"))),
    ),
}


def _adjacency_records(partition: str, layer: str) -> list[Record]:
    cases = ADJACENT_GOLD[partition] if layer == LAYER_IDENTIFIERS else ADJACENT_LOOKALIKES[partition]
    records: list[Record] = []
    for case in cases:
        for direction, values in (("forward", case.values), ("reverse", case.values[::-1])):
            for separator_name, separator in ADJACENT_SEPARATORS.items():
                for surface, templates in ADJACENT_TEMPLATES.items():
                    # An escaped JSON tab is two raw bytes, so it is not a single
                    # separator for the scanner's byte-level adjacency contract.
                    if surface == "adjacent_json_array" and separator_name == "tab":
                        continue
                    fragment = separator.join(
                        f"{{P{index}}}{{V{index}}}{{T{index}}}"
                        for index in range(1, len(values) + 1)
                    )
                    template = templates[partition].replace("{VALUES}", fragment)
                    fields: dict[str, tuple[str, str | None]] = {}
                    for index, value in enumerate(values, 1):
                        fields[f"P{index}"] = (value.prefix, None)
                        fields[f"V{index}"] = (value.value, value.label if layer == LAYER_IDENTIFIERS else None)
                        fields[f"T{index}"] = (value.suffix, None)
                    text, gold = _fill(template, fields)
                    records.append(Record(
                        uid=f"agentic-{partition}-{layer}-{case.family}-{direction}-{separator_name}-{surface}",
                        partition=partition, layer=layer, family=case.family, surface=surface,
                        validity=UNCHECKED if layer == LAYER_IDENTIFIERS else BENIGN,
                        group=f"{partition}-{layer}-{case.family}-{direction}",
                        template=f"adjacent/{surface}/{partition}",
                        language=case.language, region=case.region, text=text, gold=gold,
                    ))
    return records


# --------------------------------------------------------------------------
# Labelled PII inside benign lookalike structures. A weak postcode
# or phone rule may be vetoed when its match sits in a benign structure: the
# tail of an `ORDER-` style reference, after a currency code, or inside a
# 16-digit run. Layer A puts postcodes and reserved-range phones in exactly
# those structures, with a label that names them somewhere in the document:
# blocks above or below, past blank lines, in seven Latin-script languages and
# two other scripts, and in nested tool JSON, log and CSV records. Every value
# is gold and gated. Each layer A cell names one layer D twin: the same
# surface, value shape, benign structure and position (line, or JSON path and
# siblings), with no cue anywhere. The generator checks every pair and fails
# closed on a mismatch.

LOOKALIKE_CELL_SURFACES = ("lookalike_prose", "lookalike_log_kv", "lookalike_csv", "lookalike_tool_json")
DOCS_PER_LOOKALIKE_CELL = {LAYER_IDENTIFIERS: 6, LAYER_LOOKALIKES: 4}
CUE_VOCABULARY_PATH = Path(__file__).with_name("lookalike_cue_vocabulary.json")


def _load_cue_vocabulary() -> tuple[tuple[str, ...], frozenset[str], tuple[tuple[int, int], ...]]:
    vocabulary = json.loads(CUE_VOCABULARY_PATH.read_text(encoding="utf-8"))
    if vocabulary.get("schema_version") != 1:
        raise LayerError(f"{CUE_VOCABULARY_PATH.name}: unsupported schema_version")
    stems = tuple(stem for family in vocabulary["stems"].values() for stem in family)
    words = frozenset(word for family in vocabulary["whole_words"].values() for word in family)
    ranges = tuple((int(low, 16), int(high, 16)) for low, high in vocabulary["latin_letter_ranges"])
    return stems, words, ranges


# The veto's own cue vocabulary, one checked file shared with the Rust test.
LOOKALIKE_CUE_STEMS, LOOKALIKE_CUE_WORDS, LATIN_LETTER_RANGES = _load_cue_vocabulary()
# The veto's accent folding, exactly: nothing else is folded.
_CUE_FOLD = {
    **dict.fromkeys("àáâãäå", "a"), "ç": "c", **dict.fromkeys("èéêë", "e"),
    **dict.fromkeys("ìíîï", "i"), "ñ": "n", **dict.fromkeys("òóôõöø", "o"),
    **dict.fromkeys("ùúûü", "u"), "ß": "ss",
}


def _cue_words(text: str) -> list[str]:
    """Words split at non-letters and lower-to-upper changes, lowercased and folded."""
    words: list[str] = []
    word = ""
    previous_lower = False
    for character in text:
        if (not character.isalpha() or (character.isupper() and previous_lower)) and word:
            words.append(word)
            word = ""
        if character.isalpha():
            word += "".join(_CUE_FOLD.get(c, c) for c in character.lower())
        previous_lower = character.islower()
    return [*words, word] if word else words


def has_lookalike_cue(text: str) -> bool:
    return any(
        word in LOOKALIKE_CUE_WORDS or word.startswith(LOOKALIKE_CUE_STEMS)
        for word in _cue_words(text)
    )


def has_non_latin_letter(text: str) -> bool:
    return any(
        character.isalpha()
        and not any(low <= ord(character) <= high for low, high in LATIN_LETTER_RANGES)
        for character in text
    )


# Cue-free, Latin-only padding. {F<n>} expands to n prose lines, {L<n>} to n
# log lines, {R<n>} to n empty CSV rows, {J<n>:<indent>} to n JSON members.
LOOKALIKE_FILLER = {
    "dev": ("Checked by the night shift.", "Nothing else changed in this batch.",
            "Keep the record as it is.", "Status stays open for now.",
            "The sync ran without errors.", "Ignore the older draft.",
            "Queue length was normal.", "No action needed from the team.",
            "Reviewed again this morning.", "Batch closed at noon."),
    "test": ("Handled by the early rota.", "Nothing new since Monday.",
             "Leave the entry unchanged.", "Marked as pending review.",
             "The import finished cleanly.", "Skip the earlier version.",
             "Load was within limits.", "No follow-up is required.",
             "Looked at once more today.", "Closed out before lunch."),
}
LOOKALIKE_LOG_LINE = {
    "dev": 'level=debug svc=queue msg="heartbeat" seq={i}',
    "test": 'svc=worker msg="idle" n={i}',
}
LOOKALIKE_CSV_ROW = {"dev": "{i},,pending", "test": "{i},,queued"}
LOOKALIKE_JSON_MEMBERS = {
    "dev": (("status", "open"), ("source", "import"), ("priority", "normal"),
            ("createdBy", "system"), ("revision", "3"), ("flags", "none"),
            ("batch", "b7"), ("channel", "web"), ("owner", "ops")),
    "test": (("state", "queued"), ("origin", "sync"), ("rank", "low"),
             ("author", "robot"), ("version", "5"), ("labels", "none"),
             ("group", "g2"), ("medium", "api"), ("team", "core")),
}


def _zip5(rng: Rng, partition: str) -> str:
    low, high = {"dev": (30000, 59999), "test": (60000, 99999)}[partition]
    return str(rng.between(low, high))


# Phones come from reserved, non-reachable ranges (CONTRIBUTING.md, phone-number
# fixtures): NANPA 555-01xx and the German 1555 mobile shape. The partitions
# differ in area code (US) or in the first subscriber digits (DE).
DE_RESERVED_SUBSCRIBER_PREFIX = {"dev": "01", "test": "02"}


def _phone_us_reserved(rng: Rng, partition: str) -> str:
    return f"{rng.choice(US_AREA_CODES[partition])}-555-01{rng.digits(2)}"


def _phone_de_reserved(rng: Rng, partition: str) -> str:
    return "01555" + DE_RESERVED_SUBSCRIBER_PREFIX[partition] + rng.digits(5)


def _phone_de_run(rng: Rng, partition: str) -> str:
    """A reserved German mobile shape run on to 16 digits in groups of four.

    Luhn-invalid, as `_sku`: a card rule would otherwise cover the run and hide
    what the phone rule does with it. No phone number has 16 digits, so this
    shape is a counterweight only, never gold.
    """
    while True:
        digits = "01555" + DE_RESERVED_SUBSCRIBER_PREFIX[partition][1] + rng.digits(10)
        if not luhn_valid(digits):
            return "-".join(_by_four(digits))


# kind: (gold label, language, region, value maker)
LOOKALIKE_VALUE_KINDS: dict[str, tuple[str, str, str, Callable[[Rng, str], str]]] = {
    "zip_us": ("ZIPCODE", "en", "US", _zip5),
    "zip_de": ("ZIPCODE", "de", "DE", _zip5),
    "phone_us": ("TELEPHONENUM", "en", "US", _phone_us_reserved),
    "phone_de": ("TELEPHONENUM", "de", "DE", _phone_de_reserved),
    "phone_de_run": ("TELEPHONENUM", "de", "DE", _phone_de_run),
}


class BenignStructure(str, Enum):
    JOINED_IDENTIFIER = "joined_identifier"
    CURRENCY_AMOUNT = "currency_amount"
    DIGIT_RUN = "digit_run"


class LabelRelation(str, Enum):
    """Where a layer A label stands relative to its value."""
    ABOVE = "above"
    ABOVE_PAST_BLANK = "above_past_blank"
    BELOW_PAST_BLANK = "below_past_blank"
    SAME_LINE = "same_line"
    LOG_FIELD_ABOVE = "log_field_above"
    CSV_HEADER = "csv_header"
    JSON_ANCESTOR_KEY = "json_ancestor_key"
    JSON_SIBLING_AFTER = "json_sibling_after"
    JSON_NESTED_SIBLING_AFTER = "json_nested_sibling_after"
    JSON_TYPE_ABOVE = "json_type_above"


@dataclass(frozen=True)
class LookalikeTwin:
    """A benign structure, value shape and position: one layer D cell."""
    family: str
    surface: str
    kind: str
    structure: BenignStructure
    templates: Mapping[str, str]


@dataclass(frozen=True)
class LabelledCell:
    """One layer A cell: its twin's structure, shape and position, plus a label."""
    family: str
    twin: LookalikeTwin
    label: LabelRelation
    templates: Mapping[str, str]


def _twin(family: str, surface: str, kind: str, structure: BenignStructure, dev: str, test: str) -> LookalikeTwin:
    return LookalikeTwin(family, f"lookalike_{surface}", kind, structure, {"dev": dev, "test": test})


JOINED, CURRENCY, RUN = BenignStructure.JOINED_IDENTIFIER, BenignStructure.CURRENCY_AMOUNT, BenignStructure.DIGIT_RUN

# Layer D. {V} is benign; no cue word and no non-Latin letter anywhere.
ORDER_REF_ZIP = _twin("order_ref_zip_shape", "prose", "zip_us", JOINED,
                      "Order reference:\n{F1}\nORDER-{V}", "Reference for the parcel:\n{F7}\nREF-{V}")
ORDER_REF_ZIP_FIRST_LINE = _twin("order_ref_zip_shape_first_line", "prose", "zip_us", JOINED,
                                 "ORDER-{V}\n\n{F2}\nThe value above is the order reference.",
                                 "SKU-{V}\n\nThat line is the article reference.")
AMOUNT_ZIP = _twin("amount_zip_shape", "prose", "zip_us", CURRENCY,
                   "Invoice total:\n{F1}\nEUR {V}", "Amount due:\n\n{F2}\nUSD {V}")
ORDER_REF_ZIP_DE = _twin("order_ref_zip_shape_de", "prose", "zip_de", JOINED,
                         "Bestellnummer:\n{F7}\nBESTELLUNG-{V}", "Auftrag:\n\nAUFTRAG-{V}")
ORDER_REF_PHONE = _twin("order_ref_phone_shape", "prose", "phone_us", JOINED,
                        "Ticket reference:\n{F2}\nTICKET-{V}", "Order:\n\n{F7}\nORDER-{V}")
ORDER_REF_PHONE_INLINE = _twin("order_ref_phone_shape_inline", "prose", "phone_us", JOINED,
                               "Reference: ORDER-{V}", "Ticket reference : TICKET-{V}")
ORDER_REF_PHONE_DE = _twin("order_ref_phone_shape_de", "prose", "phone_de", JOINED,
                           "Bestellung:\nBitte den Wert unten verwenden.\nORDER-{V}",
                           "Vorgang:\n\n{F4}\nVORGANG-{V}")
SKU_RUN_DE = _twin("sku_digit_run_de", "prose", "phone_de_run", RUN,
                   "Artikelnummer:\n{F1}\n{V}", "Artikel:\n\n{F2}\n{V}")
ORDER_LOG_ZIP = _twin("order_log_record", "log_kv", "zip_us", JOINED,
                      "level=info event=order.update field=orderRef\n{L7}\nlevel=info event=order.update value=ORDER-{V}",
                      "svc=orders op=set key=invoiceRef\n{L8}\nsvc=orders op=set value=INVOICE-{V}")
ORDER_LOG_PHONE = _twin("order_log_record_phone_shape", "log_kv", "phone_us", JOINED,
                        "level=info event=ticket.update field=ticketRef\n{L7}\nlevel=info event=ticket.update value=TICKET-{V}",
                        "svc=orders op=set key=orderRef\n{L8}\nsvc=orders op=set value=ORDER-{V}")
ORDER_CSV_ZIP = _twin("order_csv_record_zip_shape", "csv", "zip_us", JOINED,
                      "row,invoiceRef,status\n{R7}\n8,INVOICE-{V},pending\n",
                      "record_no,orderRef,state\n{R8}\n9,ORDER-{V},queued\n")
ORDER_CSV_PHONE = _twin("order_csv_record", "csv", "phone_us", JOINED,
                        "row,orderRef,status\n{R7}\n8,ORDER-{V},pending\n",
                        "record_no,ticketRef,state\n{R8}\n9,TICKET-{V},queued\n")
ORDER_JSON_NESTED = _twin("order_json_nested", "tool_json", "zip_us", JOINED,
                          '{\n  "order": {\n    "items": {\n{J7:6}\n      "ref": "ORDER-{V}"\n    }\n  }\n}',
                          '{\n  "invoice": {\n    "lines": {\n{J8:6}\n      "ref": "INVOICE-{V}"\n    }\n  }\n}')
ORDER_JSON_ENTRIES = _twin(
    "order_json_entries_array", "tool_json", "zip_us", JOINED,
    '{\n  "account": {\n    "profile": {\n      "orders": {\n        "entries": [\n'
    '          {\n{J7:12}\n            "code": "ORDER-{V}"\n          }\n        ]\n      }\n    }\n  }\n}',
    '{\n  "account": {\n    "history": {\n      "tickets": {\n        "entries": [\n'
    '          {\n{J8:12}\n            "code": "TICKET-{V}"\n          }\n        ]\n      }\n    }\n  }\n}')
ORDER_JSON_SIBLING_ZIP = _twin("order_json_type_sibling_zip_shape", "tool_json", "zip_us", JOINED,
                               '{\n  "value": "ORDER-{V}",\n{J3:2}\n  "type": "order"\n}',
                               '{\n  "value": "INVOICE-{V}",\n{J4:2}\n  "type": "invoice"\n}')
ORDER_JSON_SIBLING_PHONE = _twin("order_json_type_sibling", "tool_json", "phone_us", JOINED,
                                 '{\n  "value": "ORDER-{V}",\n{J3:2}\n  "type": "order"\n}',
                                 '{\n  "value": "REF-{V}",\n{J4:2}\n  "type": "ticket"\n}')
ORDER_JSON_META = _twin("order_json_meta_after", "tool_json", "phone_us", JOINED,
                        '{\n  "entry": {\n    "value": "ORDER-{V}",\n{J3:4}\n    "meta": {\n      "type": "order"\n    }\n  }\n}',
                        '{\n  "entry": {\n    "value": "TICKET-{V}",\n{J4:4}\n    "meta": {\n      "type": "ticket"\n    }\n  }\n}')
ORDER_JSON_TYPE_ABOVE = _twin(
    "order_json_type_above", "tool_json", "phone_us", JOINED,
    '{\n  "entry": {\n    "type": "order reference for the quarterly stock count, kept with the batch '
    'record",\n{J7:4}\n    "value": "ORDER-{V}"\n  }\n}',
    '{\n  "entry": {\n    "type": "ticket reference of the account, used for two-step review '
    'and audit notices",\n{J8:4}\n    "value": "REF-{V}"\n  }\n}')
ORDER_JSON_RECORD = _twin("order_json_record_value", "tool_json", "phone_us", JOINED,
                          '{\n  "record": {\n{J7:4}\n    "value": "ORDER-{V}"\n  }\n}',
                          '{\n  "ticket": {\n{J8:4}\n    "value": "INVOICE-{V}"\n  }\n}')

LOOKALIKE_TWINS = (
    ORDER_REF_ZIP, ORDER_REF_ZIP_FIRST_LINE, AMOUNT_ZIP, ORDER_REF_ZIP_DE, ORDER_REF_PHONE,
    ORDER_REF_PHONE_INLINE, ORDER_REF_PHONE_DE, SKU_RUN_DE, ORDER_LOG_ZIP, ORDER_LOG_PHONE,
    ORDER_CSV_ZIP, ORDER_CSV_PHONE, ORDER_JSON_NESTED, ORDER_JSON_ENTRIES, ORDER_JSON_SIBLING_ZIP,
    ORDER_JSON_SIBLING_PHONE, ORDER_JSON_META, ORDER_JSON_TYPE_ABOVE, ORDER_JSON_RECORD,
)
# Twins without a layer A cell, and why none is possible.
UNPAIRED_TWINS = {
    "sku_digit_run_de": "no phone number has 16 digits, so a labelled 16-digit run is not one phone value",
}


def _labelled(family: str, twin: LookalikeTwin, label: LabelRelation, dev: str, test: str) -> LabelledCell:
    return LabelledCell(family, twin, label, {"dev": dev, "test": test})


R = LabelRelation
# Layer A. {V} is the gold value.
LOOKALIKE_GOLD_CELLS = (
    _labelled("zip_label_above", ORDER_REF_ZIP, R.ABOVE,
              "ZIP for delivery:\n{F1}\nORDER-{V}", "ZIP code of the recipient:\n{F2}\nORD-{V}"),
    _labelled("zip_label_seven_above", ORDER_REF_ZIP, R.ABOVE,
              "ZIP for delivery:\n{F7}\nORDER-{V}", "Recipient ZIP:\n{F8}\nREF-{V}"),
    _labelled("zip_label_past_blank", ORDER_REF_ZIP, R.ABOVE_PAST_BLANK,
              "ZIP for delivery:\n\nORDER-{V}", "ZIP of the customer:\n\n{F2}\nTICKET-{V}"),
    _labelled("zip_label_ten_above_past_blank", ORDER_REF_ZIP, R.ABOVE_PAST_BLANK,
              "ZIP for delivery:\n\n{F9}\nORDER-{V}", "Shipping ZIP:\n{F4}\n\n{F5}\nINVOICE-{V}"),
    _labelled("zip_label_below_past_blank", ORDER_REF_ZIP_FIRST_LINE, R.BELOW_PAST_BLANK,
              "ORDER-{V}\n\n{F2}\nThe value above is the ZIP for delivery.",
              "SKU-{V}\n\nThat reference is the customer ZIP."),
    _labelled("zip_de_label_above", ORDER_REF_ZIP_DE, R.ABOVE,
              "Postleitzahl für die Lieferung:\n{F7}\nBESTELLUNG-{V}", "PLZ des Empfängers:\n\nAUFTRAG-{V}"),
    _labelled("zip_label_currency", AMOUNT_ZIP, R.ABOVE,
              "ZIP for shipping:\n{F1}\nEUR {V}", "ZIP to use:\n\n{F3}\nUSD {V}"),
    _labelled("phone_en_label_above", ORDER_REF_PHONE, R.ABOVE,
              "Phone number of the customer:\n{F7}\nORDER-{V}", "Customer phone:\n\n{F3}\nREF-{V}"),
    _labelled("phone_de_label_above", ORDER_REF_PHONE_DE, R.ABOVE,
              "Telefonnummer des Kunden:\nBitte den Wert unten verwenden.\nORDER-{V}",
              "Telefon des Empfängers:\n\n{F6}\nBESTELLUNG-{V}"),
    _labelled("phone_fr_label_same_line", ORDER_REF_PHONE_INLINE, R.SAME_LINE,
              "Téléphone: ORDER-{V}", "Numéro de téléphone du client : TICKET-{V}"),
    _labelled("phone_es_label_above", ORDER_REF_PHONE, R.ABOVE,
              "Teléfono del cliente:\n\nORDER-{V}", "Teléfono de contacto:\n{F7}\nREF-{V}"),
    _labelled("phone_it_label_above", ORDER_REF_PHONE, R.ABOVE,
              "Telefono del cliente:\n{F9}\nORDER-{V}", "Numero di telefono:\n\n{F2}\nINVOICE-{V}"),
    _labelled("phone_nl_label_above", ORDER_REF_PHONE, R.ABOVE,
              "Telefoonnummer van de klant:\n\n{F4}\nORDER-{V}", "Mobiel nummer:\n{F8}\nTICKET-{V}"),
    _labelled("phone_pt_label_above", ORDER_REF_PHONE, R.ABOVE,
              "Telefone do cliente:\n{F7}\nORDER-{V}", "Número de telemóvel:\n\nREF-{V}"),
    _labelled("phone_cyrillic_label", ORDER_REF_PHONE_INLINE, R.SAME_LINE,
              "Телефон: ORDER-{V}", "Телефон клиента: REF-{V}"),
    _labelled("phone_japanese_label", ORDER_REF_PHONE_INLINE, R.SAME_LINE,
              "電話番号: ORDER-{V}", "お客様の電話番号: TICKET-{V}"),
    _labelled("zip_log_field_above", ORDER_LOG_ZIP, R.LOG_FIELD_ABOVE,
              "level=info event=address.update field=zipCode\n{L7}\nlevel=info event=address.update value=ORDER-{V}",
              "svc=profile op=set key=recipientZip\n{L8}\nsvc=profile op=set value=REF-{V}"),
    _labelled("phone_log_field_above", ORDER_LOG_PHONE, R.LOG_FIELD_ABOVE,
              "level=info event=contact.update kind=phone\n{L7}\nlevel=info event=contact.update value=ORDER-{V}",
              "svc=profile op=set key=mobile\n{L8}\nsvc=profile op=set value=TICKET-{V}"),
    _labelled("zip_csv_header", ORDER_CSV_ZIP, R.CSV_HEADER,
              "row,zipCode,status\n{R7}\n8,ORDER-{V},pending\n",
              "record_no,recipientZip,state\n{R8}\n9,REF-{V},queued\n"),
    _labelled("phone_csv_header", ORDER_CSV_PHONE, R.CSV_HEADER,
              "row,phone,status\n{R7}\n8,ORDER-{V},pending\n",
              "record_no,customerMobile,state\n{R8}\n9,TICKET-{V},queued\n"),
    _labelled("zip_json_nested_path", ORDER_JSON_NESTED, R.JSON_ANCESTOR_KEY,
              '{\n  "customer": {\n    "shippingAddress": {\n{J7:6}\n      "code": "ORDER-{V}"\n    }\n  }\n}',
              '{\n  "order": {\n    "deliveryAddress": {\n{J8:6}\n      "code": "REF-{V}"\n    }\n  }\n}'),
    _labelled("zip_json_entries_array", ORDER_JSON_ENTRIES, R.JSON_ANCESTOR_KEY,
              '{\n  "customer": {\n    "profile": {\n      "shippingAddress": {\n        "entries": [\n'
              '          {\n{J7:12}\n            "code": "ORDER-{V}"\n          }\n        ]\n      }\n    }\n  }\n}',
              '{\n  "account": {\n    "profile": {\n      "billingAddress": {\n        "entries": [\n'
              '          {\n{J8:12}\n            "code": "INVOICE-{V}"\n          }\n        ]\n      }\n    }\n  }\n}'),
    _labelled("zip_json_label_sibling_after", ORDER_JSON_SIBLING_ZIP, R.JSON_SIBLING_AFTER,
              '{\n  "value": "ORDER-{V}",\n{J3:2}\n  "label": "ZIP"\n}',
              '{\n  "value": "TICKET-{V}",\n{J4:2}\n  "label": "ZIP code"\n}'),
    _labelled("phone_json_type_sibling_after", ORDER_JSON_SIBLING_PHONE, R.JSON_SIBLING_AFTER,
              '{\n  "value": "ORDER-{V}",\n{J3:2}\n  "type": "phone"\n}',
              '{\n  "value": "REF-{V}",\n{J4:2}\n  "type": "mobile"\n}'),
    _labelled("phone_json_meta_type_after", ORDER_JSON_META, R.JSON_NESTED_SIBLING_AFTER,
              '{\n  "entry": {\n    "value": "ORDER-{V}",\n{J3:4}\n    "meta": {\n      "type": "phone"\n    }\n  }\n}',
              '{\n  "entry": {\n    "value": "TICKET-{V}",\n{J4:4}\n    "meta": {\n      "type": "telephone"\n    }\n  }\n}'),
    _labelled("phone_json_long_type", ORDER_JSON_TYPE_ABOVE, R.JSON_TYPE_ABOVE,
              '{\n  "entry": {\n    "type": "phone number for customer contact and delivery coordination, '
              'stored in the order record",\n{J7:4}\n    "value": "ORDER-{V}"\n  }\n}',
              '{\n  "entry": {\n    "type": "mobile number of the account holder, used for two-step sign-in '
              'and parcel notices",\n{J8:4}\n    "value": "REF-{V}"\n  }\n}'),
    _labelled("phone_json_contact_value", ORDER_JSON_RECORD, R.JSON_ANCESTOR_KEY,
              '{\n  "contact": {\n{J7:4}\n    "value": "ORDER-{V}"\n  }\n}',
              '{\n  "kontakt": {\n{J8:4}\n    "value": "INVOICE-{V}"\n  }\n}'),
)
del R

# Derived from the cells, never declared: each A cell's layer D twin.
LOOKALIKE_COUNTERWEIGHTS: dict[str, str] = {cell.family: cell.twin.family for cell in LOOKALIKE_GOLD_CELLS}


def _expand_padding(template: str, partition: str) -> str:
    import re

    def pad(match: "re.Match[str]") -> str:
        kind, count = match.group(1), int(match.group(2))
        if kind == "F":
            return "\n".join(LOOKALIKE_FILLER[partition][:count])
        if kind == "L":
            return "\n".join(LOOKALIKE_LOG_LINE[partition].format(i=i) for i in range(1, count + 1))
        if kind == "R":
            return "\n".join(LOOKALIKE_CSV_ROW[partition].format(i=i) for i in range(1, count + 1))
        indent = " " * int(match.group(3))
        return "\n".join(
            f'{indent}"{key}": "{value}",' for key, value in LOOKALIKE_JSON_MEMBERS[partition][:count]
        )

    return re.sub(r"\{([FLRJ])(\d+)(?::(\d+))?\}", pad, template)


def value_structure(text: str, start: int, end: int) -> BenignStructure | None:
    """The benign structure around text[start:end] (character offsets), if any."""
    import re

    before = text[:start]
    if re.search(r"(?:^|[^A-Za-z])[A-Za-z]+-$", before):
        return BenignStructure.JOINED_IDENTIFIER
    if re.search(r"(?:EUR|USD) $", before):
        return BenignStructure.CURRENCY_AMOUNT
    if re.fullmatch(r"\d{4}(?:-\d{4}){3}", text[start:end]):
        return BenignStructure.DIGIT_RUN
    return None


def value_position(text: str, start: int, end: int, surface: str) -> tuple[object, ...]:
    """Where text[start:end] (character offsets) sits, independent of the label.

    Tool JSON: the value's path from the root, each step (container kind,
    index among its siblings, sibling count). Other surfaces: whether its line
    is the first and the last non-blank line, and whether text precedes its
    structure or follows the value on that line.
    """
    import re

    if surface == "lookalike_tool_json":
        value = text[start:end]

        def find(node: object) -> list[tuple[str, int, int]] | None:
            if isinstance(node, str):
                return [] if value in node else None
            children = list(node.values()) if isinstance(node, dict) else node if isinstance(node, list) else []
            kind = "object" if isinstance(node, dict) else "array"
            for index, child in enumerate(children):
                path = find(child)
                if path is not None:
                    return [(kind, index, len(children)), *path]
            return None

        path = find(json.loads(text))
        if path is None:
            raise LayerError("a lookalike JSON value is not in any string")
        return ("json", *path)
    line_start = text.rfind("\n", 0, start) + 1
    line_end = text.find("\n", end)
    line_end = len(text) if line_end == -1 else line_end
    lines = [line for line in text.split("\n") if line.strip()]
    line = text[line_start:line_end]
    head = re.sub(r"(?:[A-Za-z]+-|(?:EUR|USD) )$", "", text[line_start:start])
    return (
        "line", line == lines[0], line == lines[-1], bool(head.strip()), bool(text[end:line_end].strip())
    )


def _labelled_lookalike_records(cells: Sequence[LookalikeTwin | LabelledCell], partition: str, layer: str) -> list[Record]:
    seed = PARTITION_SEEDS[partition]
    gold = layer == LAYER_IDENTIFIERS
    records: list[Record] = []
    for cell in cells:
        twin = cell.twin if isinstance(cell, LabelledCell) else cell
        label, language, region, make = LOOKALIKE_VALUE_KINDS[twin.kind]
        rng = Rng(seed, f"{layer}/lookalike/{cell.family}")
        template = _expand_padding(cell.templates[partition], partition)
        for index in range(DOCS_PER_LOOKALIKE_CELL[layer]):
            # The benign value is a recorded span only to check its position.
            text, spans, values = _fill_with_decoys(
                template, {"V": (make(rng, partition), label if gold else DECOY_PREFIX + "benign")}
            )
            records.append(Record(
                uid=f"agentic-{partition}-{layer}-{cell.family}-{index:03d}-{twin.surface}",
                partition=partition, layer=layer, family=cell.family, surface=twin.surface,
                validity=UNCHECKED if gold else BENIGN,
                group=f"{partition}-{layer}-{cell.family}-{index:03d}",
                template=f"lookalike/{cell.family}/{partition}",
                language=language, region=region, text=text, gold=spans,
            ))
            if len(spans or values) != 1:
                raise LayerError(f"{records[-1].uid}: expected exactly one lookalike value")
    return records


def lookalike_value(record: Record) -> tuple[str, int, int]:
    """The one lookalike value of a generated A or D record, in character offsets."""
    import re

    if record.gold:
        (gold,) = record.gold
        start = len(record.text.encode("utf-8")[: gold.start].decode("utf-8"))
        return gold.value, start, start + len(gold.value)
    kind = next(twin.kind for twin in LOOKALIKE_TWINS if twin.family == record.family)
    pattern = {"zip_us": r"\d{5}", "zip_de": r"\d{5}", "phone_us": r"\d{3}-555-01\d{2}",
               "phone_de": r"01555\d{7}", "phone_de_run": r"\d{4}(?:-\d{4}){3}"}[kind]
    match = list(re.finditer(rf"(?<!\d){pattern}(?!\d)", record.text))[-1]
    return match.group(0), match.start(), match.end()


def check_lookalike_pairs(records: Sequence[Record]) -> None:
    """Fail closed unless every A cell and its D twin agree on value shape,
    locale, benign structure and position, and only A carries a cue."""
    by_family: dict[tuple[str, str], list[Record]] = defaultdict(list)
    for record in records:
        if record.surface.startswith("lookalike_"):
            by_family[(record.layer, record.family)].append(record)
    signatures: dict[tuple[str, str], set[tuple[object, ...]]] = {}
    for (layer, family), family_records in by_family.items():
        signature = set()
        for record in family_records:
            value, start, end = lookalike_value(record)
            twin = next(
                (cell.twin for cell in LOOKALIKE_GOLD_CELLS if cell.family == family)
                if layer == LAYER_IDENTIFIERS
                else (twin for twin in LOOKALIKE_TWINS if twin.family == family)
            )
            if value_structure(record.text, start, end) is not twin.structure:
                raise LayerError(f"{record.uid}: value is not in a {twin.structure.value}")
            cued = has_lookalike_cue(record.text) or has_non_latin_letter(record.text)
            if cued != (layer == LAYER_IDENTIFIERS):
                raise LayerError(f"{record.uid}: a layer {layer} lookalike {'lacks' if not cued else 'carries'} a cue")
            signature.add((display_shape(value), record.language, record.region, record.surface,
                           value_position(record.text, start, end, record.surface)))
        if len(signature) != 1:
            raise LayerError(f"{layer}/{family}: documents differ in shape or position")
        signatures[(layer, family)] = signature
    for cell in LOOKALIKE_GOLD_CELLS:
        if signatures.get((LAYER_IDENTIFIERS, cell.family)) != signatures.get((LAYER_LOOKALIKES, cell.twin.family)):
            raise LayerError(f"{cell.family} and its twin {cell.twin.family} differ in shape or position")


# --------------------------------------------------------------------------
# Address blocks (generator v6). An address is personal data as a
# unit: a street, its house number, a secondary unit (`Suite 312`, `Apt. 771`,
# `Wohnung 4`, `3. Etage`), a PO box or Postfach, a US military line
# (`PSC 5512, Box 7730, APO AP ...`), the city, the state and the postcode.
# Layer A writes whole addresses in prose, in multi-line blocks, in a log
# field, split across CSV columns and in tool JSON, and every part is gold
# under the Kiji address labels; the separators between parts are not gold.
# Some cells also put a benign designator (`test Suite 4`, a team's
# `Postfach`) after the address, recorded as a decoy: an address rule that
# swallows it pays in false-positive bytes. Layer D writes the same designator
# words with no address anywhere. Every value is synthetic: invented street
# and city names, US ZIPs in the unassigned `000xx` range, German PLZ in the
# unassigned `00xxx` range and GB postcodes in the unused `ZZ` area. Every
# number range, like every name pool, is split between the partitions.

ADDRESS_SURFACES = ("address_prose", "address_block", "address_log_kv", "address_csv", "address_tool_json")
DOCS_PER_ADDRESS_CELL = {LAYER_IDENTIFIERS: 6, LAYER_LOOKALIKES: 4}
ADDRESS_LABELS = frozenset({"BUILDINGNUM", "CITY", "STATE", "STREET", "ZIPCODE"})


class Designator(str, Enum):
    """The secondary-unit or box word a cell exercises."""
    SUITE = "suite"
    APARTMENT = "apartment"
    UNIT = "unit"
    FLAT = "flat"
    FLOOR = "floor"
    PO_BOX = "po_box"
    MILITARY = "military"
    WOHNUNG = "wohnung"
    ETAGE = "etage"
    POSTFACH = "postfach"


# How each designator is written; {n} is the unit or box number.
DESIGNATOR_FORMS: dict[Designator, tuple[str, ...]] = {
    Designator.SUITE: ("Suite {n}", "Ste. {n}", "STE {n}"),
    Designator.APARTMENT: ("Apt. {n}", "Apt {n}", "Apartment {n}"),
    Designator.UNIT: ("Unit {n}", "Unit #{n}"),
    Designator.FLAT: ("Flat {n}",),
    Designator.FLOOR: ("Floor {n}", "Fl. {n}"),
    Designator.PO_BOX: ("PO Box {n}", "P.O. Box {n}"),
    Designator.MILITARY: ("PSC {n}", "Unit {n}", "CMR {n}"),
    Designator.WOHNUNG: ("Wohnung {n}", "Whg. {n}"),
    Designator.ETAGE: ("{n}. Etage", "{n}. Stock", "{n}. OG"),
    Designator.POSTFACH: ("Postfach {n}",),
}
# The word that makes a value this designator's; checked on every A unit
# value and every D decoy, so a cell cannot drift away from its twin.
DESIGNATOR_WORDS: dict[Designator, str] = {
    Designator.SUITE: r"\b(?:Suite|Ste\.?|STE)\b",
    Designator.APARTMENT: r"\b(?:Apt\.?|Apartment)\b",
    Designator.UNIT: r"\bUnit\b",
    Designator.FLAT: r"\bFlat\b",
    Designator.FLOOR: r"\b(?:Floor|Fl\.)",
    Designator.PO_BOX: r"\b(?:PO|P\.O\.) Box\b",
    Designator.MILITARY: r"\b(?:PSC|Unit|CMR|Box)\b",
    Designator.WOHNUNG: r"\b(?:Wohnung|Whg\.)",
    Designator.ETAGE: r"\. (?:Etage|Stock|OG)\b",
    Designator.POSTFACH: r"\bPostfach\b",
}
# A unit's spelling: the value with its number replaced by `{n}`. Coverage
# is checked per spelling, because a rule can match `Ste.` and not `Suite`.
_SPELLING_NUMBER = r"\d+[A-C]?"


def designator_spelling(value: str) -> str:
    import re

    return re.sub(_SPELLING_NUMBER, "{n}", value)


# Plausible number ranges per partition, disjoint so no gold value repeats
# across partitions; every other designator takes the default.
DESIGNATOR_NUMBERS: dict[Designator | None, dict[str, tuple[int, int]]] = {
    None: {"dev": (2, 489), "test": (490, 980)},
    Designator.FLOOR: {"dev": (1, 30), "test": (31, 60)},
    Designator.FLAT: {"dev": (1, 60), "test": (61, 120)},
    Designator.ETAGE: {"dev": (1, 4), "test": (5, 9)},
}
HOUSE_NUMBERS = {
    "dev": {"DE": (1, 99), "GB": (1, 99), "US": (1000, 4999)},
    "test": {"DE": (100, 199), "GB": (200, 299), "US": (5000, 9899)},
}
MILITARY_BOX_NUMBERS = {"dev": (100, 4999), "test": (5000, 9899)}

ADDRESS_STREET_STEMS = {
    "dev": ("Kalvik", "Morrowind", "Tesselby", "Brandlow", "Quenmoor", "Ostravel"),
    "test": ("Varnholt", "Elsmeré", "Corvath", "Pellinor", "Drusk", "Havelmoor"),
}
ADDRESS_STREET_SUFFIXES = {
    "US": ("Road", "Street", "Avenue", "Lane", "Drive"),
    "GB": ("Road", "Close", "Crescent", "Mews"),
    "DE": ("straße", "weg", "gasse", "allee"),
}
ADDRESS_CITIES = {
    "dev": {"US": ("Trelling", "Ashvale Point", "Norhaven"), "GB": ("Upper Brackwell", "Fennick"),
            "DE": ("Halbruck", "Oberkessel", "Lindmar")},
    "test": {"US": ("Brinmoor", "Calder Rise", "Westmere Falls"), "GB": ("Lower Tavistead", "Quellby"),
             "DE": ("Kornhelm", "Wiesenthal-Nord", "Tervelau")},
}
US_STATES = {"dev": ("OR", "WA", "CO", "MN"), "test": ("IL", "NV", "VT", "NM")}
MILITARY_POST_OFFICES = {"dev": ("FPO",), "test": ("APO", "DPO")}
MILITARY_STATES = {"dev": ("AA",), "test": ("AE", "AP")}


def _address_zip(rng: Rng, partition: str, region: str) -> str:
    if region == "GB":
        district = {"dev": 1, "test": 2}[partition]
        return f"ZZ{district}{rng.below(10)} {rng.between(1, 9)}ZZ"
    low, high = {
        ("US", "dev"): (10, 49), ("US", "test"): (50, 99),
        ("DE", "dev"): (100, 499), ("DE", "test"): (500, 999),
    }[(region, partition)]
    return f"{rng.between(low, high):05d}"


def _designator(rng: Rng, partition: str, designator: Designator, forms: Sequence[str], index: int) -> str:
    """The `index`-th document's unit: spellings rotate, so every spelling of
    a cell is generated whenever the cell has as many documents."""
    number = str(rng.between(*DESIGNATOR_NUMBERS.get(designator, DESIGNATOR_NUMBERS[None])[partition]))
    if designator in (Designator.APARTMENT, Designator.FLAT) and rng.below(3) == 0:
        number += rng.choice(("A", "B", "C"))
    return forms[index % len(forms)].format(n=number)


@dataclass(frozen=True)
class AddressCell:
    """One layer A cell: an address, every part gold, in one surface.

    {HN} house number, {ST} street, {UN} the designator's unit, {CI} city,
    {SA} state, {ZP} postcode; a military cell writes {UN} {BX}, {MC} {MS}
    {ZP}. {X} is an optional benign designator recorded as a decoy.
    """
    family: str
    designator: Designator | None
    region: str
    surface: str
    templates: Mapping[str, str]
    decoy: Designator | None = None


@dataclass(frozen=True)
class DesignatorTwin:
    """One layer D cell: a designator word and number with no address."""
    family: str
    designator: Designator
    surface: str
    region: str
    templates: Mapping[str, str]
    # Spellings in place of the designator's own (the military `Box N`).
    forms: tuple[str, ...] | None = None

    @property
    def spellings(self) -> tuple[str, ...]:
        return self.forms or DESIGNATOR_FORMS[self.designator]


def _address(family: str, designator: Designator | None, region: str, surface: str, dev: str, test: str,
             decoy: Designator | None = None) -> AddressCell:
    return AddressCell(family, designator, region, f"address_{surface}", {"dev": dev, "test": test}, decoy)


def _designator_twin(family: str, designator: Designator, surface: str, region: str, dev: str, test: str,
                     forms: tuple[str, ...] | None = None) -> DesignatorTwin:
    return DesignatorTwin(family, designator, f"address_{surface}", region, {"dev": dev, "test": test}, forms)


D_ = Designator
ADDRESS_CELLS = (
    _address("address_us_suite_prose", D_.SUITE, "US", "prose",
             "Please ship the replacement to {HN} {ST} {UN}, {CI}, {SA} {ZP} by Friday.",
             "Deliver the parcel to {HN} {ST} {UN}, {CI}, {SA} {ZP} before noon."),
    _address("address_us_apartment_prose", D_.APARTMENT, "US", "prose",
             "Her new address is {HN} {ST} {UN}, {CI}, {SA} {ZP}.",
             "He moved to {HN} {ST}, {UN}, {CI}, {SA} {ZP} last month."),
    _address("address_us_unit_block", D_.UNIT, "US", "block",
             "Mailing address:\n{HN} {ST}, {UN}\n{CI}, {SA} {ZP}",
             "Send the signed form to\n{HN} {ST} {UN}\n{CI}, {SA} {ZP}\nThanks."),
    _address("address_us_po_box_prose", D_.PO_BOX, "US", "prose",
             "Mail the cheque to {UN}, {CI}, {SA} {ZP}.",
             "Our remittance address is {UN}, {CI}, {SA} {ZP}, not the office."),
    _address("address_us_floor_log", D_.FLOOR, "US", "log_kv",
             'level=info event=shipment.create address="{HN} {ST}, {UN}, {CI}, {SA} {ZP}" status=queued',
             'svc=orders op=ship to="{HN} {ST} {UN}, {CI}, {SA} {ZP}" result=ok'),
    _address("address_us_suite_csv", D_.SUITE, "US", "csv",
             "ref,street,unit,city,state,zip\n7,{HN} {ST},{UN},{CI},{SA},{ZP}\n",
             "record_no,address_line1,address_line2,city,state,postal\n9,{HN} {ST},{UN},{CI},{SA},{ZP}\n"),
    _address("address_us_apartment_json_fields", D_.APARTMENT, "US", "tool_json",
             '{"customer":{"shipping":{"line1":"{HN} {ST}","line2":"{UN}","city":"{CI}","state":"{SA}","zip":"{ZP}"}}}',
             '{"order":{"recipient":{"street":"{HN} {ST}","unit":"{UN}","city":"{CI}","region":"{SA}","postalCode":"{ZP}"}}}'),
    _address("address_us_suite_json_line", D_.SUITE, "US", "tool_json",
             '{"ticket":{"note":"Customer moved.","address":"{HN} {ST} {UN}, {CI}, {SA} {ZP}"}}',
             '{"order":{"deliverTo":"{HN} {ST} {UN}, {CI}, {SA} {ZP}","priority":"normal"}}'),
    _address("address_us_state_block", None, "US", "block",
             "Billing address\n{HN} {ST}\n{CI}, {SA} {ZP}",
             "Return label:\n{HN} {ST}\n{CI} {SA} {ZP}\n"),
    _address("address_military_prose", D_.MILITARY, "US", "prose",
             "Forward it to {UN}, {BX}, {MC} {MS} {ZP}.",
             "Send the care package to {UN} {BX}, {MC} {MS} {ZP} this week."),
    _address("address_military_block", D_.MILITARY, "US", "block",
             "Mail goes to:\n{UN}, {BX}\n{MC} {MS} {ZP}",
             "Ship to:\n{UN}, {BX}\n{MC} {MS} {ZP}\n"),
    _address("address_us_decoy_after", None, "US", "prose",
             "Ship to {HN} {ST}, {CI}, {SA} {ZP}. Then rerun test {X} before the release.",
             "Deliver to {HN} {ST}, {CI}, {SA} {ZP}.\nThe regression {X} is still red.",
             decoy=D_.SUITE),
    _address("address_gb_flat_prose", D_.FLAT, "GB", "prose",
             "Post the keys to {UN}, {HN} {ST}, {CI} {ZP}.",
             "Please send it to {UN}, {HN} {ST}, {CI} {ZP} by Monday."),
    _address("address_de_house_number_prose", None, "DE", "prose",
             "Bitte an {ST} {HN}, {ZP} {CI} liefern.",
             "Die neue Anschrift lautet {ST} {HN}, {ZP} {CI}."),
    _address("address_de_wohnung_block", D_.WOHNUNG, "DE", "block",
             "Anschrift:\n{ST} {HN}, {UN}\n{ZP} {CI}",
             "Lieferadresse\n{ST} {HN}\n{UN}\n{ZP} {CI}"),
    _address("address_de_etage_prose", D_.ETAGE, "DE", "prose",
             "Wir sitzen in der {ST} {HN}, {UN}, {ZP} {CI}.",
             "Das Büro ist in der {ST} {HN}, {UN}, {ZP} {CI}."),
    _address("address_de_wohnung_csv", D_.WOHNUNG, "DE", "csv",
             "kunde_nr,strasse,zusatz,plz,ort\n4,{ST} {HN},{UN},{ZP},{CI}\n",
             "nr,anschrift,zusatz,plz,ort\n8,{ST} {HN},{UN},{ZP},{CI}\n"),
    _address("address_de_postfach_json", D_.POSTFACH, "DE", "tool_json",
             '{"kunde":{"anschrift":"{UN}, {ZP} {CI}"}}',
             '{"empfaenger":{"zustellung":"{UN}","plz":"{ZP}","ort":"{CI}"}}'),
    _address("address_de_decoy_after", None, "DE", "prose",
             "Lieferung an {ST} {HN}, {ZP} {CI}. Die Rückmeldung liegt in {X} des Teams.",
             "Zustellung: {ST} {HN}, {ZP} {CI}\nDie Antwort liegt in {X} der Buchhaltung.",
             decoy=D_.POSTFACH),
)

# Layer D. {X} is the benign designator; no street, city or postcode anywhere.
ADDRESS_TWINS = (
    _designator_twin("designator_suite_prose", D_.SUITE, "prose", "US",
                     "Run test {X} before merging.", "The regression {X} took four minutes."),
    _designator_twin("designator_suite_log", D_.SUITE, "log_kv", "US",
                     'level=info event=ci.run target="{X}" result=passed',
                     'svc=ci op=run suite="{X}" result=green'),
    _designator_twin("designator_apartment_prose", D_.APARTMENT, "prose", "US",
                     "The rental board lists {X} as vacant.", "In the floor plan, {X} has two windows."),
    _designator_twin("designator_unit_prose", D_.UNIT, "prose", "US",
                     "Read {X} of the course before Monday.", "{X} of the workbook covers fractions."),
    _designator_twin("designator_flat_csv", D_.FLAT, "csv", "GB",
                     "plan,fee\nbasic,{X}\n", "tier,charge\nstarter,{X}\n"),
    _designator_twin("designator_floor_prose", D_.FLOOR, "prose", "US",
                     "The printer on {X} is jammed again.", "Coffee is on {X} today."),
    _designator_twin("designator_po_box_json", D_.PO_BOX, "tool_json", "US",
                     '{"form":{"field":"{X}","required":false}}',
                     '{"template":{"placeholder":"{X}","visible":true}}'),
    _designator_twin("designator_military_prose", D_.MILITARY, "prose", "US",
                     "The {X} steering group meets at ten.", "Minutes from {X} are attached."),
    _designator_twin("designator_box_prose", D_.MILITARY, "prose", "US",
                     "Put the spare cables in {X} on the top shelf.", "The returns are packed in {X} by the door.",
                     forms=("Box {n}",)),
    _designator_twin("designator_wohnung_prose", D_.WOHNUNG, "prose", "DE",
                     "Im Exposé ist {X} bereits reserviert.", "Im Grundriss hat {X} einen Balkon."),
    _designator_twin("designator_etage_prose", D_.ETAGE, "prose", "DE",
                     "Der Aufzug hält im {X} nicht.", "Der Drucker im {X} ist leer."),
    _designator_twin("designator_postfach_log", D_.POSTFACH, "log_kv", "DE",
                     'level=warn event=mailbox.full target="{X}"',
                     'svc=mail op=sync folder="{X}" result=ok'),
)
del D_


def _address_fields(cell: AddressCell, rng: Rng, partition: str, index: int) -> dict[str, tuple[str, str | None]]:
    region = cell.region
    stem = rng.choice(ADDRESS_STREET_STEMS[partition])
    suffix = rng.choice(ADDRESS_STREET_SUFFIXES[region])
    street = f"{stem}{suffix}" if region == "DE" else f"{stem} {suffix}"
    house = str(rng.between(*HOUSE_NUMBERS[partition][region]))
    if region == "DE" and rng.below(4) == 0:
        house += rng.choice(("a", "b"))
    fields: dict[str, tuple[str, str | None]] = {
        "HN": (house, "BUILDINGNUM"),
        "ST": (street, "STREET"),
        "CI": (rng.choice(ADDRESS_CITIES[partition][region]), "CITY"),
        "SA": (rng.choice(US_STATES[partition]), "STATE"),
        "ZP": (_address_zip(rng, partition, region), "ZIPCODE"),
    }
    if cell.designator is Designator.MILITARY:
        fields |= {
            "UN": (_designator(rng, partition, Designator.MILITARY, DESIGNATOR_FORMS[Designator.MILITARY], index),
                   "STREET"),
            "BX": (f"Box {rng.between(*MILITARY_BOX_NUMBERS[partition])}", "BUILDINGNUM"),
            "MC": (rng.choice(MILITARY_POST_OFFICES[partition]), "CITY"),
            "MS": (rng.choice(MILITARY_STATES[partition]), "STATE"),
        }
    elif cell.designator is not None:
        fields["UN"] = (_designator(rng, partition, cell.designator, DESIGNATOR_FORMS[cell.designator], index),
                        "BUILDINGNUM")
    if cell.decoy is not None:
        fields["X"] = (_designator(rng, partition, cell.decoy, DESIGNATOR_FORMS[cell.decoy], index),
                       DECOY_PREFIX + "benign")
    return fields


ADDRESS_LANGUAGE = {"US": "en", "GB": "en", "DE": "de"}


def _address_records(cells: Sequence[AddressCell | DesignatorTwin], partition: str, layer: str) -> list[Record]:
    seed = PARTITION_SEEDS[partition]
    records: list[Record] = []
    for cell in cells:
        rng = Rng(seed, f"{layer}/address/{cell.family}")
        for index in range(DOCS_PER_ADDRESS_CELL[layer]):
            if isinstance(cell, AddressCell):
                fields = _address_fields(cell, rng, partition, index)
            else:
                fields = {"X": (_designator(rng, partition, cell.designator, cell.spellings, index),
                                DECOY_PREFIX + "benign")}
            text, gold, decoys = _fill_with_decoys(cell.templates[partition], fields)
            records.append(Record(
                uid=f"agentic-{partition}-{layer}-{cell.family}-{index:03d}-{cell.surface}",
                partition=partition, layer=layer, family=cell.family, surface=cell.surface,
                validity=UNCHECKED if layer == LAYER_IDENTIFIERS else BENIGN,
                group=f"{partition}-{layer}-{cell.family}-{index:03d}",
                template=f"address/{cell.family}/{partition}",
                language=ADDRESS_LANGUAGE[cell.region], region=cell.region,
                text=text, gold=gold, decoys=decoys,
            ))
    return records


# The label each address placeholder is scored under; `UN` is a street line
# in a military address and a building number everywhere else.
ADDRESS_PART_LABELS = {
    "HN": "BUILDINGNUM", "ST": "STREET", "CI": "CITY", "SA": "STATE", "ZP": "ZIPCODE",
    "BX": "BUILDINGNUM", "MC": "CITY", "MS": "STATE",
}
_ADDRESS_PLACEHOLDER = r"\{(HN|ST|UN|CI|SA|ZP|BX|MC|MS)\}"


def required_address_parts(cell: AddressCell) -> set[str]:
    """The placeholders a cell's shape must write, each exactly once."""
    if cell.designator is Designator.MILITARY:
        return {"UN", "BX", "MC", "MS", "ZP"}
    parts = {"CI", "ZP"} | ({"SA"} if cell.region == "US" else set())
    if cell.designator in (Designator.PO_BOX, Designator.POSTFACH):
        return parts | {"UN"}
    return parts | {"HN", "ST"} | ({"UN"} if cell.designator is not None else set())


def address_part_values(record: Record, cell: AddressCell) -> list[tuple[str, Gold]]:
    """Each gold span with the placeholder that wrote it, in text order.

    Fails closed unless every placeholder of the template is gold under its
    own label, so a house number and a unit (both BUILDINGNUM) are told apart.
    """
    import re

    parts = re.findall(_ADDRESS_PLACEHOLDER, cell.templates[record.partition])
    gold = sorted(record.gold, key=lambda span: span.start)
    if len(parts) != len(gold):
        raise LayerError(f"{record.uid}: {len(gold)} gold parts for placeholders {parts}")
    labels = {**ADDRESS_PART_LABELS, "UN": "STREET" if cell.designator is Designator.MILITARY else "BUILDINGNUM"}
    for part, span in zip(parts, gold):
        if span.label != labels[part]:
            raise LayerError(f"{record.uid}: placeholder {part} is gold {span.label}, not {labels[part]}")
    return list(zip(parts, gold))


def check_address_cells(records: Sequence[Record]) -> None:
    """Fail closed unless every A address is whole, part by part, and every
    unit spelling it scores has a layer D twin spelled the same way."""
    import re

    cells = {cell.family: cell for cell in ADDRESS_CELLS}
    twins = {twin.family: twin for twin in ADDRESS_TWINS}
    for cell in ADDRESS_CELLS:
        for partition, template in cell.templates.items():
            parts = re.findall(_ADDRESS_PLACEHOLDER, template)
            if sorted(parts) != sorted(required_address_parts(cell)):
                raise LayerError(
                    f"{cell.family}/{partition}: placeholders {sorted(parts)} are not the shape's "
                    f"{sorted(required_address_parts(cell))}"
                )
    used = {c.designator for c in ADDRESS_CELLS} | {c.decoy for c in ADDRESS_CELLS}
    unused = sorted(t.family for t in ADDRESS_TWINS if t.designator not in used)
    if unused:
        raise LayerError(f"layer D designator twins no layer A cell uses: {unused}")
    positive: dict[str, str] = {}
    benign: set[str] = set()
    for record in records:
        if not record.surface.startswith("address_"):
            continue
        if record.layer == LAYER_IDENTIFIERS:
            cell = cells[record.family]
            units = [span for part, span in address_part_values(record, cell) if part in ("UN", "BX")]
            if cell.designator is not None and not any(
                re.search(DESIGNATOR_WORDS[cell.designator], span.value) for span in units
            ):
                raise LayerError(f"{record.uid}: no gold part carries the {cell.designator.value} designator")
            for span in units:
                positive.setdefault(designator_spelling(span.value), record.uid)
            decoy = cell.decoy
        else:
            if record.gold:
                raise LayerError(f"{record.uid}: a layer D designator twin carries gold")
            decoy = twins[record.family].designator
            benign.update(designator_spelling(d.value) for d in record.decoys)
        if decoy is None:
            if record.decoys:
                raise LayerError(f"{record.uid}: an undeclared decoy")
            continue
        if len(record.decoys) != 1 or not re.search(DESIGNATOR_WORDS[decoy], record.decoys[0].value):
            raise LayerError(f"{record.uid}: expected one {decoy.value} decoy")
        if record.layer == LAYER_LOOKALIKES and re.search(r"\d{5}|ZZ\d", record.text):
            raise LayerError(f"{record.uid}: a designator twin carries a postcode shape")
    uncovered = sorted(set(positive) - benign)
    if uncovered:
        raise LayerError(f"unit spellings with no layer D counterweight: {uncovered} (first in {positive[uncovered[0]]})")


# --------------------------------------------------------------------------
# Phone shapes (generator v7). Layer A writes phone numbers in shapes the
# `+CC` and US/DE national rules miss: French dotted groups (`02.61.91.23.45`,
# whose last four groups also parse as an IPv4 address), national digit groups
# with no `+` behind a phone label (`Phone: 012 448 903`, `Mobile: 07 18 44
# 90`), a trunk zero in parentheses (`+44 (0)20 7946 0412`) and the `00` /
# `001-` international prefixes. The whole number is gold, prefix included.
# Layer D writes each shape's benign neighbours with no phone anywhere: dotted
# version strings, dates and OIDs, digit groups behind an order or invoice
# label, a signed score with a parenthesised zero, and `00`- or `001-`-prefixed
# product and document codes. A rule that drops the label or the shape's
# precision pays there.
#
# Values come from documented fictional ranges (CONTRIBUTING.md, phone-number
# fixtures): ARCEP's numbers reserved for fiction (02 61 91, 04 65 71 and
# 01 99 00 xx xx), BNetzA's media-production numbers (Berlin 030 23125,
# Frankfurt 069 90009, München 089 99998), Ofcom's drama range 020 7946 0xxx
# and NANPA 555-01xx. The labelled national groups have no documented range, so
# they are synthesized non-reachable: the Spanish nine-digit and the Danish
# eight-digit plans never start with 0, and every generated value does.

# The `tel_` prefix: layer R already has a `phone_de` surface.
PHONE_SURFACES = ("tel_prose", "tel_log_kv", "tel_csv", "tel_tool_json")
DOCS_PER_PHONE_CELL = {LAYER_IDENTIFIERS: 6, LAYER_LOOKALIKES: 4}


class PhoneShape(str, Enum):
    """The written phone shape a cell exercises."""
    DOTTED = "dotted"
    NATIONAL_3X3 = "national_3x3"
    NATIONAL_2X4 = "national_2x4"
    TRUNK_PARENS = "trunk_parens"
    PREFIX_00 = "prefix_00"
    PREFIX_001 = "prefix_001"


# Bare national digit groups are a phone only behind a phone label; the same
# digits unlabelled are an order or ticket number, and layer D writes them so.
NATIONAL_PHONE_SHAPES = frozenset({PhoneShape.NATIONAL_3X3, PhoneShape.NATIONAL_2X4})


def _two(rng: Rng) -> str:
    """Two digits with no leading zero, so a dotted tail stays IPv4-shaped."""
    return str(rng.between(10, 99))


# shape -> partition -> (region, value maker). Each partition draws from its
# own fictional blocks, so no value repeats across partitions.
PHONE_VALUES: dict[PhoneShape, dict[str, tuple[tuple[str, Callable[[Rng], str]], ...]]] = {
    PhoneShape.DOTTED: {
        "dev": (("FR", lambda rng: f"02.61.91.{_two(rng)}.{_two(rng)}"),),
        "test": (("FR", lambda rng: f"04.65.71.{_two(rng)}.{_two(rng)}"),),
    },
    PhoneShape.NATIONAL_3X3: {
        "dev": (("ES", lambda rng: f"0{rng.between(10, 49)} {rng.digits(3)} {rng.digits(3)}"),),
        "test": (("ES", lambda rng: f"0{rng.between(50, 99)} {rng.digits(3)} {rng.digits(3)}"),),
    },
    PhoneShape.NATIONAL_2X4: {
        "dev": (("DK", lambda rng: f"0{rng.between(1, 4)} {rng.digits(2)} {rng.digits(2)} {rng.digits(2)}"),),
        "test": (("DK", lambda rng: f"0{rng.between(5, 9)} {rng.digits(2)} {rng.digits(2)} {rng.digits(2)}"),),
    },
    PhoneShape.TRUNK_PARENS: {
        "dev": (("DE", lambda rng: f"+49 (0)30 23125 {rng.digits(3)}"),
                ("FR", lambda rng: f"+33 (0)1 99 00 {rng.digits(2)} {rng.digits(2)}")),
        "test": (("GB", lambda rng: f"+44 (0)20 7946 0{rng.digits(3)}"),
                 ("DE", lambda rng: f"+49 (0)69 90009 {rng.digits(3)}")),
    },
    PhoneShape.PREFIX_00: {
        "dev": (("DE", lambda rng: f"0049 30 23125{rng.digits(3)}"),
                ("FR", lambda rng: f"0033 1 99 00 {rng.digits(2)} {rng.digits(2)}")),
        "test": (("GB", lambda rng: f"0044 20 7946 0{rng.digits(3)}"),
                 ("DE", lambda rng: f"0049 89 99998 {rng.digits(3)}")),
    },
    PhoneShape.PREFIX_001: {
        "dev": (("US", lambda rng: f"001-{rng.choice(US_AREA_CODES['dev'])}-555-01{rng.digits(2)}"),),
        "test": (("US", lambda rng: f"001 {rng.choice(US_AREA_CODES['test'])} 555 01{rng.digits(2)}"),),
    },
}
# Every gold value of a shape matches its pattern in full: a cell cannot
# drift into a shape another rule already covers.
PHONE_SHAPE_PATTERNS: dict[PhoneShape, str] = {
    PhoneShape.DOTTED: r"0[1-9](?:\.\d\d){4}",
    PhoneShape.NATIONAL_3X3: r"0\d\d \d{3} \d{3}",
    PhoneShape.NATIONAL_2X4: r"0\d(?: \d\d){3}",
    PhoneShape.TRUNK_PARENS: r"\+\d{2} \(0\)\d{1,2}(?: \d{2,5}){2,4}",
    PhoneShape.PREFIX_00: r"00(?:33|44|49) \d{1,2}(?: \d{2,8}){1,4}",
    PhoneShape.PREFIX_001: r"001[- ]\d{3}[- ]555[- ]01\d\d",
}
# The over-broad rule for each shape: no label, no country code, no
# fictional-block anchor. Every A value matches it, and so does at least one
# layer D decoy of the shape, so shipping it costs false-positive bytes. The
# committed mutant policy carries the same patterns.
PHONE_BROAD_PATTERNS: dict[PhoneShape, str] = {
    PhoneShape.DOTTED: r"\b\d{1,2}(?:\.\d{1,4}){2,6}\b",
    PhoneShape.NATIONAL_3X3: r"\b\d{2,3} \d{3} \d{3}\b",
    PhoneShape.NATIONAL_2X4: r"\b\d{2}(?: \d{2}){3}\b",
    PhoneShape.TRUNK_PARENS: r"\+\d{1,3} ?\(0\)",
    PhoneShape.PREFIX_00: r"\b00\d{2}[ -]\d{1,4}[ -]\d{2,5}",
    PhoneShape.PREFIX_001: r"\b001[- ]\d{3}[- ]\d{3}[- ]\d{3,4}\b",
}
# The ordinary rule a detector would write for each shape: its exact group
# widths and separators, with no label, numbering-plan or context check. Every
# A value of the shape matches it, and so does at least one layer D twin
# written in the same shape, so it pays false-positive bytes as well. The
# `(0)` trunk after `+CC` has no benign writing; its counterweight is the
# broad pattern and the phone parser. The committed narrow mutant policy
# carries the same patterns.
PHONE_NARROW_PATTERNS: dict[PhoneShape, str] = {
    PhoneShape.DOTTED: r"\b0\d(?:\.\d{2}){4}\b",
    PhoneShape.NATIONAL_3X3: r"\b\d{3} \d{3} \d{3}\b",
    PhoneShape.NATIONAL_2X4: r"\b\d{2}(?: \d{2}){3}\b",
    PhoneShape.PREFIX_00: r"\b00\d{2} \d{1,2}(?: \d{2,8}){1,4}\b",
    PhoneShape.PREFIX_001: r"\b001[- ]\d{3}[- ]\d{3}[- ]\d{4}\b",
}
# E.164 spare country codes: a `00` code behind one reaches no phone.
UNASSIGNED_COUNTRY_CODES = {"dev": "28", "test": "89"}
# A benign context that makes dotted pairs a version or a part number.
DOTTED_BENIGN_CONTEXT = r"(?i)(?:\b|_)(?:firmware|build|version|release|revision|part|catalog|model)(?:\b|_)"
# A phone label, in any cell language. National cells carry one before the
# value; no layer D twin carries one anywhere.
PHONE_CUE = (
    r"(?i)\b(?:phone|mobile|tel|tél|telephone|téléphone|telefon|telefono|teléfono|mobil|msisdn|call|dial|ring|"
    r"rappeler|joindre|appelez|anrufen)\b"
)


@dataclass(frozen=True)
class PhoneCell:
    """One layer A cell: {V} is the whole phone number, gold."""
    family: str
    shape: PhoneShape
    surface: str
    language: str
    templates: Mapping[str, str]


@dataclass(frozen=True)
class PhoneTwin:
    """One layer D cell: {X} is a benign value of the shape's broad pattern."""
    family: str
    shape: PhoneShape
    surface: str
    language: str
    region: str
    templates: Mapping[str, str]
    make: Callable[[Rng, str], str]


def _phone_cell(family: str, shape: PhoneShape, surface: str, language: str, dev: str, test: str) -> PhoneCell:
    return PhoneCell(family, shape, f"tel_{surface}", language, {"dev": dev, "test": test})


def _phone_twin(family: str, shape: PhoneShape, surface: str, language: str, region: str, dev: str, test: str,
                make: Callable[[Rng, str], str]) -> PhoneTwin:
    return PhoneTwin(family, shape, f"tel_{surface}", language, region, {"dev": dev, "test": test}, make)


def _dotted_version(rng: Rng, partition: str) -> str:
    """Five dotted groups whose first four parse as a public IPv4 address."""
    major = {"dev": (1, 4), "test": (5, 9)}[partition]
    return f"{rng.between(*major)}.{_two(rng)}.{_two(rng)}.{_two(rng)}.{_two(rng)}"


def _dotted_date(rng: Rng, partition: str) -> str:
    year = {"dev": 2025, "test": 2026}[partition]
    return f"{rng.between(1, 28):02d}.{rng.between(1, 12):02d}.{year}"


def _oid(rng: Rng, partition: str) -> str:
    arc = {"dev": "1.3.6.1.4.1", "test": "2.16.840.1"}[partition]
    return f"{arc}.{rng.between(1000, 60000)}.{rng.between(1, 9)}"


def _order_3x3(rng: Rng, partition: str) -> str:
    low, high = {"dev": (10, 49), "test": (50, 99)}[partition]
    return f"0{rng.between(low, high)} {rng.digits(3)} {rng.digits(3)}"


def _amount_3x3(rng: Rng, partition: str) -> str:
    return f"{rng.between(100, 999)} {rng.digits(3)} {rng.digits(3)}"


def _ticket_2x4(rng: Rng, partition: str) -> str:
    first = {"dev": (1, 4), "test": (5, 9)}[partition]
    return f"0{rng.between(*first)} {rng.digits(2)} {rng.digits(2)} {rng.digits(2)}"


def _score_2x4(rng: Rng, partition: str) -> str:
    return " ".join(str(rng.between(10, 99)) for _ in range(4))


def _signed_zero(rng: Rng, partition: str) -> str:
    return f"+{rng.between(10, 49) if partition == 'dev' else rng.between(50, 99)} (0)"


def _product_00(rng: Rng, partition: str) -> str:
    country = {"dev": "49", "test": "44"}[partition]
    return f"00{country}-{rng.digits(4)}-{rng.digits(4)}"


def _document_001(rng: Rng, partition: str) -> str:
    return f"001-{rng.digits(3)}-{rng.digits(3)}-{rng.digits(3)}"


def _dotted_part(rng: Rng, partition: str) -> str:
    """Dotted pairs in the phone's exact shape, a firmware or part number.

    Every `0X.XX.XX.XX.XX` is a possible French number, so even a benign value
    comes from an ARCEP fiction block and can reach no one."""
    block = {"dev": "01.99.00", "test": "02.61.91"}[partition]
    return f"{block}.{rng.digits(2)}.{rng.digits(2)}"


def _ticket_00(rng: Rng, partition: str) -> str:
    """Space-grouped like a `00` phone, behind a spare country code."""
    return f"00{UNASSIGNED_COUNTRY_CODES[partition]} {rng.between(1, 99)} {rng.digits(4)} {rng.digits(4)}"


def _item_001(rng: Rng, partition: str, separator: str | None = None) -> str:
    """Grouped like a `001` NANP phone; an exchange starting 0 or 1 is never assigned.

    Written with each partition's own positive separator unless `separator` is given."""
    separator = separator or {"dev": "-", "test": " "}[partition]
    groups = (str(rng.between(200, 999)), f"{rng.between(0, 1)}{rng.digits(2)}", rng.digits(4))
    return separator.join(("001", *groups))


def _item_001_spaced(rng: Rng, partition: str) -> str:
    """A space-separated `001` item code in both partitions."""
    return _item_001(rng, partition, " ")


def phone_reading(value: str, before: str) -> PhoneShape | None:
    """The phone shape `value` reads as, or None when it cannot be a phone:
    a lexical match alone is not a phone reading."""
    import re

    if re.fullmatch(PHONE_SHAPE_PATTERNS[PhoneShape.TRUNK_PARENS], value):
        return PhoneShape.TRUNK_PARENS
    prefixed = re.fullmatch(r"00(\d{2}) \d{1,2}(?: \d{2,8}){1,4}", value)
    if prefixed and prefixed.group(1) not in UNASSIGNED_COUNTRY_CODES.values():
        return PhoneShape.PREFIX_00
    nanp = re.fullmatch(r"001[- ](\d)\d\d[- ](\d)\d\d[- ]\d{4}", value)
    if nanp and nanp.group(1) not in "01" and nanp.group(2) not in "01":
        return PhoneShape.PREFIX_001
    if re.fullmatch(PHONE_SHAPE_PATTERNS[PhoneShape.DOTTED], value) and not re.search(DOTTED_BENIGN_CONTEXT, before):
        return PhoneShape.DOTTED
    return None


P_ = PhoneShape
PHONE_CELLS = (
    _phone_cell("phone_dotted_prose", P_.DOTTED, "prose", "fr",
                "Tél. : {V}, merci de rappeler avant midi.",
                "Vous pouvez me joindre au {V} demain matin."),
    _phone_cell("phone_dotted_signature", P_.DOTTED, "prose", "fr",
                "Cordialement,\nService client\n{V}",
                "Bien à vous,\nL'équipe support\n{V}\n"),
    _phone_cell("phone_dotted_log", P_.DOTTED, "log_kv", "fr",
                "level=info event=callback.request phone={V} status=queued",
                'svc=crm op=contact tel="{V}" result=ok'),
    _phone_cell("phone_dotted_csv", P_.DOTTED, "csv", "fr",
                "id,service,telephone\n3,support,{V}\n",
                "ref,tel,statut\n12,{V},actif\n"),
    _phone_cell("phone_national_3x3_prose", P_.NATIONAL_3X3, "prose", "en",
                "Phone: {V} (evenings only).",
                "Please note my new mobile, {V}, for the delivery driver."),
    _phone_cell("phone_national_3x3_json", P_.NATIONAL_3X3, "tool_json", "en",
                '{"contact":{"phone":"{V}","preferred":true}}',
                '{"customer":{"mobile":"{V}","channel":"sms"}}'),
    _phone_cell("phone_national_2x4_prose", P_.NATIONAL_2X4, "prose", "en",
                "Mobile: {V}",
                "Best reached by phone on {V} after six."),
    _phone_cell("phone_national_2x4_log", P_.NATIONAL_2X4, "log_kv", "en",
                'level=info event=profile.update mobile="{V}" status=ok',
                'svc=support op=callback phone="{V}" result=queued'),
    _phone_cell("phone_national_2x4_csv", P_.NATIONAL_2X4, "csv", "en",
                "id,mobile,status\n4,{V},active\n",
                "record_no,phone,state\n9,{V},pending\n"),
    _phone_cell("phone_trunk_parens_prose", P_.TRUNK_PARENS, "prose", "en",
                "Please call {V} if the courier is late.",
                "Our office line is {V}, open weekdays."),
    _phone_cell("phone_trunk_parens_json", P_.TRUNK_PARENS, "tool_json", "en",
                '{"office":{"phone":"{V}","hours":"9-17"}}',
                '{"contact":{"tel":"{V}"},"verified":false}'),
    _phone_cell("phone_prefix_00_prose", P_.PREFIX_00, "prose", "en",
                "From abroad, dial {V} and ask for billing.",
                "Call {V} from outside the country."),
    _phone_cell("phone_prefix_00_csv", P_.PREFIX_00, "csv", "en",
                "id,phone,note\n2,{V},office\n",
                "record_no,tel,source\n5,{V},import\n"),
    _phone_cell("phone_prefix_001_prose", P_.PREFIX_001, "prose", "en",
                "From Europe, dial {V} for the help desk.",
                "Call {V} from abroad, the line is free."),
    _phone_cell("phone_prefix_001_log", P_.PREFIX_001, "log_kv", "en",
                "level=info event=callback.request phone={V} status=queued",
                'svc=crm op=dial msisdn="{V}" result=ok'),
)
# Layer D. {X} is benign and matches the shape's broad pattern; no phone,
# no phone label anywhere.
PHONE_TWINS = (
    _phone_twin("phone_twin_dotted_version", P_.DOTTED, "prose", "en", "US",
                "Firmware {X} fixes the boot loop.", "Upgrade the agent to {X} before Friday.",
                _dotted_version),
    _phone_twin("phone_twin_dotted_version_log", P_.DOTTED, "log_kv", "en", "US",
                'level=info event=deploy build="{X}" status=ok', "svc=ci op=release version={X} result=green",
                _dotted_version),
    _phone_twin("phone_twin_dotted_date_csv", P_.DOTTED, "csv", "de", "DE",
                "id,datum,status\n3,{X},offen\n", "nr,faellig,stand\n8,{X},erledigt\n",
                _dotted_date),
    _phone_twin("phone_twin_oid_json", P_.DOTTED, "tool_json", "en", "US",
                '{"certificate":{"policyOid":"{X}"}}', '{"snmp":{"oid":"{X}","type":"gauge"}}',
                _oid),
    _phone_twin("phone_twin_order_3x3", P_.NATIONAL_3X3, "prose", "en", "ES",
                "Order {X} shipped today.", "Invoice {X} is paid in full.",
                _order_3x3),
    _phone_twin("phone_twin_amount_3x3_json", P_.NATIONAL_3X3, "tool_json", "en", "ES",
                '{"invoice":{"total":"EUR {X}","status":"open"}}', '{"budget":{"amount":"{X}","currency":"EUR"}}',
                _amount_3x3),
    _phone_twin("phone_twin_ticket_2x4", P_.NATIONAL_2X4, "log_kv", "en", "DK",
                'level=info event=ticket.close ref="{X}" status=done', 'svc=support op=merge case="{X}" result=ok',
                _ticket_2x4),
    _phone_twin("phone_twin_scores_2x4_csv", P_.NATIONAL_2X4, "csv", "en", "DK",
                "round,scores\n1,{X}\n", "heat,times\n2,{X}\n",
                _score_2x4),
    _phone_twin("phone_twin_signed_zero", P_.TRUNK_PARENS, "prose", "en", "GB",
                "The home side finished {X} on goal difference.", "Stock moved {X} after the audit.",
                _signed_zero),
    _phone_twin("phone_twin_product_00", P_.PREFIX_00, "prose", "en", "DE",
                "Reorder part {X} before the line stops.", "The spare kit {X} is out of stock.",
                _product_00),
    _phone_twin("phone_twin_product_00_csv", P_.PREFIX_00, "csv", "en", "GB",
                "sku,qty\n{X},4\n", "part_no,stock\n{X},12\n",
                _product_00),
    _phone_twin("phone_twin_document_001", P_.PREFIX_001, "tool_json", "en", "US",
                '{"document":{"number":"{X}","type":"invoice"}}', '{"filing":{"ref":"{X}","status":"draft"}}',
                _document_001),
    _phone_twin("phone_twin_dotted_firmware", P_.DOTTED, "prose", "en", "US",
                "Firmware {X} fixes the fan curve.", "Flash build {X} before the release.",
                _dotted_part),
    _phone_twin("phone_twin_dotted_part_csv", P_.DOTTED, "csv", "en", "US",
                "part,qty\n{X},3\n", "model,stock\n{X},9\n",
                _dotted_part),
    _phone_twin("phone_twin_ticket_00", P_.PREFIX_00, "prose", "en", "GB",
                "Ticket {X} is closed.", "Reorder kit {X} today.",
                _ticket_00),
    _phone_twin("phone_twin_item_001", P_.PREFIX_001, "log_kv", "en", "US",
                'svc=warehouse op=pick item="{X}" result=ok', 'level=info event=rma.open case="{X}" status=new',
                _item_001),
    _phone_twin("phone_twin_item_001_spaced", P_.PREFIX_001, "csv", "en", "US",
                "sku,bin\n{X},A4\n", "item,shelf\n{X},C2\n",
                _item_001_spaced),
)
del P_


def _phone_records(cells: Sequence[PhoneCell | PhoneTwin], partition: str, layer: str) -> list[Record]:
    seed = PARTITION_SEEDS[partition]
    records: list[Record] = []
    for cell in cells:
        rng = Rng(seed, f"{layer}/phone/{cell.family}")
        for index in range(DOCS_PER_PHONE_CELL[layer]):
            if isinstance(cell, PhoneCell):
                makers = PHONE_VALUES[cell.shape][partition]
                region, make = makers[index % len(makers)]
                fields = {"V": (make(rng), "TELEPHONENUM")}
            else:
                region = cell.region
                fields = {"X": (cell.make(rng, partition), DECOY_PREFIX + "benign")}
            text, gold, decoys = _fill_with_decoys(cell.templates[partition], fields)
            records.append(Record(
                uid=f"agentic-{partition}-{layer}-{cell.family}-{index:03d}-{cell.surface}",
                partition=partition, layer=layer, family=cell.family, surface=cell.surface,
                validity=UNCHECKED if layer == LAYER_IDENTIFIERS else BENIGN,
                group=f"{partition}-{layer}-{cell.family}-{index:03d}",
                template=f"phone/{cell.family}/{partition}",
                language=cell.language, region=region,
                text=text, gold=gold, decoys=decoys,
            ))
    return records


def check_phone_cells(records: Sequence[Record]) -> None:
    """Fail closed unless every A phone is whole and in its shape, every
    national one follows a phone label, and every shape A scores has a layer D
    twin its broad pattern pays for, with no phone label in any twin and no
    twin value with a phone reading; every narrow rule must pay in layer D too."""
    import re

    cells = {cell.family: cell for cell in PHONE_CELLS}
    twins = {twin.family: twin for twin in PHONE_TWINS}
    unused = sorted(t.family for t in PHONE_TWINS if t.shape not in {c.shape for c in PHONE_CELLS})
    if unused:
        raise LayerError(f"layer D phone twins no layer A cell uses: {unused}")
    scored: dict[PhoneShape, str] = {}
    paid: set[PhoneShape] = set()
    narrow_paid: set[PhoneShape] = set()
    for record in records:
        if not record.surface.startswith("tel_"):
            continue
        if record.layer == LAYER_IDENTIFIERS:
            cell = cells[record.family]
            if record.decoys or len(record.gold) != 1 or record.gold[0].label != "TELEPHONENUM":
                raise LayerError(f"{record.uid}: expected exactly one TELEPHONENUM gold and no decoy")
            value = record.gold[0].value
            if not re.fullmatch(PHONE_SHAPE_PATTERNS[cell.shape], value):
                raise LayerError(f"{record.uid}: {value!r} is not a {cell.shape.value} phone")
            for kind, patterns in (("broad", PHONE_BROAD_PATTERNS), ("narrow", PHONE_NARROW_PATTERNS)):
                if cell.shape in patterns and not re.search(patterns[cell.shape], value):
                    raise LayerError(f"{record.uid}: the {cell.shape.value} {kind} pattern misses {value!r}")
            before = record.text.encode("utf-8")[: record.gold[0].start].decode("utf-8")
            if cell.shape in NATIONAL_PHONE_SHAPES and not re.search(PHONE_CUE, before):
                raise LayerError(f"{record.uid}: a national {cell.shape.value} phone has no phone label before it")
            scored.setdefault(cell.shape, record.uid)
        else:
            twin = twins[record.family]
            if record.gold:
                raise LayerError(f"{record.uid}: a layer D phone twin carries gold")
            if len(record.decoys) != 1:
                raise LayerError(f"{record.uid}: expected one benign decoy")
            if re.search(PHONE_CUE, record.text):
                raise LayerError(f"{record.uid}: a layer D phone twin carries a phone label")
            decoy = record.decoys[0]
            before = record.text.encode("utf-8")[: decoy.start].decode("utf-8")
            reading = phone_reading(decoy.value, before)
            if reading is not None:
                raise LayerError(f"{record.uid}: a layer D decoy reads as a {reading.value} phone")
            if not re.search(PHONE_BROAD_PATTERNS[twin.shape], decoy.value):
                raise LayerError(f"{record.uid}: the {twin.shape.value} broad pattern misses the decoy")
            paid.add(twin.shape)
            narrow = PHONE_NARROW_PATTERNS.get(twin.shape)
            if narrow and re.search(narrow, decoy.value):
                narrow_paid.add(twin.shape)
    uncovered = sorted(shape.value for shape in set(scored) - paid)
    if uncovered:
        raise LayerError(f"phone shapes with no layer D counterweight: {uncovered}")
    free = sorted(shape.value for shape in set(scored) & set(PHONE_NARROW_PATTERNS) - narrow_paid)
    if free:
        raise LayerError(f"phone shapes whose narrow rule pays nothing in layer D: {free}")


# --------------------------------------------------------------------------
# Cued grammar and short identifiers (generator v8). Layer A writes values that
# only their wording makes personal: a person's age after `turned`, `at the age
# of`, before `y/o` or `year old female`; a date of birth given one sentence
# after the question (`date of birth? It's 3/11/1987`); a 12- to 15-digit
# (Maestro-length) card number right after a card label; and postcodes in
# foreign or short shapes (`481 22`, `53-320`, six digits, `75534-030`, three
# digits) right after a postal label. The value alone is gold. Layer D writes
# the same numbers where the wording says they are not personal: an object's or
# an animal's age, a person turning 90 degrees, a date after an unrelated
# sentence, order, tracking and timestamp numbers, and codes behind batch,
# room or error labels, some of them one clause away from a card or postal
# word. A rule that drops the person, the sentence-break copula or the
# cue-adjacency pays there.

CUE_SURFACES = ("cue_prose", "cue_log_kv", "cue_csv", "cue_tool_json")
DOCS_PER_CUE_CELL = {LAYER_IDENTIFIERS: 6, LAYER_LOOKALIKES: 4}


class CueShape(str, Enum):
    """The wording or written shape a cell exercises."""
    AGE_TURNED = "age_turned"
    AGE_AT_THE_AGE_OF = "age_at_the_age_of"
    AGE_YO = "age_yo"
    AGE_YEAR_OLD_GENDER = "age_year_old_gender"
    DOB_SENTENCE_BREAK = "dob_sentence_break"
    CARD_SHORT_COMPACT = "card_short_compact"
    CARD_SHORT_GROUPED = "card_short_grouped"
    ZIP_SE = "zip_se"
    ZIP_PL = "zip_pl"
    ZIP_SIX = "zip_six"
    ZIP_BR = "zip_br"
    ZIP_THREE = "zip_three"

    @property
    def label(self) -> str:
        return CUE_LABELS[self.value.split("_", 1)[0]]


CUE_LABELS = {"age": "AGE", "dob": "DATEOFBIRTH", "card": "CREDITCARDNUMBER", "zip": "ZIPCODE"}

_AGE_VALUE = r"(?:[1-9]\d?|1[01]\d)"
_DOB_VALUE = (
    r"(?:(?:0?[1-9]|1[0-2])/(?:0?[1-9]|[12]\d|3[01])/(?:19|20)\d\d"
    r"|(?:0[1-9]|[12]\d|3[01])\.(?:0[1-9]|1[0-2])\.(?:19|20)\d\d"
    r"|(?:19|20)\d\d-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12]\d|3[01]))"
)
_DATE_LOOSE = r"\d{1,4}[./-]\d{1,2}[./-]\d{2,4}"
# Maestro issuer prefixes (50, 56-58, 6x); each partition draws its own.
_CARD_IIN = r"(?:5[0678]|6\d)"
CARD_IINS = {"dev": ("50", "56"), "test": ("58", "67")}
# Every gold value of a shape matches its pattern in full.
CUE_SHAPE_PATTERNS: dict[CueShape, str] = {
    CueShape.AGE_TURNED: _AGE_VALUE,
    CueShape.AGE_AT_THE_AGE_OF: _AGE_VALUE,
    CueShape.AGE_YO: _AGE_VALUE,
    CueShape.AGE_YEAR_OLD_GENDER: _AGE_VALUE,
    CueShape.DOB_SENTENCE_BREAK: _DOB_VALUE,
    CueShape.CARD_SHORT_COMPACT: _CARD_IIN + r"\d{10,13}",
    CueShape.CARD_SHORT_GROUPED: _CARD_IIN + r"\d\d \d{4} \d{4}",
    CueShape.ZIP_SE: r"[1-9]\d\d \d\d",
    CueShape.ZIP_PL: r"\d\d-\d{3}",
    CueShape.ZIP_SIX: r"[1-9]\d{5}",
    CueShape.ZIP_BR: r"\d{5}-\d{3}",
    CueShape.ZIP_THREE: r"[1-9]\d\d",
}
# Words that make a sentence about a person. No apostrophes: the mutant
# policies carry these patterns as TOML literal strings.
PERSON_WORDS = (
    r"(?:i|me|my|he|she|his|her|we|they|patient|customer|client|mother|father|mom|dad|son|daughter|wife|"
    r"husband|grandmother|grandfather|woman|man|female|male|ich|er|sie|mein|meine|meiner|meinem|mutter|vater|"
    r"tochter|sohn|oma|opa)"
)
_ZIP_WORDS = r"(?:zip(?:[\x20_-]?code)?|post(?:al)?[\x20_-]?code|postcode|postnummer|postleitzahl|plz|cep|kod\x20pocztowy|póstnúmer)"
# The over-broad rule for each shape: the wording or the digits with no person,
# sentence-break copula or cue. Every A value matches it and so does every
# layer D twin of the shape. The committed broad mutant policy carries the same
# patterns.
CUE_BROAD_PATTERNS: dict[CueShape, str] = {
    CueShape.AGE_TURNED: r"(?i)\bturn(?:ed|s)?\s+\d{1,3}\b|\b\d{1,3}\s+geworden\b",
    CueShape.AGE_AT_THE_AGE_OF: r"(?i)\b(?:at\s+the\s+age\s+of|im\s+alter\s+von)\s+\d{1,3}\b",
    CueShape.AGE_YO: r"(?i)\b\d{1,3}\s?y/o\b",
    CueShape.AGE_YEAR_OLD_GENDER: r"(?i)\b\d{1,3}[- ]years?[- ]old\b",
    CueShape.DOB_SENTENCE_BREAK: r"(?i)(?:date\s+of\s+birth|\bdob\b|geburtsdatum)[^\n]{0,80}?" + _DATE_LOOSE,
    CueShape.CARD_SHORT_COMPACT: r"\b\d{12,15}\b",
    CueShape.CARD_SHORT_GROUPED: r"\b\d{4} \d{4} \d{4}\b",
    CueShape.ZIP_SE: r"\b\d{3} \d{2}\b",
    CueShape.ZIP_PL: r"\b\d{2}-\d{3}\b",
    CueShape.ZIP_SIX: r"\b\d{6}\b",
    CueShape.ZIP_BR: r"\b\d{5}-\d{3}\b",
    CueShape.ZIP_THREE: r"\b\d{3}\b",
}
# The rule a detector would plausibly write for each shape: a person word
# somewhere before the age wording, any few words between the date-of-birth
# question and the date, the issuer prefix and card length with no cue, a
# postal word anywhere within 40 non-digit characters, `at the age of` or `im
# Alter von` before a person-range age with no subject check. Every A value of
# the shape matches it, and so does at least one layer D twin, so each pays
# false positives. The committed narrow mutant policy carries the same
# patterns.
CUE_NARROW_PATTERNS: dict[CueShape, str] = {
    CueShape.AGE_TURNED: (
        rf"(?i)\b{PERSON_WORDS}\b[^.\n]{{0,40}}?\b(?:turned|turns)\s+\d{{1,3}}\b"
        rf"|\b{PERSON_WORDS}\b[^.\n]{{0,40}}?\b\d{{1,3}}\s+geworden\b"
    ),
    CueShape.AGE_AT_THE_AGE_OF: rf"(?i)\b(?:at the age of|im alter von) {_AGE_VALUE}\b",
    CueShape.AGE_YO: rf"(?i)\b{PERSON_WORDS}\b[^.\n]{{0,40}}?\b\d{{1,3}}\s?y/o\b|\b\d{{1,3}}\s?y/o\s+(?:fe)?male\b",
    CueShape.AGE_YEAR_OLD_GENDER: r"(?i)\b\d{1,3}[- ]years?[- ]old[- ](?:fe)?male\b",
    CueShape.DOB_SENTENCE_BREAK: (
        r"(?i)(?:date\s+of\s+birth|\bdob\b|geburtsdatum)\s*[.?!]\s+(?:\S+\s+){0,3}?" + _DATE_LOOSE
    ),
    CueShape.CARD_SHORT_COMPACT: rf"\b{_CARD_IIN}\d{{10,13}}\b",
    CueShape.CARD_SHORT_GROUPED: rf"\b{_CARD_IIN}\d\d \d{{4}} \d{{4}}\b",
    **{
        shape: rf"(?i){_ZIP_WORDS}[^\d]{{0,40}}?\b{CUE_SHAPE_PATTERNS[shape]}\b"
        for shape in (CueShape.ZIP_SE, CueShape.ZIP_PL, CueShape.ZIP_SIX, CueShape.ZIP_BR, CueShape.ZIP_THREE)
    },
}
# The reference reading (`cue_reading`): what makes a value personal.
_SENTENCE_BREAK = r'[.!?\n"{}]'
AGE_GRAMMAR: dict[CueShape, tuple[str | None, str | None]] = {
    CueShape.AGE_TURNED: (r"(?i)\bturn(?:ed|s)?\s+$", r"(?i)^\s+geworden\b"),
    CueShape.AGE_AT_THE_AGE_OF: (r"(?i)\b(?:at\s+the\s+age\s+of|im\s+alter\s+von)\s+$", None),
    CueShape.AGE_YO: (None, r"(?i)^\s?y/o\b"),
    CueShape.AGE_YEAR_OLD_GENDER: (None, r"(?i)^[- ]years?[- ]old\b"),
}
# An age followed by a unit is an angle or a share, and one followed by an
# object or an animal noun is not a person's.
AGE_NOT_PERSON_AFTER = (
    r"(?i)^\s*(?:degrees?|grad|°|%|percent)"
    r"|^(?:[- ]?(?:y/o|years?[- ]old|jahren?))?[- ]+(?:(?:fe)?male[- ]+)?"
    r"(?:laptop|server|boiler|car|building|bridge|codebase|cat|dog|horse|mare|stallion|labrador|terrier|tortoise)\b"
)
DOB_BREAK = (
    r"(?i)(?:date\s+of\s+birth|\bdob\b|geburtsdatum)\s*[.?!]\s+"
    r"(?:it'?s|it\s+is|that'?s|that\s+is|es\s+ist|das\s+ist)(?:\s+der)?\s+$"
)
CARD_DIRECT = (
    r"(?i)(?:\bcard(?:[\x20_-]?(?:number|num|no|nr))?|\bkarten?(?:nummer|nr)|\bmaestro)\b"
    r"[\"'\s:=]*(?:\{\s*\"(?:number|num|pan|value)\"\s*:\s*\")?(?:(?:is|ist|lautet)\s+)?$"
)
CARD_HEADER = r"(?i)card(?:[\x20_-]?(?:number|num|no|nr))?"
CARD_WORD = r"(?i)card|karte|maestro"
ZIP_DIRECT = rf"(?i)\b{_ZIP_WORDS}\b[\"'\s:=]*(?:(?:is|ist|lautet)\s+)?$"
ZIP_HEADER = rf"(?i){_ZIP_WORDS}"
ZIP_WORD = rf"(?i){_ZIP_WORDS}"


def _csv_header(text: str, start: int) -> str:
    """The header of the CSV column the character offset `start` falls in."""
    line_start = text.rfind("\n", 0, start) + 1
    column = text[line_start:start].count(",")
    return text.split("\n", 1)[0].split(",")[column]


def cue_reading(shape: CueShape, text: str, start: int, end: int, surface: str) -> bool:
    """Whether the value at character offsets [start, end) reads as personal.

    This is the reference semantics the cells are built around, not a rule to
    ship: layer A values must read as personal and layer D values must not."""
    import re

    before, after = text[:start], text[end:]
    if shape.label == "AGE":
        head = re.split(_SENTENCE_BREAK, before)[-1]
        tail = re.split(_SENTENCE_BREAK, after)[0]
        if not re.search(rf"(?i)\b{PERSON_WORDS}\b", f"{head} {tail}"):
            return False
        before_cue, after_cue = AGE_GRAMMAR[shape]
        if not ((before_cue and re.search(before_cue, before)) or (after_cue and re.search(after_cue, after))):
            return False
        return not re.search(AGE_NOT_PERSON_AFTER, after)
    if shape.label == "DATEOFBIRTH":
        return bool(re.search(DOB_BREAK, before))
    direct, header = (CARD_DIRECT, CARD_HEADER) if shape.label == "CREDITCARDNUMBER" else (ZIP_DIRECT, ZIP_HEADER)
    if re.search(direct, before):
        return True
    return surface == "cue_csv" and re.fullmatch(header, _csv_header(text, start)) is not None


@dataclass(frozen=True)
class CueCell:
    """One layer A cell: {V} is the gold value."""
    family: str
    shape: CueShape
    surface: str
    language: str
    region: str
    templates: Mapping[str, str]
    make: Callable[[Rng, str, int], str]


@dataclass(frozen=True)
class CueTwin:
    """One layer D cell: {X} is a benign value of the shape. A `near_cue` twin
    writes a card or postal word one clause away from the value."""
    family: str
    shape: CueShape
    surface: str
    language: str
    region: str
    templates: Mapping[str, str]
    make: Callable[[Rng, str, int], str]
    near_cue: bool = False


def _cue_cell(family: str, shape: CueShape, surface: str, language: str, region: str, dev: str, test: str,
              make: Callable[[Rng, str, int], str]) -> CueCell:
    return CueCell(family, shape, f"cue_{surface}", language, region, {"dev": dev, "test": test}, make)


def _cue_twin(family: str, shape: CueShape, surface: str, language: str, region: str, dev: str, test: str,
              make: Callable[[Rng, str, int], str], near_cue: bool = False) -> CueTwin:
    return CueTwin(family, shape, f"cue_{surface}", language, region, {"dev": dev, "test": test}, make, near_cue)


def _age(rng: Rng, partition: str, index: int) -> str:
    """Ages split by partition, so no gold value repeats across them."""
    return str(rng.between(19, 56) if partition == "dev" else rng.between(57, 94))


def _angle(rng: Rng, partition: str, index: int) -> str:
    return rng.choice(("45", "90"))


def _birth_parts(rng: Rng, partition: str) -> tuple[int, int, int]:
    year = rng.between(1940, 1971) if partition == "dev" else rng.between(1972, 2004)
    return year, rng.between(1, 12), rng.between(1, 28)


def _benign_parts(rng: Rng, partition: str) -> tuple[int, int, int]:
    return {"dev": 2025, "test": 2026}[partition], rng.between(1, 12), rng.between(1, 28)


def _us_date(parts: tuple[int, int, int]) -> str:
    year, month, day = parts
    return f"{month}/{day}/{year}"


def _de_date(parts: tuple[int, int, int]) -> str:
    year, month, day = parts
    return f"{day:02d}.{month:02d}.{year}"


def _iso_date(parts: tuple[int, int, int]) -> str:
    year, month, day = parts
    return f"{year}-{month:02d}-{day:02d}"


def _short_card(rng: Rng, partition: str, length: int, valid: bool) -> str:
    prefix = rng.choice(CARD_IINS[partition])
    payload = prefix + rng.digits(length - 1 - len(prefix))
    number = payload + luhn_check_digit(payload)
    return number if valid else _bump_last_digit(number)


def _grouped(number: str) -> str:
    return f"{number[:4]} {number[4:8]} {number[8:]}"


CARD_COMPACT_LENGTHS = (12, 12, 13, 14, 15, 15)


def _card_compact(rng: Rng, partition: str, index: int) -> str:
    """12 to 15 digits, Luhn-valid and Luhn-failing in turn: both at 12 and 15."""
    length = CARD_COMPACT_LENGTHS[index % len(CARD_COMPACT_LENGTHS)]
    return _short_card(rng, partition, length, index % 2 == 0)


def _card_grouped(rng: Rng, partition: str, index: int) -> str:
    return _grouped(_short_card(rng, partition, 12, index % 2 == 0))


def _order_12(rng: Rng, partition: str, index: int) -> str:
    """A Luhn-valid 12-digit number with a card issuer prefix, never a card."""
    return _short_card(rng, partition, 12, True)


def _reference_13_15(rng: Rng, partition: str, index: int) -> str:
    """A card-prefixed 13- to 15-digit reference that fails Luhn."""
    return _short_card(rng, partition, 13 + index % 3, False)


def _grouped_12(rng: Rng, partition: str, index: int) -> str:
    return _grouped(_short_card(rng, partition, 12, True))


def _epoch_ms(rng: Rng, partition: str, index: int) -> str:
    value = "17" + rng.digits(11)
    return _bump_last_digit(value) if luhn_valid(value) else value


def _zip_maker(shape: CueShape) -> Callable[[Rng, str, int], str]:
    def make(rng: Rng, partition: str, index: int) -> str:
        low = partition == "dev"
        if shape is CueShape.ZIP_SE:
            return f"{rng.between(100, 499) if low else rng.between(500, 999)} {rng.digits(2)}"
        if shape is CueShape.ZIP_PL:
            return f"{rng.between(10, 49) if low else rng.between(50, 99)}-{rng.digits(3)}"
        if shape is CueShape.ZIP_SIX:
            return str(rng.between(100000, 499999) if low else rng.between(500000, 999999))
        if shape is CueShape.ZIP_BR:
            return f"{rng.between(10000, 49999) if low else rng.between(50000, 99999)}-{rng.digits(3)}"
        return str(rng.between(100, 499) if low else rng.between(500, 999))
    return make


C_ = CueShape
_zip_se, _zip_pl, _zip_six, _zip_br, _zip_three = (
    _zip_maker(s) for s in (C_.ZIP_SE, C_.ZIP_PL, C_.ZIP_SIX, C_.ZIP_BR, C_.ZIP_THREE)
)
CUE_CELLS = (
    _cue_cell("age_turned_prose", C_.AGE_TURNED, "prose", "en", "US",
              "I just turned {V} and finally booked the trip.",
              "My mother turned {V} last week, so we are hosting lunch.", _age),
    _cue_cell("age_turned_prose_de", C_.AGE_TURNED, "prose", "de", "DE",
              "Meine Tochter ist letzte Woche {V} geworden.",
              "Ich bin im Mai {V} geworden und feiere am Samstag.", _age),
    _cue_cell("age_turned_log", C_.AGE_TURNED, "log_kv", "en", "US",
              'level=info event=profile.note note="customer turned {V} in March" status=ok',
              'svc=crm op=annotate text="client says she turned {V} last month" result=ok', _age),
    _cue_cell("age_at_the_age_of_prose", C_.AGE_AT_THE_AGE_OF, "prose", "en", "US",
              "He retired at the age of {V} after the merger.",
              "My father learned to swim at the age of {V}.", _age),
    _cue_cell("age_at_the_age_of_prose_de", C_.AGE_AT_THE_AGE_OF, "prose", "de", "DE",
              "Meine Oma hat im Alter von {V} Jahren Spanisch gelernt.",
              "Mein Vater ist im Alter von {V} Jahren in Rente gegangen.", _age),
    _cue_cell("age_at_the_age_of_json", C_.AGE_AT_THE_AGE_OF, "tool_json", "en", "US",
              '{"history":{"note":"patient quit smoking at the age of {V}"}}',
              '{"intake":{"summary":"she started running at the age of {V}","verified":true}}', _age),
    _cue_cell("age_yo_prose", C_.AGE_YO, "prose", "en", "US",
              "hi all, i'm {V} y/o and new to the forum",
              "Patient is a {V} y/o with chest pain since Monday.", _age),
    _cue_cell("age_yo_csv", C_.AGE_YO, "csv", "en", "US",
              "id,summary\n3,{V} y/o female smoker\n",
              "case,triage_note\n11,{V} y/o male with fever\n", _age),
    _cue_cell("age_year_old_gender_prose", C_.AGE_YEAR_OLD_GENDER, "prose", "en", "US",
              "This {V} year old female reports knee pain.",
              "A {V}-year-old male presented with a cough.", _age),
    _cue_cell("age_year_old_gender_json", C_.AGE_YEAR_OLD_GENDER, "tool_json", "en", "US",
              '{"triage":{"note":"{V} year old male, no allergies"}}',
              '{"case":{"summary":"{V}-year-old female, stable","priority":2}}', _age),
    _cue_cell("dob_sentence_break_prose", C_.DOB_SENTENCE_BREAK, "prose", "en", "US",
              "Can you confirm your date of birth? It's {V}.",
              "Thanks for waiting. You asked for my date of birth. It is {V}.",
              lambda rng, partition, index: _us_date(_birth_parts(rng, partition))),
    _cue_cell("dob_sentence_break_prose_de", C_.DOB_SENTENCE_BREAK, "prose", "de", "DE",
              "Sie fragten nach meinem Geburtsdatum. Es ist der {V}.",
              "Mein Geburtsdatum? Das ist der {V}.",
              lambda rng, partition, index: _de_date(_birth_parts(rng, partition))),
    _cue_cell("dob_sentence_break_log", C_.DOB_SENTENCE_BREAK, "log_kv", "en", "US",
              "level=info event=call.transcript text=\"agent asked for date of birth. It's {V}\" status=ok",
              'svc=voice op=transcribe utterance="my date of birth? it is {V}" result=ok',
              lambda rng, partition, index: (_us_date if partition == "dev" else _iso_date)(_birth_parts(rng, partition))),
    _cue_cell("card_short_compact_prose", C_.CARD_SHORT_COMPACT, "prose", "en", "US",
              "What is the limit for card {V}?",
              "Please block my debit card number {V} today.", _card_compact),
    _cue_cell("card_short_compact_prose_de", C_.CARD_SHORT_COMPACT, "prose", "de", "DE",
              "Meine Kartennummer lautet {V}.",
              "Maestro {V} wurde gesperrt.", _card_compact),
    _cue_cell("card_short_compact_log", C_.CARD_SHORT_COMPACT, "log_kv", "en", "US",
              "level=warn event=payment.declined card_number={V} reason=limit",
              'svc=pay op=refund card_no="{V}" result=ok', _card_compact),
    _cue_cell("card_short_compact_json", C_.CARD_SHORT_COMPACT, "tool_json", "en", "US",
              '{"payment":{"cardNumber":"{V}","brand":"maestro"}}',
              '{"wallet":{"card":{"number":"{V}"}}}', _card_compact),
    _cue_cell("card_short_grouped_prose", C_.CARD_SHORT_GROUPED, "prose", "en", "US",
              "Maestro {V} expires next month.",
              "My card number is {V}, can you check it?", _card_grouped),
    _cue_cell("card_short_grouped_csv", C_.CARD_SHORT_GROUPED, "csv", "en", "US",
              "holder_ref,card_number,status\nH-11,{V},active\n",
              "account,card_no,state\nA7,{V},blocked\n", _card_grouped),
    _cue_cell("zip_se_prose", C_.ZIP_SE, "prose", "en", "SE",
              "Ship it to the office, ZIP: {V}.",
              "Our postcode is {V} if the courier asks.", _zip_se),
    _cue_cell("zip_pl_csv", C_.ZIP_PL, "csv", "en", "PL",
              "name,postcode,id\nWarehouse,{V},4\n",
              "site,zip,ref\nDepot,{V},9\n", _zip_pl),
    _cue_cell("zip_pl_prose_de", C_.ZIP_PL, "prose", "de", "PL",
              "Die PLZ lautet {V}.",
              "Postleitzahl: {V}, bitte eintragen.", _zip_pl),
    _cue_cell("zip_six_json", C_.ZIP_SIX, "tool_json", "en", "IN",
              '{"address":{"zip":"{V}","country":"IN"}}',
              '{"shipping":{"postalCode":"{V}"}}', _zip_six),
    _cue_cell("zip_br_log", C_.ZIP_BR, "log_kv", "pt", "BR",
              'level=info event=address.verify cep="{V}" status=ok',
              "svc=geo op=lookup zip_code={V} result=hit", _zip_br),
    _cue_cell("zip_three_prose", C_.ZIP_THREE, "prose", "en", "IS",
              "My zip code is {V}.",
              "Postal code: {V}. Thanks!", _zip_three),
)
# Layer D. {X} is benign: it matches the shape's broad pattern and does not
# read as personal.
CUE_TWINS = (
    _cue_twin("age_twin_turned_object", C_.AGE_TURNED, "prose", "en", "US",
              "The old bridge turned {X} this spring.",
              "The company turned {X} in May and opened a new plant.", _age),
    _cue_twin("age_twin_turned_degrees", C_.AGE_TURNED, "prose", "en", "US",
              "She turned {X} degrees to face the door.",
              "He turned {X} degrees and walked back to the car.", _angle),
    _cue_twin("age_twin_turned_object_de", C_.AGE_TURNED, "prose", "de", "DE",
              "Die Firma ist dieses Jahr {X} geworden.",
              "Das Stadion ist im Juni {X} geworden.", _age),
    _cue_twin("age_twin_at_the_age_of_object", C_.AGE_AT_THE_AGE_OF, "prose", "en", "US",
              "The oak was felled at the age of {X}.",
              "The whisky was bottled at the age of {X}.", _age),
    _cue_twin("age_twin_at_the_age_of_clause", C_.AGE_AT_THE_AGE_OF, "prose", "en", "US",
              "The firm, at the age of {X}, was sold to a rival.",
              "The bridge, at the age of {X}, still carries the morning traffic.", _age),
    _cue_twin("age_twin_at_the_age_of_object_de", C_.AGE_AT_THE_AGE_OF, "prose", "de", "DE",
              "Die Eiche wurde im Alter von {X} Jahren gefällt.",
              "Der Wein wurde im Alter von {X} Jahren abgefüllt.", _age),
    _cue_twin("age_twin_yo_object", C_.AGE_YO, "prose", "en", "US",
              "My {X} y/o laptop still boots.",
              "My {X} y/o boiler finally gave up.", _age),
    _cue_twin("age_twin_year_old_object", C_.AGE_YEAR_OLD_GENDER, "prose", "en", "US",
              "We are replacing a {X} year old codebase.",
              "The {X}-year-old building needs a new roof.", _age),
    _cue_twin("age_twin_year_old_animal", C_.AGE_YEAR_OLD_GENDER, "prose", "en", "US",
              "The shelter has a {X} year old female cat for adoption.",
              "They sold the {X}-year-old male horse last week.", _age),
    _cue_twin("dob_twin_optional", C_.DOB_SENTENCE_BREAK, "prose", "en", "US",
              "Date of birth is optional. The form closes {X}.",
              "The date of birth field is hidden. Renewal is due {X}.",
              lambda rng, partition, index: _us_date(_benign_parts(rng, partition))),
    _cue_twin("dob_twin_last_login", C_.DOB_SENTENCE_BREAK, "prose", "en", "US",
              "Please update your date of birth. Last login was {X}.",
              "We could not verify the date of birth. Retry after {X}.",
              lambda rng, partition, index: _us_date(_benign_parts(rng, partition))),
    _cue_twin("card_twin_order", C_.CARD_SHORT_COMPACT, "prose", "en", "US",
              "Order number {X} shipped this morning.",
              "Tracking number {X} is out for delivery.", _order_12),
    _cue_twin("card_twin_timestamp_log", C_.CARD_SHORT_COMPACT, "log_kv", "en", "US",
              "level=info event=job.done ts_ms={X} status=ok",
              'svc=queue op=ack sent_at_ms="{X}" result=ok', _epoch_ms),
    _cue_twin("card_twin_transaction_json", C_.CARD_SHORT_COMPACT, "tool_json", "en", "US",
              '{"transaction":{"id":"{X}","state":"settled"}}',
              '{"shipment":{"trackingId":"{X}"}}', _reference_13_15),
    _cue_twin("card_twin_grouped_ref_csv", C_.CARD_SHORT_GROUPED, "csv", "en", "US",
              "ref,qty\n{X},2\n", "batch,units\n{X},40\n", _grouped_12),
    _cue_twin("card_twin_reader_serial", C_.CARD_SHORT_GROUPED, "prose", "en", "US",
              "Card reader serial {X} needs a firmware update.",
              "The card terminal ID is {X}; restart it tonight.", _grouped_12, near_cue=True),
    _cue_twin("zip_twin_se_batch", C_.ZIP_SE, "prose", "en", "SE",
              "Batch {X} passed QA.", "Seat block {X} is reserved.", _zip_se),
    _cue_twin("zip_twin_se_near", C_.ZIP_SE, "prose", "en", "SE",
              "Postcode lookup failed for batch {X}.",
              "The zip code service rejected ticket {X}.", _zip_se, near_cue=True),
    _cue_twin("zip_twin_pl_error_log", C_.ZIP_PL, "log_kv", "en", "PL",
              "level=error event=job.fail code={X} retry=true",
              'svc=hr op=room booking="{X}" result=ok', _zip_pl),
    _cue_twin("zip_twin_pl_near_de", C_.ZIP_PL, "prose", "de", "PL",
              "Das PLZ-Feld ist leer; Fehler {X} wurde protokolliert.",
              "Postleitzahl fehlt, Fehlercode {X}.", _zip_pl, near_cue=True),
    _cue_twin("zip_twin_six_order_json", C_.ZIP_SIX, "tool_json", "en", "IN",
              '{"order":{"id":"{X}","state":"packed"}}',
              '{"build":{"number":"{X}"}}', _zip_six),
    _cue_twin("zip_twin_six_near_log", C_.ZIP_SIX, "log_kv", "en", "IN",
              "level=warn event=zip.validation.failed order={X}",
              'svc=geo op=postcode_check invoice="{X}" result=skipped', _zip_six, near_cue=True),
    _cue_twin("zip_twin_br_part", C_.ZIP_BR, "prose", "en", "BR",
              "Part {X} is back in stock.", "Invoice {X} was paid.", _zip_br),
    _cue_twin("zip_twin_br_near_csv", C_.ZIP_BR, "csv", "en", "BR",
              "postcode_checked,part\nyes,{X}\n", "zip_verified,sku\nno,{X}\n", _zip_br, near_cue=True),
    _cue_twin("zip_twin_three_near", C_.ZIP_THREE, "prose", "en", "IS",
              "ZIP upload finished in {X} seconds.",
              "The zip archive holds {X} files.", _zip_three, near_cue=True),
)
del C_, _zip_se, _zip_pl, _zip_six, _zip_br, _zip_three

# Layer A card cells carry checksum-invalid twins that contract v2 credits on
# every surface; the gate maps them to their label by family.
CUE_FAMILY_LABELS = {cell.family: cell.shape.label for cell in CUE_CELLS}
# Their layer D twins join the credit guard: a false-positive rise on them
# fails the gate outright.
CREDIT_GUARD_FAMILIES["CREDITCARDNUMBER"] = tuple(sorted({
    *CREDIT_GUARD_FAMILIES["CREDITCARDNUMBER"],
    *(twin.family for twin in CUE_TWINS if twin.shape.label == "CREDITCARDNUMBER"),
}))
# The generator version that added a guarded family. A scorecard measured on an
# older corpus (a displayed release's record) has no such cells and is not
# asked for them; one measured on this version must carry them.
CREDIT_GUARD_SINCE: dict[str, int] = {
    twin.family: 8 for twin in CUE_TWINS if twin.shape.label == "CREDITCARDNUMBER"
}


CREDIT_GUARD_FAMILIES["TAXNUM"] = (*CREDIT_GUARD_FAMILIES["TAXNUM"],
    "gov_twin_tax_eleven", "gov_near_tax_eleven", "gov_twin_tax_grouped", "gov_near_tax_grouped")
CREDIT_GUARD_SINCE.update({family: 9 for family in CREDIT_GUARD_FAMILIES["TAXNUM"] if family.startswith("gov_")})


def guard_families(generator_version: int) -> list[str]:
    """The credit-guard families a corpus of this generator version contains."""
    return sorted({
        family for families in CREDIT_GUARD_FAMILIES.values() for family in families
        if CREDIT_GUARD_SINCE.get(family, 0) <= generator_version
    })


def _cue_records(cells: Sequence[CueCell | CueTwin], partition: str, layer: str) -> list[Record]:
    seed = PARTITION_SEEDS[partition]
    records: list[Record] = []
    for cell in cells:
        rng = Rng(seed, f"{layer}/cue/{cell.family}")
        for index in range(DOCS_PER_CUE_CELL[layer]):
            value = cell.make(rng, partition, index)
            if isinstance(cell, CueCell):
                fields = {"V": (value, cell.shape.label)}
                if cell.shape.label == "CREDITCARDNUMBER":
                    validity = VALID if luhn_valid(value) else INVALID
                else:
                    validity = UNCHECKED
            else:
                fields = {"X": (value, DECOY_PREFIX + "benign")}
                validity = BENIGN
            text, gold, decoys = _fill_with_decoys(cell.templates[partition], fields)
            records.append(Record(
                uid=f"agentic-{partition}-{layer}-{cell.family}-{index:03d}-{cell.surface}",
                partition=partition, layer=layer, family=cell.family, surface=cell.surface,
                validity=validity,
                group=f"{partition}-{layer}-{cell.family}-{index:03d}",
                template=f"cue/{cell.family}/{partition}",
                language=cell.language, region=cell.region,
                text=text, gold=gold, decoys=decoys,
            ))
    return records


def _char_span(text: str, span: Gold) -> tuple[int, int]:
    encoded = text.encode("utf-8")
    start = len(encoded[: span.start].decode("utf-8"))
    return start, start + len(encoded[span.start : span.end].decode("utf-8"))


def _overlaps(pattern: str, text: str, start: int, end: int) -> bool:
    import re

    return any(match.start() < end and start < match.end() for match in re.finditer(pattern, text))


def check_cue_cells(records: Sequence[Record]) -> None:
    """Fail closed unless every A value is whole, in its shape and reads as
    personal, and every shape A scores has layer D twins that do not: its broad
    rule must pay in layer D, and so must its narrow rule where it has one. A
    near-cue twin must carry a card or postal word; any other card or postal
    twin must carry none."""
    import re

    cells = {cell.family: cell for cell in CUE_CELLS}
    twins = {twin.family: twin for twin in CUE_TWINS}
    unused = sorted(t.family for t in CUE_TWINS if t.shape not in {c.shape for c in CUE_CELLS})
    if unused:
        raise LayerError(f"layer D cue twins no layer A cell uses: {unused}")
    scored: dict[CueShape, str] = {}
    paid: set[CueShape] = set()
    narrow_paid: set[CueShape] = set()
    for record in records:
        if not record.surface.startswith("cue_"):
            continue
        if record.layer == LAYER_IDENTIFIERS:
            cell = cells[record.family]
            if record.decoys or len(record.gold) != 1 or record.gold[0].label != cell.shape.label:
                raise LayerError(f"{record.uid}: expected exactly one {cell.shape.label} gold and no decoy")
            start, end = _char_span(record.text, record.gold[0])
            value = record.gold[0].value
            if not re.fullmatch(CUE_SHAPE_PATTERNS[cell.shape], value):
                raise LayerError(f"{record.uid}: {value!r} is not a {cell.shape.value} value")
            if re.search(r"\d$", record.text[:start]) or re.match(r"\d", record.text[end:]):
                raise LayerError(f"{record.uid}: {value!r} touches another digit")
            for kind, patterns in (("broad", CUE_BROAD_PATTERNS), ("narrow", CUE_NARROW_PATTERNS)):
                if cell.shape in patterns and not _overlaps(patterns[cell.shape], record.text, start, end):
                    raise LayerError(f"{record.uid}: the {cell.shape.value} {kind} pattern misses {value!r}")
            if not cue_reading(cell.shape, record.text, start, end, record.surface):
                raise LayerError(f"{record.uid}: {value!r} does not read as a {cell.shape.value} value")
            scored.setdefault(cell.shape, record.uid)
        else:
            twin = twins[record.family]
            if record.gold:
                raise LayerError(f"{record.uid}: a layer D cue twin carries gold")
            if len(record.decoys) != 1:
                raise LayerError(f"{record.uid}: expected one benign decoy")
            start, end = _char_span(record.text, record.decoys[0])
            if cue_reading(twin.shape, record.text, start, end, record.surface):
                raise LayerError(f"{record.uid}: a layer D decoy reads as a {twin.shape.value} value")
            word = {"CREDITCARDNUMBER": CARD_WORD, "ZIPCODE": ZIP_WORD}.get(twin.shape.label)
            if word and bool(re.search(word, record.text)) != twin.near_cue:
                state = "lacks" if twin.near_cue else "carries"
                raise LayerError(f"{record.uid}: a {twin.shape.label} twin {state} a cue word")
            if not _overlaps(CUE_BROAD_PATTERNS[twin.shape], record.text, start, end):
                raise LayerError(f"{record.uid}: the {twin.shape.value} broad pattern misses the decoy")
            paid.add(twin.shape)
            narrow = CUE_NARROW_PATTERNS.get(twin.shape)
            if narrow and _overlaps(narrow, record.text, start, end):
                narrow_paid.add(twin.shape)
    uncovered = sorted(shape.value for shape in set(scored) - paid)
    if uncovered:
        raise LayerError(f"cue shapes with no layer D counterweight: {uncovered}")
    free = sorted(shape.value for shape in set(scored) & set(CUE_NARROW_PATTERNS) - narrow_paid)
    if free:
        raise LayerError(f"cue shapes whose narrow rule pays nothing in layer D: {free}")


# The surface prefix each generator version added. Every earlier document stays
# byte identical, so an older corpus is a filter of the current one.
GENERATOR_ADDITIONS = {4: "adjacent_", 5: "lookalike_", 6: "address_", 7: "tel_", 8: "cue_", 9: "gov_"}


def records_as_of(version: int, records: Iterable[Record]) -> list[Record]:
    """The documents of generator `version`, from the current generator's output."""
    if not 3 <= version <= GENERATOR_VERSION:
        raise LayerError(f"generator version {version} cannot be rebuilt from version {GENERATOR_VERSION}")
    later = tuple(prefix for added, prefix in GENERATOR_ADDITIONS.items() if added > version)
    return [record for record in records if not record.surface.startswith(later)] if later else list(records)


# The committed contract each older generator version was scored under.
HISTORICAL_CONTRACTS = {
    4: Path("docs/reference/benchmarks/scored-labels-agentic-generator-v4.json"),
    5: Path("docs/reference/benchmarks/scored-labels-agentic-generator-v5.json"),
    6: Path("docs/reference/benchmarks/scored-labels-agentic-generator-v6.json"),
    7: Path("docs/reference/benchmarks/scored-labels-agentic-generator-v7.json"),
    8: Path("docs/reference/benchmarks/scored-labels-agentic-generator-v8.json"),
}


def corpus_identity(repo_root: Path, version: int) -> tuple[str, str]:
    """(corpus SHA-256, contract file SHA-256) of the published partition as
    generator `version` built it, rebuilt from committed inputs only."""
    records = records_as_of(version, generate(PUBLISHED_PARTITION))
    contract = SCORED_LABELS_PATH if version == GENERATOR_VERSION else HISTORICAL_CONTRACTS.get(version)
    if contract is None:
        raise LayerError(f"no committed scored-label contract for generator version {version}")
    return (
        hashlib.sha256(corpus_bytes(records)).hexdigest(),
        hashlib.sha256((repo_root / contract).read_bytes()).hexdigest(),
    )


# --------------------------------------------------------------------------
# Layer R: the repeat-value slice. One document repeats a value 2-4 times in
# different positions and shapes (every occurrence is gold) next to decoys:
# ordinary words spelled like a name part, words containing a name part, and
# digit runs shared with a repeated identifier. A pipeline that re-finds known
# values across a document must cover the repeats without touching the decoys.
# Value makers take (rng, partition), so a layer B transcript can reuse them.

REPEAT_HEADERS = {
    "dev": "From: {G} {S} <{E}>\nSubject: Contract renewal\n\n",
    "test": "From: {G} {S} <{E}>\nSubject: Delivery complaint\n\n",
}
REPEAT_BODIES: dict[str, dict[str, tuple[str, ...]]] = {
    "header_signature": {
        "dev": ("Hello team,\nMs {S} asked to renew before Friday. {G} will call back.\n\nBest,\n{G}",),
        "test": ("Hi,\nplease reply to Ms {S} today, she wrote twice.\n\nThanks, {G}",),
    },
    "case_lower": {
        "dev": ("Logged in as {GL} {SL} from the kiosk.",),
        "test": ("Parcel signed by {GL} {SL} at the front desk.",),
    },
    "case_upper": {
        "dev": ("CUSTOMER: {GU} {SU}",),
        "test": ("ACCOUNT HOLDER: {GU} {SU}",),
    },
    "case_nbsp": {
        "dev": ("Assigned to {G}{NB}{S} for review.",),
        "test": ("Contact person: {G}{NB}{S}",),
    },
    "case_linebreak": {
        "dev": ("Best wishes\n{G}\n{S}",),
        "test": ("Kind regards\n{G}\n{S}",),
    },
}
# (slot, name, decoy word, decoy sentence). The name is used as the person's
# given name or surname; the sentence uses the same spelling as a word.
WORD_NAMES = {
    "dev": (
        ("G", "Hope", "Hope", "{W} this helps."),
        ("G", "Mark", "Mark", "{W} the date in your calendar."),
        ("G", "Bill", "Bill", "{W} total is attached."),
        ("G", "Dawn", "Dawn", "{W} shift starts early."),
        ("S", "Hall", "Hall", "Meet in {W} B."),
        ("S", "Rich", "Rich", "Use {W} text formatting."),
    ),
    "test": (
        ("G", "Rose", "Rose", "The {W} garden opens at nine."),
        ("G", "May", "May", "We meet again in {W}."),
        ("G", "Will", "Will", "{W} you confirm the slot?"),
        ("G", "Grant", "Grant", "{W} approved by the board."),
        ("S", "Page", "Page", "See {W} 3 of the report."),
        ("S", "Court", "Court", "The {W} hearing moved, the Government portal has details."),
    ),
}
# (slot, name, containing word, sentence); the file name repeats the word. A
# containing word must not be PII itself: a city (Heidelberg for Berg) is
# location gold in Kiji, so it cannot serve as a decoy.
SUBSTRING_NAMES = {
    "dev": (
        ("G", "Art", "Article", "{W} 5 applies, see files/{WL}_2026.pdf."),
        ("G", "Ed", "Education", "{W} budget approved, see files/{WL}_2026.pdf."),
        ("G", "Sam", "Sample", "{W} size is small, see files/{WL}_2026.pdf."),
        ("S", "Ross", "Crossroads", "Turn at the {W}, map in files/{WL}_2026.pdf."),
        ("S", "Lang", "Language", "{W} settings changed, see files/{WL}_2026.pdf."),
    ),
    "test": (
        ("G", "Ann", "Annual", "The {W} report is due, see files/{WL}_2026.pdf."),
        ("G", "Eva", "Evaluation", "{W} results are attached as files/{WL}_2026.pdf."),
        ("G", "Max", "Maximum", "{W} load was reached, log in files/{WL}_2026.pdf."),
        ("S", "Berg", "Iceberg", "The {W} lettuce is out of stock, see files/{WL}_2026.pdf."),
        ("S", "Hamm", "Hammer", "Bring the {W} tomorrow, list in files/{WL}_2026.pdf."),
    ),
}
REPEAT_SIGNOFFS = {"dev": " Ms {S} will follow up.\n\nBest,\n{G}", "test": " Ms {S} confirmed.\n\nThanks, {G}"}
ID_REPEAT_TEMPLATES = {
    "dev": (
        "Please use {V} for the refund.\n"
        '{"action": "refund", "{K}": "{VC}"}\n'
        "level=info svc=billing {K}={VC} batch={T}\n"
        "Batch {T} is unrelated to the customer."
    ),
    "test": (
        "Hi, the value on file is {V}, please keep it.\n"
        '{"operation": "update", "{K}": "{VC}"}\n'
        "2026-04-17T08:03:51Z INFO sync {K}={VC} job={T}\n"
        "Job {T} finished without errors."
    ),
}
ID_REPEAT_FAMILIES: dict[str, tuple[str, str, str, str]] = {
    # surface: (label, key family, language, region)
    "iban_de": ("IBAN", "iban", "de", "DE"),
    "phone_de": ("TELEPHONENUM", "phone", "de", "DE"),
    "steuer_id": ("TAXNUM", "steuer_id", "de", "DE"),
}


def _repeat_value(surface: str, rng: Rng, partition: str) -> Value:
    if surface == "iban_de":
        return Value(_by_four(_iban_de(rng)))
    if surface == "phone_de":
        return _phone_de(rng, partition)
    return Value(_chunks(_steuer_id(rng), (2, 3, 3, 3)))


def _person_fields(given: str, surname: str, email: str) -> dict[str, tuple[str, str | None]]:
    return {
        "G": (given, "GIVENNAME"),
        "S": (surname, "SURNAME"),
        "E": (email, "EMAIL"),
        "GL": (given.lower(), "GIVENNAME"),
        "SL": (surname.lower(), "SURNAME"),
        "GU": (given.upper(), "GIVENNAME"),
        "SU": (surname.upper(), "SURNAME"),
        "NB": (NBSP, None),
    }


def _name_email(given: str, surname: str, rng: Rng, partition: str) -> str:
    local = _ascii_fold(f"{given}.{surname}").lower()
    return f"{local}@{rng.choice(EMAIL_DOMAINS[partition])}"


def _repeat_record(partition: str, family: str, surface: str, index: int, template: str,
                   template_id: str, fields: Mapping[str, tuple[str, str | None]],
                   language: str, region: str) -> Record:
    text, gold, decoys = _fill_with_decoys(template, fields)
    return Record(
        uid=f"agentic-{partition}-R-{family}-{index:03d}-{surface}-{REPEAT}",
        partition=partition,
        layer=LAYER_REPEATS,
        family=family,
        surface=surface,
        validity=REPEAT,
        group=f"{partition}-R-{family}-{surface}-{index:03d}",
        template=template_id,
        language=language,
        region=region,
        text=text,
        gold=gold,
        decoys=decoys,
    )


def _repeat_records(partition: str, seed: int) -> list[Record]:
    records: list[Record] = []
    header = REPEAT_HEADERS[partition]
    for body_family, bodies in REPEAT_BODIES.items():
        family, surface = (
            ("header_signature", "thread")
            if body_family == "header_signature"
            else ("case_variants", body_family.removeprefix("case_"))
        )
        rng = Rng(seed, f"R/{body_family}")
        for index in range(DOCS_PER_FAMILY):
            given, surname = _person(rng, partition)
            email = _name_email(given, surname, rng, partition)
            body = rng.choice(bodies[partition])
            template = header + body
            records.append(_repeat_record(
                partition, family, surface, index, template,
                f"R/{body_family}/{partition}/{bodies[partition].index(body)}",
                _person_fields(given, surname, email),
                "de" if index % 2 else "en", "DE" if index % 2 else "US",
            ))
    for family, pool in (("word_names", WORD_NAMES), ("substring_names", SUBSTRING_NAMES)):
        rng = Rng(seed, f"R/{family}")
        entries = pool[partition]
        for index in range(DOCS_PER_FAMILY):
            slot, name, word, sentence = entries[index % len(entries)]
            given, surname = _person(rng, partition)
            if slot == "G":
                given = name
            else:
                surname = name
            email = _name_email(given, surname, rng, partition)
            fields = {
                **_person_fields(given, surname, email),
                "W": (word, DECOY_PREFIX + family),
                "WL": (word.lower(), DECOY_PREFIX + family),
            }
            template = header + sentence + REPEAT_SIGNOFFS[partition]
            records.append(_repeat_record(
                partition, family, "thread", index, template,
                f"R/{family}/{partition}/{entries.index(entries[index % len(entries)])}",
                fields, "en", "US",
            ))
    rng = Rng(seed, "R/id_repeats")
    template = ID_REPEAT_TEMPLATES[partition]
    for surface, (label, key_family, language, region) in ID_REPEAT_FAMILIES.items():
        for index in range(DOCS_PER_FAMILY):
            value = _repeat_value(surface, rng, partition)
            digits = _only_digits(value.render())
            shared = digits[2:8]
            fields = {
                "V": (value.render(), label),
                "VC": (value.render(""), label),
                "K": (rng.choice(KEYS[key_family][partition]), None),
                "T": (shared, DECOY_PREFIX + "shared_digits"),
            }
            records.append(_repeat_record(
                partition, "id_repeats", surface, index, template,
                f"R/id_repeats/{partition}/0", fields, language, region,
            ))
    return records


def generate(partition: str) -> list[Record]:
    if partition not in PARTITIONS:
        raise LayerError(f"unknown partition {partition!r}")
    seed = PARTITION_SEEDS[partition]
    records = (
        _identifier_records(partition, seed)
        + _lookalike_records(partition, seed)
        + _repeat_records(partition, seed)
        + _adjacency_records(partition, LAYER_IDENTIFIERS)
        + _adjacency_records(partition, LAYER_LOOKALIKES)
        + _labelled_lookalike_records(LOOKALIKE_GOLD_CELLS, partition, LAYER_IDENTIFIERS)
        + _labelled_lookalike_records(LOOKALIKE_TWINS, partition, LAYER_LOOKALIKES)
        + _address_records(ADDRESS_CELLS, partition, LAYER_IDENTIFIERS)
        + _address_records(ADDRESS_TWINS, partition, LAYER_LOOKALIKES)
        + _phone_records(PHONE_CELLS, partition, LAYER_IDENTIFIERS)
        + _phone_records(PHONE_TWINS, partition, LAYER_LOOKALIKES)
        + _cue_records(CUE_CELLS, partition, LAYER_IDENTIFIERS)
        + _cue_records(CUE_TWINS, partition, LAYER_LOOKALIKES)
        + government_ids.records(sys.modules[__name__], partition)
    )
    check_lookalike_pairs(records)
    check_address_cells(records)
    check_phone_cells(records)
    check_cue_cells(records)
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


def load_contract(
    repo_root: Path, path: Path | None = None, version: int = GENERATOR_VERSION
) -> score.ScoredLabelContract:
    """The contract that rules on generator `version` (default: the current one).

    A record measured on an older generator is rescored under the contract
    committed for that version, which rules on exactly the labels it emits.
    """
    if path is None and version != GENERATOR_VERSION and version not in HISTORICAL_CONTRACTS:
        raise LayerError(f"no committed scored-label contract for generator version {version}")
    relative = path or (SCORED_LABELS_PATH if version == GENERATOR_VERSION else HISTORICAL_CONTRACTS[version])
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
    if not isinstance(corpus, dict) or corpus.get("generator_version") != version:
        raise LayerError(
            f"{display} rules on generator_version {corpus.get('generator_version') if isinstance(corpus, dict) else None!r}, "
            f"but the generator is version {version}"
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
    repeats: list[score.Document]


def prepare(repo_root: Path, contract_path: Path | None = None) -> PreparedLayers:
    records = generate(PUBLISHED_PARTITION)
    contract = load_contract(repo_root, contract_path)
    documents = apply_contract([record.to_document() for record in records], contract)
    identifiers = [d for d in documents if d.cell and d.cell.startswith(LAYER_IDENTIFIERS + "|")]
    lookalikes = [d for d in documents if d.cell and d.cell.startswith(LAYER_LOOKALIKES + "|")]
    repeats = [d for d in documents if d.cell and d.cell.startswith(LAYER_REPEATS + "|")]
    if any(d.spans for d in lookalikes):
        raise LayerError("a layer D lookalike document carries gold")
    if len(identifiers) + len(lookalikes) + len(repeats) != len(documents):
        raise LayerError("a generated document belongs to no layer")
    return PreparedLayers(
        manifest(PUBLISHED_PARTITION, records), contract, identifiers, lookalikes, repeats
    )


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
    run_r = _layer_run(scorecard, LAYER_REPEATS, config)
    lines += ["", "**Layer R, repeat-value slice**", "",
              "| Family | Shape | Gold bytes | Leaked bytes | Byte recall | False-positive bytes |",
              "| --- | --- | ---: | ---: | ---: | ---: |"]
    for key, block in sorted(run_r.get("per_cell", {}).items()):
        _, family, surface, _ = key.split("|")
        utf8 = block["utf8_bytes"]
        lines.append(
            f"| {family} | {surface} | {utf8['pii']} | {utf8['leaked']} | "
            f"{utf8['recall']:.2f} | {utf8['false_positive']} |"
        )
    return "\n".join(lines) + "\n"


GATE_LAYERS = ("C", LAYER_IDENTIFIERS, LAYER_LOOKALIKES, LAYER_REPEATS)

DEPENDENCY_OWNERS = {
    "policy.rulepacks.paths": "policy",
    "policy.custom_recognizers": "policy",
    "ner.model_dir": "ner",
    "safety_net.nym.model_dir": "safety_net",
    "dob_judge.model_dir": "dob_judge",
    "davlan-mbert-ner-hrl-onnx": "ner",
    "nym-small-int8": "safety_net",
    "gliner-multi-pii-dob-int8": "dob_judge",
}


def _dependency_owner(reference: str) -> str | None:
    for prefix, owner in DEPENDENCY_OWNERS.items():
        if reference == prefix or reference.startswith(prefix + "[") or reference.startswith(prefix + "/"):
            return owner
    return None


def policy_dependency_files(policy: Mapping[str, object], working_dir: Path) -> dict[str, str]:
    """Hash external policy inputs by logical reference, independent of worktree path."""
    files: dict[str, str] = {}

    def resolved(value: object, reference: str) -> Path:
        if not isinstance(value, str) or not value:
            raise LayerError(f"invalid {reference} path in policy")
        path = Path(value).expanduser()
        return path if path.is_absolute() else working_dir / path

    def add_file(value: object, reference: str) -> Path:
        if _dependency_owner(reference) is None:
            raise LayerError(f"unknown policy dependency reference {reference}")
        path = resolved(value, reference)
        try:
            if not path.is_file():
                raise OSError("not a regular file")
            files[reference] = score.sha256_file(path)
        except OSError as error:
            raise LayerError(f"cannot hash policy input {reference} at {path}: {error}") from error
        return path

    def add_dir(value: object, reference: str) -> None:
        directory = resolved(value, reference)
        if not directory.is_dir():
            raise LayerError(f"cannot hash policy model directory {reference} at {directory}")
        hashed_any = False
        for path in sorted(directory.rglob("*")):
            if any(part.startswith(".") for part in path.relative_to(directory).parts):
                continue
            if path.is_symlink():
                raise LayerError(f"policy model directory {reference} contains a symlink: {path}")
            if path.is_file():
                add_file(str(path), f"{reference}/{path.relative_to(directory).as_posix()}")
                hashed_any = True
        if not hashed_any:
            raise LayerError(f"policy model directory {reference} has no hashable files: {directory}")

    rules = policy.get("policy", {})
    if isinstance(rules, dict):
        rulepacks = rules.get("rulepacks", {})
        if isinstance(rulepacks, dict):
            for index, path in enumerate(rulepacks.get("paths", [])):
                reference = f"policy.rulepacks.paths[{index}]"
                rulepack_path = add_file(path, reference)
                try:
                    rulepack = tomllib.loads(rulepack_path.read_text(encoding="utf-8"))
                except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
                    raise LayerError(f"cannot parse policy rulepack {rulepack_path}: {error}") from error
                for recognizer_index, recognizer in enumerate(rulepack.get("recognizers", [])):
                    match = recognizer.get("match") if isinstance(recognizer, dict) else None
                    if isinstance(match, dict) and "terms_file" in match:
                        add_file(match["terms_file"],
                                 f"{reference}.recognizers[{recognizer_index}].terms_file")
        for index, recognizer in enumerate(rules.get("custom_recognizers", [])):
            if isinstance(recognizer, dict) and "terms_file" in recognizer:
                add_file(recognizer["terms_file"], f"policy.custom_recognizers[{index}].terms_file")
    for section in ("ner", "dob_judge"):
        config = policy.get(section)
        if isinstance(config, dict) and config.get("model_dir") and (
            section != "dob_judge" or config.get("enabled") is True
        ):
            add_dir(config["model_dir"], f"{section}.model_dir")
    safety_net = policy.get("safety_net")
    if isinstance(safety_net, dict) and safety_net.get("backend") == "nym":
        nym = safety_net.get("nym")
        if not isinstance(nym, dict) or not nym.get("model_dir"):
            raise LayerError("active Nym policy has no model_dir")
        add_dir(nym["model_dir"], "safety_net.nym.model_dir")
    return dict(sorted(files.items()))


def policy_dependency_provenance(
    policy_path: Path, working_dir: Path, policy_data: Mapping[str, object] | None = None,
) -> dict[str, object]:
    if policy_data is None:
        try:
            policy_data = tomllib.loads(policy_path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
            raise LayerError(f"cannot read policy dependencies from {policy_path}: {error}") from error
    return {"files": policy_dependency_files(policy_data, working_dir)}


def model_bundle_identity(bundles: object) -> dict[str, str]:
    if not isinstance(bundles, list):
        raise LayerError("scorecard has no model-bundle provenance")
    identity: dict[str, str] = {}
    for bundle in bundles:
        if not isinstance(bundle, dict):
            raise LayerError("invalid model-bundle provenance")
        model_id, digest = bundle.get("model_id"), bundle.get("observed_sha256")
        if (not isinstance(model_id, str) or not model_id or not isinstance(digest, str)
                or len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest)):
            raise LayerError("invalid model-bundle provenance")
        if model_id in identity:
            raise LayerError(f"duplicate model-bundle provenance: {model_id}")
        identity[model_id] = digest
    return dict(sorted(identity.items()))


def policy_dependency_identity(scorecard: Mapping[str, object], allow_legacy: bool) -> dict[str, object] | None:
    provenance = scorecard.get("runner_provenance")
    dependencies = provenance.get("policy_dependencies") if isinstance(provenance, dict) else None
    if dependencies is None and allow_legacy:
        return None
    if not isinstance(dependencies, dict) or not isinstance(dependencies.get("files"), dict):
        raise LayerError("scorecard has no policy-dependency identity; measure it again or use --allow-legacy-policy-inputs")
    files = dependencies["files"]
    if any(not isinstance(k, str) or not isinstance(v, str) or len(v) != 64
           or any(char not in "0123456789abcdef" for char in v) for k, v in files.items()):
        raise LayerError("scorecard has invalid policy-dependency file digests")
    bundles = model_bundle_identity(provenance.get("model_bundles"))
    return {"files": files, "model_bundles": bundles}


def _layer_identity(scorecard: Mapping[str, object]) -> dict[str, object]:
    layers = scorecard.get("layers")
    if not isinstance(layers, dict):
        raise LayerError(
            "scorecard has no agentic layers: it predates them or was run with "
            "--no-agentic-layers; measure the base again on this harness"
        )
    gold_validity = layers.get("gold_validity")
    if not isinstance(gold_validity, dict) or gold_validity.get("C") is None:
        raise LayerError(
            "scorecard predates the gold-validity digest; measure the base again on this harness"
        )
    parameters = scorecard.get("parameters", {})
    identity = {
        "kiji_contract": score.scorecard_scored_label_contract_identity(scorecard),
        "kiji_dataset": scorecard.get("dataset", {}).get("integrity"),
        "corpus_sha256": layers.get("generator", {}).get("corpus_sha256"),
        "layer_contract": layers.get("scored_label_contract", {}).get("file_sha256"),
        "layer_c_gold_validity": gold_validity["C"],
        "configs": parameters.get("configs"),
        "policy_sha256": parameters.get("policy_sha256"),
        "ner_threshold": parameters.get("ner_threshold"),
    }
    missing = [
        key for key, value in identity.items()
        if (value is None and key != "ner_threshold") or (
            key == "kiji_contract"
            and None in value
            and value != (score.SCORED_LABEL_CONTRACT_V1_ID, 1, None)
        )
    ]
    if "ner_threshold" not in parameters:
        missing.append("ner_threshold")
    if missing:
        raise LayerError(
            f"scorecard has no gate identity for {', '.join(missing)}; "
            "measure the base again on this harness"
        )
    return identity


def gold_validity_digest(
    documents: Sequence[score.Document], validator_measurements: Mapping[str, object]
) -> dict[str, object]:
    """SHA-256 over every gold span's validator verdict (document, span, label).

    Gold validity is a property of the gold, but the probe that decides it is
    built from the measured tree. Two scorecards are gate-comparable only when
    this digest matches, so a PR that changes a validator cannot relabel the
    gold it now leaks as "failed its checksum" and drop it from the gate.
    """
    responses = validator_measurements["documents"]
    rows = sorted(
        [document.uid, span.start, span.end, span.label,
         bool(validation["applicable"]), validation["validator_passed"] is True]
        for document in documents
        for span, validation in zip(
            document.spans, responses[document.uid]["gold_validation"], strict=True
        )
    )
    payload = json.dumps(rows, separators=(",", ":")).encode("utf-8")
    return {"algorithm": "sha256", "entities": len(rows), "value": hashlib.sha256(payload).hexdigest()}


def layer_totals(scorecard: Mapping[str, object], config: str) -> dict[str, dict[str, int]]:
    """Per layer: gated leaked bytes, FP bytes, refusals and restore counts.

    Gold that fails its own checksum stays scored in the headline, but only a
    rule without a checksum can reach it, so it is reported (`twin_leaked`)
    and never gated (user decision 2026-09-26). Layer A excludes its
    checksum-invalid twins; layer C excludes Kiji gold its validator fails,
    per label from the validator split. Layers D and R have no such gold.
    Layer A credits IBAN/card invalid twins on every surface and the 2026-09-28
    classes only on cued surfaces. Layer C lacks cue metadata and credits all
    validator-failed gold for those labels; see the gate documentation.
    Each row also carries `guard_false_positive`: layer D false-positive bytes
    per CREDIT_GUARD_FAMILIES family (empty for other layers).
    """
    family_labels = {family.name: family.label for family in IDENTIFIER_FAMILIES} | CUE_FAMILY_LABELS | government_ids.FAMILY_LABELS
    # A scorecard that does not say which corpus it measured is held to the current one.
    version = scorecard.get("layers", {}).get("generator", {}).get("generator_version", GENERATOR_VERSION)
    if type(version) is not int:
        raise LayerError("scorecard layers carry no integer generator_version")
    required_guard_families = guard_families(version)
    totals: dict[str, dict[str, int]] = {}
    for layer in GATE_LAYERS:
        run = _layer_run(scorecard, layer, config)
        utf8 = run["metrics"]["utf8_bytes"]
        contract = run.get("pipeline_contract")
        if not isinstance(contract, dict):
            raise LayerError(f"layer {layer} has no pipeline_contract; measure this scorecard again")
        for field in ("documents", "restore_exact_documents", "manifest_valid_documents"):
            if type(contract.get(field)) is not int or contract[field] < 0:
                raise LayerError(f"layer {layer} has no valid pipeline_contract.{field}; measure this scorecard again")
        if any(contract[field] > contract["documents"] for field in
               ("restore_exact_documents", "manifest_valid_documents")):
            raise LayerError(f"layer {layer} pipeline_contract counts exceed documents")
        availability = run.get("pipeline_availability")
        if not isinstance(availability, dict):
            raise LayerError(f"layer {layer} has no pipeline_availability; measure this scorecard again")
        for field in ("attempted_documents", "completed_documents", "failed_closed_documents"):
            if type(availability.get(field)) is not int or availability[field] < 0:
                raise LayerError(f"layer {layer} has no valid pipeline_availability.{field}; measure this scorecard again")
        if (availability["completed_documents"] != contract["documents"] or
                availability["completed_documents"] + availability["failed_closed_documents"] !=
                availability["attempted_documents"]):
            raise LayerError(f"layer {layer} pipeline document counts disagree")
        twin_leaked = 0
        if layer == LAYER_IDENTIFIERS:
            twin_leaked = sum(
                block["utf8_bytes"]["leaked"]
                for cell, block in run["per_cell"].items()
                if cell.split("|")[3] == INVALID
                and not invalid_twin_credited(family_labels.get(cell.split("|")[1]),
                                               cell.split("|")[2])
            )
        elif layer == "C":
            by_label = run.get("validator_recall_by_label")
            if not isinstance(by_label, dict):
                raise LayerError(
                    "layer C has no validator split; the gate cannot separate "
                    "validator-failed gold"
                )
            twin_leaked = sum(
                block["production_recall_by_gold_validity"]["validator_failed_gold"]["leaked_utf8_bytes"]
                for label, block in by_label.items()
                if block.get("production_recall_by_gold_validity")
                and label not in CREDIT_SCOPE_BY_LABEL
            )
        guard_false_positive: dict[str, int] = {}
        if layer == LAYER_LOOKALIKES:
            cells = run.get("per_cell")
            if not isinstance(cells, dict):
                raise LayerError("layer D has no per_cell; the credit guard cannot be checked")
            for family in required_guard_families:
                blocks = [block for cell, block in cells.items() if cell.split("|")[1] == family]
                if not blocks:
                    raise LayerError(f"layer D has no {family} cells; the credit guard cannot be checked")
                guard_false_positive[family] = sum(block["utf8_bytes"]["false_positive"] for block in blocks)
        totals[layer] = {
            "attempted": availability["attempted_documents"],
            "documents": contract["documents"],
            "headline_leaked": utf8["leaked"],
            "leaked": utf8["leaked"] - twin_leaked,
            "twin_leaked": twin_leaked,
            "false_positive": utf8["false_positive"],
            "failed_closed": availability["failed_closed_documents"],
            "restore_exact": contract["restore_exact_documents"],
            "manifest_valid": contract["manifest_valid_documents"],
            "guard_false_positive": guard_false_positive,
        }
    return totals


def credit_guard_rise(base: Mapping[str, Mapping[str, object]],
                      candidate: Mapping[str, Mapping[str, object]]) -> list[str]:
    """CREDIT_GUARD_FAMILIES families whose layer D FP bytes rose. Fails closed
    when either side lacks a guarded family's count."""
    rose = []
    for label, families in sorted(CREDIT_GUARD_FAMILIES.items()):
        for family in families:
            if CREDIT_GUARD_SINCE.get(family) and all(
                family not in (totals[LAYER_LOOKALIKES].get("guard_false_positive") or {})
                for totals in (base, candidate)
            ):
                continue  # both sides measured a corpus older than this family
            counts = []
            for side, totals in (("base", base), ("candidate", candidate)):
                guard = totals[LAYER_LOOKALIKES].get("guard_false_positive")
                if not isinstance(guard, Mapping) or type(guard.get(family)) is not int:
                    raise LayerError(f"{side} has no layer D {family} FP count; the credit guard cannot be checked")
                counts.append(guard[family])
            if counts[1] > counts[0]:
                rose.append(f"{family} ({label}: {counts[0]} -> {counts[1]})")
    return rose


def decide(base: Mapping[str, Mapping[str, int]], candidate: Mapping[str, Mapping[str, int]]) -> dict[str, object]:
    """The net-bytes rule gate (user decision 2026-09-26), per contract.

    Fail when any layer leaks more (gated or headline bytes) or refuses more. A leak fix passes only when
    its summed FP-byte increase over all layers is smaller than its summed
    leaked-byte decrease. With no leak change, an FP-only fix passes when the
    summed FP bytes fall.
    """
    rows = {
        layer: {
            "attempted_base": base[layer]["attempted"],
            "attempted_candidate": candidate[layer]["attempted"],
            "documents_base": base[layer]["documents"],
            "documents_candidate": candidate[layer]["documents"],
            "headline_leaked_base": base[layer]["headline_leaked"],
            "headline_leaked_candidate": candidate[layer]["headline_leaked"],
            "leaked_base": base[layer]["leaked"],
            "leaked_candidate": candidate[layer]["leaked"],
            "twin_leaked_base": base[layer]["twin_leaked"],
            "twin_leaked_candidate": candidate[layer]["twin_leaked"],
            "false_positive_base": base[layer]["false_positive"],
            "false_positive_candidate": candidate[layer]["false_positive"],
            "failed_closed_base": base[layer]["failed_closed"],
            "failed_closed_candidate": candidate[layer]["failed_closed"],
            "restore_exact_base": base[layer]["restore_exact"],
            "restore_exact_candidate": candidate[layer]["restore_exact"],
            "restore_failures_base": base[layer]["documents"] - base[layer]["restore_exact"],
            "restore_failures_candidate": candidate[layer]["documents"] - candidate[layer]["restore_exact"],
            "manifest_valid_base": base[layer]["manifest_valid"],
            "manifest_valid_candidate": candidate[layer]["manifest_valid"],
            "manifest_invalid_base": base[layer]["documents"] - base[layer]["manifest_valid"],
            "manifest_invalid_candidate": candidate[layer]["documents"] - candidate[layer]["manifest_valid"],
        }
        for layer in GATE_LAYERS
    }
    # Any rise fails, on the gated bytes or on the headline (all gold): a
    # regression must not hide inside gold the gate leaves out.
    leak_rise = [
        l for l, r in rows.items()
        if r["leaked_candidate"] > r["leaked_base"]
        or r["headline_leaked_candidate"] > r["headline_leaked_base"]
    ]
    refusal_rise = [l for l, r in rows.items() if r["failed_closed_candidate"] > r["failed_closed_base"]]
    guard_rise = credit_guard_rise(base, candidate)
    restore_drop = [l for l, r in rows.items() if r["restore_exact_candidate"] < r["restore_exact_base"]]
    manifest_drop = [l for l, r in rows.items() if r["manifest_valid_candidate"] < r["manifest_valid_base"]]
    restore_failure_rise = [l for l, r in rows.items()
                            if r["restore_failures_candidate"] > r["restore_failures_base"]]
    manifest_failure_rise = [l for l, r in rows.items()
                             if r["manifest_invalid_candidate"] > r["manifest_invalid_base"]]
    restore_gain = [l for l, r in rows.items() if r["documents_candidate"] == r["documents_base"]
                    and r["restore_exact_candidate"] > r["restore_exact_base"]]
    manifest_gain = [l for l, r in rows.items() if r["documents_candidate"] == r["documents_base"]
                     and r["manifest_valid_candidate"] > r["manifest_valid_base"]]
    refusal_drop = [l for l, r in rows.items() if r["failed_closed_candidate"] < r["failed_closed_base"]]
    leak_drop = sum(r["leaked_base"] - r["leaked_candidate"] for r in rows.values())
    fp_rise = sum(r["false_positive_candidate"] - r["false_positive_base"] for r in rows.values())
    summary = {"leaked_bytes_decrease": leak_drop, "false_positive_bytes_increase": fp_rise}
    population_change = [l for l, r in rows.items() if r["attempted_candidate"] != r["attempted_base"]]
    if population_change:
        verdict, reason = "fail", f"attempted document counts differ in {population_change}"
    elif refusal_rise:
        verdict, reason = "fail", f"failed-closed documents rose in {refusal_rise}"
    elif restore_drop:
        verdict, reason = "fail", f"exact-restore documents fell in {restore_drop}"
    elif manifest_drop:
        verdict, reason = "fail", f"valid-manifest documents fell in {manifest_drop}"
    elif restore_failure_rise:
        verdict, reason = "fail", f"exact-restore failures rose in {restore_failure_rise}"
    elif manifest_failure_rise:
        verdict, reason = "fail", f"invalid-manifest documents rose in {manifest_failure_rise}"
    elif leak_rise:
        verdict, reason = "fail", f"leaked bytes rose in {leak_rise}"
    elif guard_rise:
        verdict, reason = "fail", (
            f"credit guard: FP bytes rose on {guard_rise}, the benign twin of credited "
            "checksum-invalid gold; no net-bytes gain offsets it"
        )
    elif leak_drop > 0:
        if fp_rise < leak_drop:
            verdict, reason = "pass", f"leaked bytes fell by {leak_drop}, FP bytes changed by {fp_rise:+d}"
        else:
            verdict, reason = "fail", (
                f"net bytes: FP bytes rose by {fp_rise}, not less than the {leak_drop} "
                "leaked bytes saved"
            )
    elif fp_rise < 0:
        reason = f"FP bytes fell by {-fp_rise}"
        if restore_gain or manifest_gain:
            reason += f"; exact restore rose in {restore_gain}; valid manifests rose in {manifest_gain}"
        verdict = "pass"
    elif (restore_gain or manifest_gain) and fp_rise == 0:
        verdict, reason = "pass", (
            f"reversibility improved: exact restore rose in {restore_gain}; "
            f"valid manifests rose in {manifest_gain}"
        )
    elif (restore_gain or manifest_gain) and fp_rise > 0:
        verdict, reason = "fail", f"FP bytes rose by {fp_rise} despite reversibility gain"
    elif refusal_drop:
        verdict, reason = "fail", f"failed-closed documents fell in {refusal_drop}, without an eligible gain"
    else:
        verdict, reason = "fail", "no layer's leaked bytes fell and FP bytes did not fall"
    credit_guard = {
        family: {"base": base[LAYER_LOOKALIKES]["guard_false_positive"][family],
                 "candidate": candidate[LAYER_LOOKALIKES]["guard_false_positive"][family]}
        for families in CREDIT_GUARD_FAMILIES.values() for family in families
        if family in base[LAYER_LOOKALIKES]["guard_false_positive"]
    }
    return {"verdict": verdict, "reason": reason, "summary": summary, "layers": rows,
            "gate_credit_version": GATE_CREDIT_VERSION,
            "credit_guard": credit_guard}


def _scorecard_policy(scorecard: Mapping[str, object], label: str) -> tuple[dict[str, object], str]:
    provenance = scorecard.get("runner_provenance") or {}
    policy = provenance.get("policy") or {}
    parameters = scorecard.get("parameters") or {}
    path = policy.get("path")
    recorded = policy.get("sha256")
    if not isinstance(path, str) or not isinstance(recorded, str):
        raise LayerError(f"{label} scorecard has no policy path and SHA-256 provenance")
    try:
        raw = Path(path).expanduser().read_bytes()
    except OSError as error:
        raise LayerError(f"cannot read {label} policy {path}: {error}") from error
    digest = hashlib.sha256(raw).hexdigest()
    if digest != recorded or digest != parameters.get("policy_sha256"):
        raise LayerError(f"{label} policy file differs from its scorecard SHA-256")
    try:
        return tomllib.loads(raw.decode("utf-8")), digest
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise LayerError(f"invalid {label} policy TOML: {error}") from error


def _toml_equal(left: object, right: object) -> bool:
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(
            _toml_equal(left[key], right[key]) for key in left
        )
    if isinstance(left, list):
        return len(left) == len(right) and all(
            _toml_equal(a, b) for a, b in zip(left, right, strict=True)
        )
    if isinstance(left, str):
        return normalize_home_path(left) == normalize_home_path(right)
    return left == right


def normalize_home_path(value: str) -> str:
    """Keep paths below the current home portable without changing other values."""
    path = Path(value)
    if not path.is_absolute():
        return value
    try:
        relative = path.relative_to(Path.home())
    except ValueError:
        return value
    return str(Path("~") / relative)


def _policy_delta_comparison(
    base: Mapping[str, object], candidate: Mapping[str, object], delta_path: Path
) -> tuple[bool, str, dict[str, str], set[str]]:
    base_policy, base_sha = _scorecard_policy(base, "base")
    candidate_policy, candidate_sha = _scorecard_policy(candidate, "candidate")
    try:
        raw = delta_path.read_bytes()
    except OSError as error:
        raise LayerError(f"cannot read declared policy delta {delta_path}: {error}") from error
    digests = {
        "base": base_sha,
        "candidate": candidate_sha,
        "delta": hashlib.sha256(raw).hexdigest(),
    }
    try:
        delta = tomllib.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise LayerError(f"invalid declared policy delta TOML: {error}") from error
    if not delta or any(not isinstance(section, dict) for section in delta.values()):
        return False, "policy delta must declare at least one TOML section", digests, set()
    existing = sorted(base_policy.keys() & delta.keys())
    if existing:
        return False, f"policy delta changes existing base sections: {existing}", digests, set()
    if not _toml_equal(candidate_policy, {**base_policy, **delta}):
        return False, "candidate policy differs beyond the declared new sections", digests, set()
    return True, "candidate policy equals base plus declared new sections", digests, set(delta)


def _dependency_difference(
    base: dict[str, object] | None, candidate: dict[str, object] | None,
    added_sections: set[str],
) -> list[str]:
    if base is None or candidate is None:
        return [] if base is candidate else ["policy_dependencies"]
    differing: list[str] = []
    for reference in sorted(base["files"].keys() | candidate["files"].keys()):
        if base["files"].get(reference) != candidate["files"].get(reference):
            owner = _dependency_owner(reference)
            if not (owner in added_sections and reference not in base["files"]):
                differing.append(f"policy input {reference}")
    for model_id in sorted(base["model_bundles"].keys() | candidate["model_bundles"].keys()):
        if base["model_bundles"].get(model_id) != candidate["model_bundles"].get(model_id):
            if _dependency_owner(model_id) not in added_sections:
                differing.append(f"model bundle {model_id}")
    return differing


def gate(
    base: Mapping[str, object], candidate: Mapping[str, object], config: str | None = None,
    policy_delta: Path | None = None,
    allow_legacy_policy_inputs: bool = False,
) -> dict[str, object]:
    """Identity check, then `decide` on the production arm's layer totals."""
    base_identity = _layer_identity(base)
    candidate_identity = _layer_identity(candidate)
    delta_result: dict[str, object] = {}
    policy_ok = True
    added_sections: set[str] = set()
    if policy_delta is not None:
        policy_ok, reason, digests, added_sections = _policy_delta_comparison(base, candidate, policy_delta)
        delta_result = {
            "policy_digests": digests,
            "policy_delta_reason": reason,
            "policy_delta_path": str(policy_delta),
        }
    differing = sorted(
        key for key in base_identity
        if base_identity[key] != candidate_identity[key]
        and (key != "policy_sha256" or policy_delta is None)
        and (key != "ner_threshold" or "ner" not in added_sections)
    )
    base_dependencies = policy_dependency_identity(base, allow_legacy_policy_inputs)
    candidate_dependencies = policy_dependency_identity(candidate, allow_legacy_policy_inputs)
    differing.extend(_dependency_difference(base_dependencies, candidate_dependencies, added_sections))
    if not policy_ok or differing:
        if not policy_ok:
            differing.append("policy_sha256")
        return {"verdict": "not_comparable", "differing": sorted(differing), "layers": {}, **delta_result}
    config = config or production_config(candidate)
    result = decide(layer_totals(base, config), layer_totals(candidate, config))
    return {**result, "config": config, **delta_result}


def gate_markdown(result: Mapping[str, object]) -> str:
    policy_differs = "policy_sha256" in result.get("differing", ()) and "policy_delta_reason" in result
    explanation = (
        f"{result['policy_delta_reason']}; differing: {result['differing']}"
        if policy_differs else None
    )
    detail = explanation or result.get("reason", result.get("differing"))
    if "policy_delta_path" in result:
        detail = f"{detail}; policy delta: {result['policy_delta_path']}"
    lines = [f"Verdict: **{result['verdict']}** ({detail})", ""]
    if "policy_digests" in result:
        lines += ["Policy SHA-256 digests:"]
        lines += [f"- {name}: `{digest}`" for name, digest in result["policy_digests"].items()]
        lines.append("")
    if result["layers"]:
        lines += ["| Layer | Leaked base | Leaked cand | FP base | FP cand | Failed closed base | Failed closed cand | Restore base | Restore cand | Valid manifest base | Valid manifest cand | Twin leak base | Twin leak cand |",
                  "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"]
        for layer, r in result["layers"].items():
            lines.append(
                f"| {layer} | {r['leaked_base']} | {r['leaked_candidate']} | {r['false_positive_base']} | "
                f"{r['false_positive_candidate']} | {r['failed_closed_base']} | {r['failed_closed_candidate']} | "
                f"{r['restore_exact_base']} | {r['restore_exact_candidate']} | "
                f"{r['manifest_valid_base']} | {r['manifest_valid_candidate']} | "
                f"{r['twin_leaked_base']} | {r['twin_leaked_candidate']} |"
            )
        guard = result.get("credit_guard", {})
        if guard:
            lines += ["", "Credit guard (layer D FP bytes on the benign twin of credited "
                      "checksum-invalid gold; any rise fails): " + ", ".join(
                          f"{family} {counts['base']} -> {counts['candidate']}"
                          for family, counts in guard.items())]
        lines += ["", f"Gate credit contract v{GATE_CREDIT_VERSION}: gated leak excludes gold that fails its checksum "
                  "(layer A twins, layer C validator-failed Kiji gold), except IBAN and card "
                  "on all surfaces and TAXNUM, CPF, BSN, and NHS twins only on cued layer A surfaces. "
                  "Phone has no layer A family; layer C credits all five newer labels without a cue split; "
                  "the excluded bytes are reported in the twin columns. "
                  "The gate is necessary, not sufficient: review still judges precision."]
    return "\n".join(lines) + "\n"


# --------------------------------------------------------------------------


def _load_json(path: Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def _measure(args: argparse.Namespace) -> None:
    """Layers only, for a binary this tree did not build (a past release's).

    The corpus, contract, scorer and validator probe come from this tree; the
    detection under test is the binary's. The output is a scorecard-shaped
    object with `layers` and no layer C `runs`, which each release's own
    committed scorecard already provides.
    """
    import platform

    import run_no_opf_benchmark as runner
    import scorecard_record as records

    repo_root = Path(__file__).resolve().parents[2]
    probe = score.validator_probe_binary(repo_root)
    if not args.binary.is_file() or not probe.is_file():
        raise LayerError(f"missing binary {args.binary} or validator probe {probe}")
    policy = args.policy.resolve() if args.policy else None
    vocabulary_root = (args.vocabulary_root or repo_root).resolve()
    # A past release's binary emits the recognizer IDs of its own rulepacks.
    score.COMMITTED_SOURCE_ID_VOCABULARY = score.load_committed_source_id_vocabulary(
        vocabulary_root
    )
    prepared = prepare(repo_root)
    writer = records.RecordWriter(
        [], {"documents": {}},
        corpus_sha256=prepared.manifest["corpus_sha256"],
        extra_documents=[record.to_document() for record in generate(PUBLISHED_PARTITION)],
        layer_contract=prepared.contract,
    )
    layers = runner.measure_agentic_layers(
        prepared=prepared,
        repo_root=repo_root,
        binary=args.binary.resolve(),
        validator_probe=probe,
        davlan_model=args.model_dir.expanduser().resolve(),
        threshold=args.threshold,
        diagnostics_dir=args.output.parent / "logs",
        warmup_count=0,
        measured_repetitions=1,
        policy_path=policy,
        configs=args.config,
        replacing_actions=frozenset(args.manifest_actions.split(",")),
        split_composite_source_ids=args.split_composite_source_ids,
        record_writer=writer,
    )
    result = {
        "schema_version": score.SCORECARD_SCHEMA_VERSION,
        "measured": args.label,
        "gaze": score.git_metadata(repo_root),
        "binary_sha256": score.sha256_file(args.binary),
        "binary_commit": score.git_metadata(vocabulary_root),
        "parameters": {
            "configs": list(args.config),
            "policy_sha256": score.sha256_file(policy) if policy else None,
            "ner_threshold": args.threshold,
            "manifest_replacing_actions": sorted(args.manifest_actions.split(",")),
            "split_composite_source_ids": args.split_composite_source_ids,
        },
        "hardware": platform.platform(),
        "runs": [],
        "layers": layers,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    result["observation_record"] = writer.write(
        args.output.with_name("observations-v1.jsonl.gz"), result,
        add_reference=True,
    )
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")


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
    measure_cmd = commands.add_parser(
        "measure",
        help="score layers A, D and R with any bench binary, e.g. a past release's",
    )
    measure_cmd.add_argument("--binary", type=Path, required=True)
    measure_cmd.add_argument("--config", action="append", required=True)
    measure_cmd.add_argument("--policy", type=Path)
    measure_cmd.add_argument("--model-dir", type=Path, default=Path("~/.local/share/gaze/models/davlan-mbert-ner-hrl"))
    measure_cmd.add_argument("--threshold", type=float, default=0.3)
    measure_cmd.add_argument("--label", required=True, help="e.g. v0.15.1; recorded in the output")
    measure_cmd.add_argument(
        "--manifest-actions",
        choices=("tokenize,redact", "tokenize"),
        default="tokenize,redact",
        help="trace actions that are manifest entries; 'tokenize' for a release before #623",
    )
    measure_cmd.add_argument(
        "--split-composite-source-ids",
        action="store_true",
        help="check each part of an `a+b` source ID, as v0.14.0 emits them",
    )
    measure_cmd.add_argument(
        "--vocabulary-root",
        type=Path,
        help=(
            "checkout of the binary's own commit; its committed rulepacks and "
            "model IDs validate the binary's source IDs (default: this tree)"
        ),
    )
    measure_cmd.add_argument("--output", type=Path, required=True)
    totals_cmd = commands.add_parser("totals", help="print the gate's per-layer totals of one scorecard")
    totals_cmd.add_argument("scorecard", type=Path)
    totals_cmd.add_argument("--config")
    gate_cmd = commands.add_parser("gate", help="apply the rule gate to a base/candidate pair")
    gate_cmd.add_argument("--base", type=Path, required=True)
    gate_cmd.add_argument("--candidate", type=Path, required=True)
    gate_cmd.add_argument("--config")
    gate_cmd.add_argument("--policy-delta", type=Path, help="TOML sections added to the base policy")
    gate_cmd.add_argument("--allow-legacy-policy-inputs", action="store_true",
                          help="compare two historical scorecards without policy-dependency identity")
    args = parser.parse_args(argv)
    try:
        if args.command == "generate":
            records = generate(args.partition)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_bytes(corpus_bytes(records))
            print(json.dumps(manifest(args.partition, records), indent=2))
        elif args.command == "manifest":
            print(json.dumps({p: manifest(p, generate(p)) for p in PARTITIONS}, indent=2))
        elif args.command == "measure":
            _measure(args)
        elif args.command == "totals":
            scorecard = _load_json(args.scorecard)
            config = args.config or production_config(scorecard)
            print(json.dumps(layer_totals(scorecard, config), indent=2, sort_keys=True))
        elif args.command == "grid":
            print(coverage_grid(_load_json(args.scorecard), args.config), end="")
        else:
            result = gate(_load_json(args.base), _load_json(args.candidate), args.config,
                          args.policy_delta, args.allow_legacy_policy_inputs)
            print(gate_markdown(result), end="")
            return {"pass": 0, "fail": 1}.get(str(result["verdict"]), 2)
    except LayerError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
