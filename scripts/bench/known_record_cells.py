#!/usr/bin/env python3
"""Known-record oracle cells for the three match kinds the v4 oracle never hit.

The known-record oracle arm derives each record from a document's own gold, so
the record value always equals the text byte for byte. That arm therefore never
measures `whitespace_flexible`, `whitespace_case_folded` or
`corroborated_single`. These cells supply a record whose value differs from the
text only in the way one kind is for, next to a benign counterweight twin that
reuses the same record and the same shape where the token is not the record
person's PII.

Every pair shares one `PairDescriptor`, derived from the documents and not
declared, so a swapped twin fails generation. A small Python model of the
record matcher checks each cell before it can reach a scorecard: every positive
target is matched by exactly one record slot, and no two slots overlap in any
document. The Rust matcher is the source of truth; the oracle run confirms the
model. No rule is tuned on these cells, so there is one partition. Every value
is synthetic; phones use the reserved `+49 1555` range (CONTRIBUTING.md).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import unicodedata
from collections import Counter
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Callable, Mapping, Sequence
from unittest.mock import patch

import agentic_layers as agentic
import gaze_bench_score as score

GENERATOR_VERSION = 2
SEED = 2026092901
LAYER = "K"
SOURCE_POSITIVE = "known-record-kind-cells"
# The oracle arm marks this source as decoy in its attribution rows.
SOURCE_COUNTERWEIGHT = "known-record-oracle-counterweight"
CONTRACT_V2_PATH = Path("docs/reference/benchmarks/scored-labels-v2.json")

NBSP = "\u00a0"
NARROW_NBSP = "\u202f"
# Mirrors the record matcher's bound on one whitespace run.
MAX_WHITESPACE_RUN = 32
# Subset of crates/gaze-recognizers/assets/record-common-names-v1.txt the cells
# use. Unlisted names match every occurrence; listed ones need corroboration.
LISTED_COMMON_NAMES = ("Mark", "Will", "Rose", "Grace", "Hope")
UNLISTED_WORD_NAMES = ("Summer", "Dawn", "Ivy", "Autumn")
GREETINGS = ("Hi", "Hello", "Dear", "Hallo", "Bonjour", "Olá")
# Record JSON keys the context parser infers; other classes need a field_map.
INFERRED_KEYS = {"Name": "name", "Location": "address", "custom:phone": "phone", "custom:iban": "iban"}


class MatchKind(str, Enum):
    WHITESPACE_FLEXIBLE = "whitespace_flexible"
    WHITESPACE_CASE_FOLDED = "whitespace_case_folded"
    CORROBORATED_SINGLE = "corroborated_single"


class ControlKind(str, Enum):
    """Pairs kept for context that exercise no probe kind; never summed into one."""

    # Compact record, grouped text: the matcher has no digit-run joining.
    UNMATCHED_COMPACT_RECORD = "control_unmatched_compact_record"
    # Irregular record spacing is canonicalized, so the text match is exact.
    EXACT_CANONICAL_RECORD = "control_exact_canonical_record"
    # An unlisted word-name matches every occurrence; its twin prices that.
    EXACT_UNLISTED_NAME = "control_exact_unlisted_name"
    # Gaze folds NBSP-class separators to a space before detection, so these
    # are exact or case-folded matches, not whitespace-flexible ones.
    EXACT_AFTER_SPACE_FOLDING = "control_exact_after_space_folding"
    CASE_FOLDED_AFTER_SPACE_FOLDING = "control_case_folded_after_space_folding"


Bucket = MatchKind | ControlKind
# The matcher kind a control's positive target lands in.
CONTROL_MEASURED_KIND = {
    ControlKind.UNMATCHED_COMPACT_RECORD: "unmatched_term",
    ControlKind.EXACT_CANONICAL_RECORD: "exact",
    ControlKind.EXACT_UNLISTED_NAME: "exact",
    ControlKind.EXACT_AFTER_SPACE_FOLDING: "exact",
    ControlKind.CASE_FOLDED_AFTER_SPACE_FOLDING: "case_folded",
}


class Role(str, Enum):
    POSITIVE = "positive"
    COUNTERWEIGHT = "counterweight"


SURFACES = ("prose", "email", "log_kv", "tool_json")
# JSON strings and one-line log values cannot carry a raw line break.
SINGLE_LINE_SURFACES = frozenset({"log_kv", "tool_json"})


class CellError(ValueError):
    pass


@dataclass(frozen=True)
class RecordField:
    class_name: str
    raw: str


@dataclass(frozen=True)
class Target:
    """A span the pair is about: gold for a positive, the lure for a twin."""

    start: int
    end: int
    slot: int
    # (record group, kind) the record matcher assigns, compared against the
    # canonical record value as the Rust matcher does.
    attribution: tuple[str, str]
    expect_match: bool


@dataclass(frozen=True)
class PairDescriptor:
    bucket: Bucket
    variant: str
    surface: str
    language: str
    record_sha256: str
    primary_shapes: tuple[str, ...]
    primary_attribution: tuple[tuple[str, str], ...]


@dataclass(frozen=True)
class Cell:
    uid: str
    role: Role
    bucket: Bucket
    variant: str
    surface: str
    language: str
    text: str
    gold: tuple[agentic.Gold, ...]
    record: tuple[RecordField, ...]
    targets: tuple[Target, ...]

    @property
    def cell(self) -> str:
        return f"{LAYER}|{self.bucket.value}/{self.variant}|{self.surface}|{self.role.value}"

    @property
    def probe_kind(self) -> MatchKind | None:
        return self.bucket if isinstance(self.bucket, MatchKind) else None

    @property
    def control_kind(self) -> ControlKind | None:
        return self.bucket if isinstance(self.bucket, ControlKind) else None

    def measured_kind(self) -> str:
        """The kind the matcher gives the primary target, derived from text and record."""
        (target,) = self.primary_targets()
        text = self.text.encode("utf-8")[target.start : target.end].decode("utf-8")
        field = self.record[target.slot]
        return matcher_kind(text, field.class_name, field.raw)

    def primary_targets(self) -> tuple[Target, ...]:
        """Slot 0 carries the kind; other slots only corroborate it."""
        return tuple(t for t in self.targets if t.slot == 0)

    def descriptor(self) -> PairDescriptor:
        encoded = self.text.encode("utf-8")
        return PairDescriptor(
            bucket=self.bucket,
            variant=self.variant,
            surface=surface_of(self.text),
            language=self.language,
            record_sha256=hashlib.sha256(record_context(self.record).encode("utf-8")).hexdigest(),
            primary_shapes=tuple(shape(encoded[t.start : t.end].decode("utf-8")) for t in self.primary_targets()),
            primary_attribution=tuple(t.attribution for t in self.primary_targets()),
        )

    def context_json(self) -> str:
        return record_context(self.record)

    def to_document(self) -> score.Document:
        positive = self.role is Role.POSITIVE
        return score.Document(
            uid=self.uid,
            text=self.text,
            language=self.language,
            region="",
            source_dataset=SOURCE_POSITIVE if positive else SOURCE_COUNTERWEIGHT,
            spans=tuple(score.Span(g.start, g.end, g.label) for g in self.gold),
            negative_category=None if positive else f"record_{self.bucket.value}",
            cell=self.cell,
        )

    def to_json(self) -> dict[str, object]:
        return {
            "id": self.uid,
            "role": self.role.value,
            "bucket": self.bucket.value,
            "variant": self.variant,
            "surface": self.surface,
            "language": self.language,
            "text": self.text,
            "gold": [{"start": g.start, "end": g.end, "label": g.label, "value": g.value} for g in self.gold],
            "record": [{"class": f.class_name, "value": f.raw} for f in self.record],
            "targets": [
                {"start": t.start, "end": t.end, "slot": t.slot, "attribution": list(t.attribution), "expect_match": t.expect_match}
                for t in self.targets
            ],
        }


@dataclass(frozen=True)
class Pair:
    positive: Cell
    counterweight: Cell


def record_context(record: Sequence[RecordField]) -> str:
    """The oracle arm's context JSON: one slot per value, inferred keys first."""
    fields: dict[str, dict[str, str]] = {}
    mapping: dict[str, str] = {}
    for index, field in enumerate(record):
        slot = f"v{index:02d}"
        key = INFERRED_KEYS.get(field.class_name, "value")
        fields[slot] = {key: field.raw}
        if field.class_name not in INFERRED_KEYS:
            mapping[f"/{slot}/{key}"] = field.class_name
    context: dict[str, object] = {"record": fields}
    if mapping:
        context["field_map"] = mapping
    return json.dumps(context, ensure_ascii=False)


def shape(value: str) -> str:
    """Case and whitespace pattern with run lengths dropped: `Anna\\u00a0Weber` -> `Aa~Aa`."""
    out: list[str] = []
    for char in value:
        if char == NBSP:
            symbol = "~"
        elif char == NARROW_NBSP:
            symbol = "^"
        elif char == "\n":
            symbol = "/"
        elif char.isspace():
            symbol = "_"
        elif char.isdigit():
            symbol = "9"
        elif char.isalpha():
            symbol = "A" if char.isupper() else "a"
        else:
            symbol = char
        if not out or out[-1] != symbol or symbol in "~^/_":
            out.append(symbol)
    return "".join(out)


def surface_of(text: str) -> str:
    if text.startswith("{"):
        return "tool_json"
    if text.startswith("Subject: "):
        return "email"
    if text.startswith("2026-") and " INFO " in text:
        return "log_kv"
    return "prose"


# --------------------------------------------------------------------------
# Text assembly with byte-exact spans.


class Builder:
    def __init__(self) -> None:
        self._parts: list[str] = []
        self._length = 0
        self.gold: list[agentic.Gold] = []

    def add(self, text: str) -> tuple[int, int]:
        start = self._length
        self._parts.append(text)
        self._length += len(text.encode("utf-8"))
        return start, self._length

    def gold_span(self, text: str, label: str) -> tuple[int, int]:
        start, end = self.add(text)
        self.gold.append(agentic.Gold(start, end, label, text))
        return start, end

    def text(self) -> str:
        return "".join(self._parts)


def wrap(surface: str, ticket: int, fill: Callable[[Builder], list[Target]]) -> tuple[str, tuple[agentic.Gold, ...], tuple[Target, ...]]:
    builder = Builder()
    if surface == "email":
        builder.add(f"Subject: Ticket {ticket}\n\n")
    elif surface == "log_kv":
        builder.add(f'2026-05-04T09:12:44Z INFO crm.note ticket={ticket} note="')
    elif surface == "tool_json":
        builder.add(f'{{"tool": "crm.add_note", "ticket": {ticket}, "note": "')
    before = len(builder.text())
    targets = fill(builder)
    body = builder.text()[before:]
    if surface in SINGLE_LINE_SURFACES and ("\n" in body or '"' in body or "\\" in body):
        raise CellError(f"{surface} body needs escaping")
    if surface == "email":
        builder.add("\n\nThanks,\nSupport desk")
    elif surface == "log_kv":
        builder.add('" status=ok')
    elif surface == "tool_json":
        builder.add('"}')
    return builder.text(), tuple(builder.gold), tuple(targets)


# --------------------------------------------------------------------------
# Synthetic values.

GIVEN = ("Anna", "Jonas", "Clara", "Felix", "Nora", "Elias", "Ida", "Oskar")
SURNAMES = ("Lindqvist", "Okafor", "Brandt", "Castellano", "Nakamura", "Albrecht", "Moreau", "Haldane")
UNICODE_NAMES = (("Zoë", "Ångström"), ("Jürgen", "Østergaard"), ("Chloé", "Mäkelä"), ("Björn", "Çelik"))
STREETS = ("Lindenallee", "Ahornweg", "Birkenstraße", "Ulmenring")


def _digit_bump(value: str, rng: agentic.Rng) -> str:
    positions = [i for i, ch in enumerate(value) if ch.isdigit()][-6:]
    index = positions[rng.below(len(positions))]
    return value[:index] + str((int(value[index]) + 1) % 10) + value[index + 1 :]


def _groups(value: str, size: int = 4) -> list[str]:
    return [value[i : i + size] for i in range(0, len(value), size)]


def _identifier(variant: str, rng: agentic.Rng) -> tuple[str, list[str], str]:
    """(record class, display groups, gold label)."""
    if variant.startswith("iban"):
        # Record values need at least three letters when they have any, so a
        # DE IBAN (two letters) is refused as a record; NL and GB carry a bank code.
        make = agentic._iban_nl if rng.below(2) == 0 else agentic._iban_gb
        return "custom:iban", _groups(make(rng)), "IBAN"
    if variant.startswith("phone"):
        return "custom:phone", ["+49", "1555", "01" + rng.digits(5)], "PHONENUMBER"
    # National identifiers in their official printed grouping.
    if variant.startswith("steuer_id"):
        value = agentic._steuer_id(rng)
        return "custom:steuer_id", [value[:2], value[2:5], value[5:8], value[8:]], "TAXNUM"
    if variant.startswith("bsn"):
        value = agentic._bsn(rng)
        return "custom:national_id", [value[:4], value[4:6], value[6:]], "NATIONALID"
    if variant.startswith("nhs"):
        value = agentic._nhs(rng)
        return "custom:national_id", [value[:3], value[3:6], value[6:]], "NATIONALID"
    return "custom:credit_card", _groups(agentic._card(rng)), "CREDITCARDNUMBER"


IDENTIFIER_SENTENCES = {
    "en": ("Please use ", " for the refund.", "The unrelated catalog reference is ", "."),
    "de": ("Bitte nutze ", " für die Erstattung.", "Die fremde Katalognummer lautet ", "."),
}
# No class cue: the baseline leaks these grouped national identifiers, so only
# a whitespace-flexible record match can protect them.
UNCUED_SENTENCES = {
    "en": ("Please update the file, ", " is correct now.", "The unrelated catalog reference is ", "."),
    "de": ("Bitte in der Akte ändern, ", " stimmt jetzt.", "Die fremde Katalognummer lautet ", "."),
}
IDENTIFIER_SEPARATORS = {
    "iban_double_space": "  ",
    "phone_double_space": "  ",
    "card_double_space": "  ",
    "steuer_id_double_space": "  ",
    "bsn_double_space": "  ",
    "nhs_triple_space": "   ",
    "steuer_id_nbsp": NBSP,
    "card_narrow_nbsp": NARROW_NBSP,
}
UNCUED_VARIANTS = frozenset({"steuer_id_double_space", "bsn_double_space", "nhs_triple_space", "steuer_id_nbsp"})
NAME_SENTENCES = {
    "en": ("The form was signed by ", " yesterday.", "Tickets for the ", " concert hall are sold out.", "field"),
    "de": ("Das Formular hat ", " gestern unterschrieben.", "Karten für die ", " Konzerthalle sind ausverkauft.", "haus"),
}
STREET_SENTENCES = {
    "en": ("Send the parcel to ", " please.", "The public library at ", " opens at nine."),
    "de": ("Schick das Paket bitte an ", ".", "Die Stadtbibliothek in der ", " öffnet um neun."),
}
# Word uses of each listed name, one per corroborated variant; none is PII.
WORD_USES = {
    "Mark": ("Mark the date in the team calendar.", "The Mark II printer is back online.", "Quick note: Mark every parcel as fragile.", "Mark down the invoice total."),
    "Will": ("Will the courier arrive before noon?", "Free Will is the book club pick.", "Quick note: Will the invoice go out today?", "Will this parcel ship today?"),
    "Rose": ("Rose petals decorate the reception desk.", "The Rose Garden café opens at nine.", "Quick note: Rose bushes need water.", "Rose tea is back on the menu."),
    "Grace": ("Grace period ends on Friday.", "The Grace Notes playlist is shared.", "Quick note: Grace period applies to invoices.", "Grace notes add colour to the tune."),
    "Hope": ("Hope the upload works this time.", "The Hope Valley line is delayed.", "Quick note: Hope the fix lands today.", "Hope springs eternal, says the poster."),
}
UNLISTED_WORD_USES = {
    "Summer": "Summer schedule starts on Monday.",
    "Dawn": "Dawn deliveries arrive before six.",
    "Ivy": "Ivy covers the north wall of the depot.",
    "Autumn": "Autumn catalogue ships next week.",
}
CORROBORATED_VARIANTS = (
    "listed_adjacent_peer",
    "listed_surname_comma",
    "listed_greeting_full_name",
    "listed_greeting_before_full_name",
)


# --------------------------------------------------------------------------
# Variant builders. Each returns (positive fill, counterweight fill, record).

Fill = Callable[[Builder], list[Target]]


def _target(span: tuple[int, int], slot: int, attribution: tuple[str, str], expect_match: bool = True) -> Target:
    return Target(span[0], span[1], slot, attribution, expect_match)


def _identifier_variant(variant: str, language: str, rng: agentic.Rng) -> tuple[Fill, Fill, tuple[RecordField, ...]]:
    class_name, groups, label = _identifier(variant, rng)
    spaced = " ".join(groups)
    group = class_name
    if variant.endswith("compact_record"):
        record_raw, text_sep, expect = "".join(groups), " ", False
    else:
        record_raw = spaced
        text_sep = IDENTIFIER_SEPARATORS[variant]
        expect = True
    rendered = text_sep.join(groups)
    lure = text_sep.join(_digit_bump(spaced, rng).split(" "))
    sentences = UNCUED_SENTENCES if variant in UNCUED_VARIANTS else IDENTIFIER_SENTENCES
    lead, tail, lure_lead, lure_tail = sentences[language]
    kind = matcher_kind(rendered, class_name, record_raw)

    def positive(b: Builder) -> list[Target]:
        b.add(lead)
        span = b.gold_span(rendered, label)
        b.add(tail)
        return [_target(span, 0, (group, kind), expect)]

    def counterweight(b: Builder) -> list[Target]:
        b.add(lure_lead)
        span = b.add(lure)
        b.add(lure_tail)
        return [_target(span, 0, (group, kind), False)]

    return positive, counterweight, (RecordField(class_name, record_raw),)


def _name_variant(variant: str, language: str, rng: agentic.Rng) -> tuple[Fill, Fill, tuple[RecordField, ...]]:
    if variant.startswith("name_unicode"):
        given, surname = UNICODE_NAMES[rng.below(len(UNICODE_NAMES))]
    else:
        given, surname = GIVEN[rng.below(len(GIVEN))], SURNAMES[rng.below(len(SURNAMES))]
    separator = {
        "name_line_break": "\n",
        "name_record_irregular": " ",
        "name_upper_double_space": "  ",
        "name_lower_double_space": "  ",
        "name_upper_line_break": "\n",
        "name_unicode_upper_double_space": "  ",
        "name_lower_nbsp": NBSP,
    }[variant]
    record_raw = f"{given}  {surname}" if variant == "name_record_irregular" else f"{given} {surname}"
    case: Callable[[str], str] = str.upper if "upper" in variant else str.lower if "lower" in variant else str
    kind = matcher_kind(case(given) + separator + case(surname), "Name", record_raw)
    lead, tail, lure_lead, lure_tail, suffix = NAME_SENTENCES[language]

    def positive(b: Builder) -> list[Target]:
        b.add(lead)
        start, _ = b.gold_span(case(given), "FIRSTNAME")
        b.add(separator)
        _, end = b.gold_span(case(surname), "SURNAME")
        b.add(tail)
        return [_target((start, end), 0, ("name_multi", kind))]

    def counterweight(b: Builder) -> list[Target]:
        b.add(lure_lead)
        span = b.add(case(given) + separator + case(surname + suffix))
        b.add(lure_tail)
        return [_target(span, 0, ("name_multi", kind), False)]

    return positive, counterweight, (RecordField("Name", record_raw),)


def _street_variant(language: str, rng: agentic.Rng) -> tuple[Fill, Fill, tuple[RecordField, ...]]:
    street = STREETS[rng.below(len(STREETS))]
    number = str(rng.between(12, 89))
    lure_number = number[::-1] if number[0] != number[1] else str(int(number) + 1)
    lead, tail, lure_lead, lure_tail = STREET_SENTENCES[language]
    bucket = ("address_part", matcher_kind(f"{street}  {number}", "Location", f"{street} {number}"))

    def positive(b: Builder) -> list[Target]:
        b.add(lead)
        start, _ = b.gold_span(street, "STREET")
        b.add("  ")
        _, end = b.gold_span(number, "BUILDINGNUM")
        b.add(tail)
        return [_target((start, end), 0, bucket)]

    def counterweight(b: Builder) -> list[Target]:
        b.add(lure_lead)
        span = b.add(street + "  " + lure_number)
        b.add(lure_tail)
        return [_target(span, 0, bucket, False)]

    return positive, counterweight, (RecordField("Location", f"{street} {number}"),)


def _corroborated_variant(variant: str, index: int, rng: agentic.Rng) -> tuple[Fill, Fill, tuple[RecordField, ...]]:
    surname = SURNAMES[rng.below(len(SURNAMES))]
    if variant == "unlisted_alone":
        given = UNLISTED_WORD_NAMES[index % len(UNLISTED_WORD_NAMES)]
        record = (RecordField("Name", given), RecordField("Name", surname))

        def positive(b: Builder) -> list[Target]:
            span = b.gold_span(given, "FIRSTNAME")
            b.add(" called back about the invoice.")
            return [_target(span, 0, ("name_single", "exact"))]

        def counterweight(b: Builder) -> list[Target]:
            sentence = UNLISTED_WORD_USES[given]
            span = b.add(given)
            b.add(sentence[len(given) :])
            # Unlisted names match every occurrence: this lure prices that.
            return [_target(span, 0, ("name_single", "exact"), True)]

        return positive, counterweight, record

    given = LISTED_COMMON_NAMES[index % len(LISTED_COMMON_NAMES)]
    listed = ("name_single", MatchKind.CORROBORATED_SINGLE.value)
    word_use = WORD_USES[given][CORROBORATED_VARIANTS.index(variant)]
    if variant in {"listed_greeting_full_name", "listed_greeting_before_full_name"}:
        record = (RecordField("Name", given), RecordField("Name", f"{given} {surname}"))
        # Order matters to NER: a greeting before the full name is the shape it misses.
        greeting_first = variant == "listed_greeting_before_full_name"

        def positive(b: Builder) -> list[Target]:
            if greeting_first:
                b.add("Hi ")
                greeting = b.gold_span(given, "FIRSTNAME")
                b.add(", the refund is approved. ")
            start, _ = b.gold_span(given, "FIRSTNAME")
            b.add(" ")
            _, end = b.gold_span(surname, "SURNAME")
            b.add(" opened the ticket.")
            if not greeting_first:
                b.add(" Hi ")
                greeting = b.gold_span(given, "FIRSTNAME")
                b.add(", the refund is approved.")
            return [_target(greeting, 0, listed), _target((start, end), 1, ("name_multi", "exact"))]
    else:
        record = (RecordField("Name", given), RecordField("Name", surname))

        def positive(b: Builder) -> list[Target]:
            if variant == "listed_adjacent_peer":
                b.add("Yesterday ")
                first = b.gold_span(given, "FIRSTNAME")
                b.add(" ")
                second = b.gold_span(surname, "SURNAME")
                b.add(" called about the delivery.")
            else:
                b.add("Signed: ")
                second = b.gold_span(surname, "SURNAME")
                b.add(", ")
                first = b.gold_span(given, "FIRSTNAME")
                b.add(".")
            return [_target(first, 0, listed), _target(second, 1, ("name_single", "exact"))]

    def counterweight(b: Builder) -> list[Target]:
        offset = word_use.index(given)
        b.add(word_use[:offset])
        span = b.add(given)
        b.add(word_use[offset + len(given) :])
        return [_target(span, 0, listed, False)]

    return positive, counterweight, record


VARIANTS: dict[Bucket, tuple[str, ...]] = {
    MatchKind.WHITESPACE_FLEXIBLE: (
        "iban_double_space",
        "phone_double_space",
        "card_double_space",
        "steuer_id_double_space",
        "bsn_double_space",
        "nhs_triple_space",
        "name_line_break",
        "street_double_space",
    ),
    MatchKind.WHITESPACE_CASE_FOLDED: (
        "name_upper_double_space",
        "name_lower_double_space",
        "name_upper_line_break",
        "name_unicode_upper_double_space",
    ),
    MatchKind.CORROBORATED_SINGLE: CORROBORATED_VARIANTS,
    ControlKind.UNMATCHED_COMPACT_RECORD: ("iban_compact_record",),
    ControlKind.EXACT_CANONICAL_RECORD: ("name_record_irregular",),
    ControlKind.EXACT_UNLISTED_NAME: ("unlisted_alone",),
    ControlKind.EXACT_AFTER_SPACE_FOLDING: ("steuer_id_nbsp", "card_narrow_nbsp"),
    ControlKind.CASE_FOLDED_AFTER_SPACE_FOLDING: ("name_lower_nbsp",),
}


def surfaces_for(variant: str) -> tuple[str, ...]:
    if "line_break" in variant:
        return tuple(s for s in SURFACES if s not in SINGLE_LINE_SURFACES)
    return SURFACES


def _english_only(variant: str) -> bool:
    return variant in CORROBORATED_VARIANTS or variant == "unlisted_alone"


def _variant_fills(variant: str, language: str, index: int, rng: agentic.Rng) -> tuple[Fill, Fill, tuple[RecordField, ...]]:
    if _english_only(variant):
        return _corroborated_variant(variant, index, rng)
    if variant.startswith(("iban", "phone", "card", "steuer_id", "bsn", "nhs")):
        return _identifier_variant(variant, language, rng)
    if variant == "street_double_space":
        return _street_variant(language, rng)
    return _name_variant(variant, language, rng)


def generate() -> list[Pair]:
    pairs: list[Pair] = []
    ticket = 4100
    for bucket, variants in VARIANTS.items():
        for variant in variants:
            rng = agentic.Rng(SEED, f"{LAYER}/{bucket.value}/{variant}")
            for index, surface in enumerate(surfaces_for(variant)):
                language = "en" if _english_only(variant) or index % 2 == 0 else "de"
                positive_fill, counterweight_fill, record = _variant_fills(variant, language, index, rng)
                cells = []
                for role, fill in ((Role.POSITIVE, positive_fill), (Role.COUNTERWEIGHT, counterweight_fill)):
                    ticket += 1
                    text, gold, targets = wrap(surface, ticket, fill)
                    uid = f"known-record-kind-{bucket.value}-{variant}-{surface}-{role.value}"
                    cells.append(Cell(uid, role, bucket, variant, surface, language, text, gold, record, targets))
                pairs.append(Pair(*cells))
    check(pairs)
    return pairs


# --------------------------------------------------------------------------
# Fail-closed checks, including a model of the record matcher.


def gaze_fold(text: str) -> str:
    """Gaze's pre-detection folding (crates/gaze/src/normalize.rs).

    Every Unicode space separator becomes an ASCII space and fullwidth ASCII
    becomes ASCII, then NFC. Line breaks and tabs are not space separators.
    """
    out = []
    for char in text:
        if char in "\u200c\u200d":
            continue
        if unicodedata.category(char) == "Zs":
            char = " "
        elif "\uff01" <= char <= "\uff5e":
            char = chr(ord(char) - 0xFEE0)
        out.append(unicodedata.normalize("NFC", char))
    return "".join(out)


def matcher_kind(text: str, record_class: str, raw: str) -> str:
    """The Rust matcher's kind for a hit on Gaze-folded text.

    Mirrors `record_match_kind` plus the listed-name corroboration gate:
    compared with the canonical record value, after `gaze_fold`. #718's
    round-3 recorder compares the unfolded text with the raw value instead.
    """
    text = gaze_fold(text)
    term = " ".join(raw.split())
    collapsed = " ".join(text.split())
    fold = record_class == "Name"
    if collapsed != term and not (fold and collapsed.casefold() == term.casefold()):
        return "unmatched_term"
    listed = {name.casefold() for name in LISTED_COMMON_NAMES}
    if fold and len(term.split()) == 1 and term.casefold() in listed:
        return MatchKind.CORROBORATED_SINGLE.value
    if text == term:
        return "exact"
    if collapsed == term:
        return MatchKind.WHITESPACE_FLEXIBLE.value
    if text.casefold() == term.casefold():
        return "case_folded"
    return MatchKind.WHITESPACE_CASE_FOLDED.value


def _identifier_char(char: str) -> bool:
    return char == "_" or char.isalnum()


def _boundary(text: str, start: int, end: int) -> bool:
    def component(chars: str) -> bool:
        if not chars:
            return False
        if chars[0] == "-":
            return len(chars) > 1 and _identifier_char(chars[1])
        return _identifier_char(chars[0])

    return not component(text[:start][::-1]) and not component(text[end:])


def _normalize(text: str, fold: bool) -> tuple[str, list[int], list[int]]:
    """Collapse whitespace runs, optionally case fold; char offsets back to `text`."""
    out: list[str] = []
    starts: list[int] = []
    ends: list[int] = []
    run = 0
    for index, char in enumerate(text):
        if char.isspace():
            run += 1
            if run == 1 or run == MAX_WHITESPACE_RUN + 1:
                out.append(" ")
                starts.append(index)
                ends.append(index + 1)
            else:
                ends[-1] = index + 1
            continue
        run = 0
        mapped = char.casefold() if fold else char
        for position, piece in enumerate(mapped):
            out.append(piece)
            starts.append(index if position == 0 else -1)
            ends.append(index + 1 if position == len(mapped) - 1 else -1)
    return "".join(out), starts, ends


def _occurrences(text: str, term: str, fold: bool) -> list[tuple[int, int]]:
    normalized, starts, ends = _normalize(text, fold)
    needle = " ".join(term.split())
    needle = needle.casefold() if fold else needle
    found = []
    at = normalized.find(needle)
    while at != -1:
        start, end = starts[at], ends[at + len(needle) - 1]
        if start >= 0 and end >= 0 and _boundary(text, start, end):
            found.append((start, end))
        at = normalized.find(needle, at + 1)
    return found


def _name_position(text: str, start: int, end: int) -> bool:
    prefix = text[:start].rstrip()
    word = ""
    for char in reversed(prefix):
        if not char.isalpha():
            break
        word = char + word
    return word in GREETINGS and text[end : end + 1] in {",", "!", ":"}


def _corroborated(text: str, start: int, end: int, slot: int, record: Sequence[RecordField]) -> bool:
    own = record[slot].raw
    if text[start] != own[0]:
        return False
    for other, field in enumerate(record):
        if other == slot or field.class_name != "Name":
            continue
        if len(field.raw.split()) > 1:
            if any(piece.casefold() == own.casefold() for piece in field.raw.split()) and _occurrences(text, field.raw, True):
                if _name_position(text, start, end):
                    return True
            continue
        at = text.find(field.raw)
        while at != -1:
            peer_end = at + len(field.raw)
            if _boundary(text, at, peer_end):
                between = text[peer_end:start] if peer_end <= start else text[end:at] if end <= at else None
                if between is not None and len(between.encode("utf-8")) <= 16 and all(
                    ch.isspace() or ch in ",-’'" for ch in between
                ):
                    return True
            at = text.find(field.raw, at + 1)
    return False


def model_matches(text: str, record: Sequence[RecordField]) -> list[tuple[int, int, int]]:
    """(slot, byte start, byte end) the record matcher is expected to emit."""
    listed = {name.casefold() for name in LISTED_COMMON_NAMES}
    matches = []
    for slot, field in enumerate(record):
        fold = field.class_name == "Name"
        single_listed = fold and len(field.raw.split()) == 1 and field.raw.casefold() in listed
        for start, end in _occurrences(text, field.raw, fold):
            if single_listed and not _corroborated(text, start, end, slot, record):
                continue
            matches.append((slot, len(text[:start].encode("utf-8")), len(text[:end].encode("utf-8"))))
    return sorted(matches)


def check(pairs: Sequence[Pair]) -> None:
    uids = [cell.uid for pair in pairs for cell in (pair.positive, pair.counterweight)]
    if len(set(uids)) != len(uids):
        raise CellError("cell IDs are not unique")
    for pair in pairs:
        left = pair.positive.descriptor()
        right = pair.counterweight.descriptor()
        if left != right:
            raise CellError(f"{pair.positive.uid} and its twin differ in shape or position: {left} != {right}")
        for cell in (pair.positive, pair.counterweight):
            _check_cell(cell)
        _check_bucket(pair.positive)


def _check_bucket(positive: Cell) -> None:
    """A probe must exercise its declared kind; a control lands in its own bucket."""
    measured = positive.measured_kind()
    expected = positive.probe_kind.value if positive.probe_kind else CONTROL_MEASURED_KIND[positive.control_kind]
    if measured != expected:
        raise CellError(f"{positive.uid}: primary target measures {measured}, bucket {positive.bucket.value} expects {expected}")
    (target,) = positive.primary_targets()
    if target.attribution[1] != measured:
        raise CellError(f"{positive.uid}: declared attribution {target.attribution[1]} is not the measured {measured}")


def accepted_record_value(value: str) -> bool:
    """The context parser's floor: three letters if any, else four digits."""
    letters = sum(ch.isalpha() for ch in value)
    digits = sum(ch.isnumeric() for ch in value)
    return digits >= 4 if letters == 0 else letters >= 3


def _check_cell(cell: Cell) -> None:
    encoded = cell.text.encode("utf-8")
    for field in cell.record:
        if not accepted_record_value(" ".join(field.raw.split())):
            raise CellError(f"{cell.uid}: the context parser refuses a record value")
    if cell.role is Role.COUNTERWEIGHT and cell.gold:
        raise CellError(f"{cell.uid}: a counterweight carries gold")
    if cell.role is Role.POSITIVE and not cell.gold:
        raise CellError(f"{cell.uid}: a positive carries no gold")
    for gold in cell.gold:
        if encoded[gold.start : gold.end].decode("utf-8") != gold.value:
            raise CellError(f"{cell.uid}: gold offsets do not select the inserted value")
    raws = [(field.class_name, " ".join(field.raw.split()).casefold()) for field in cell.record]
    if len(set(raws)) != len(raws):
        raise CellError(f"{cell.uid}: duplicate record value would split attribution")
    matches = model_matches(cell.text, cell.record)
    for index, (slot_a, start_a, end_a) in enumerate(matches):
        for slot_b, start_b, end_b in matches[index + 1 :]:
            if slot_a != slot_b and start_a < end_b and start_b < end_a:
                raise CellError(f"{cell.uid}: two record slots overlap, attribution is ambiguous")
    for target in cell.targets:
        hit = any(m == (target.slot, target.start, target.end) for m in matches)
        if hit != target.expect_match:
            raise CellError(f"{cell.uid}: target slot {target.slot} match is {hit}, expected {target.expect_match}")
    if cell.role is Role.COUNTERWEIGHT:
        expected = {(t.slot, t.start, t.end) for t in cell.targets if t.expect_match}
        if set(matches) != expected:
            raise CellError(f"{cell.uid}: counterweight matches outside its declared lure")


# --------------------------------------------------------------------------
# Oracle arm integration.


def cells(pairs: Sequence[Pair]) -> list[Cell]:
    return [cell for pair in pairs for cell in (pair.positive, pair.counterweight)]


def corpus_bytes(pairs: Sequence[Pair]) -> bytes:
    return b"".join(
        json.dumps(cell.to_json(), ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8") + b"\n"
        for cell in cells(pairs)
    )


def manifest(pairs: Sequence[Pair]) -> dict[str, object]:
    by_kind = Counter(pair.positive.bucket.value for pair in pairs)
    return {
        "generator": "scripts/bench/known_record_cells.py",
        "generator_version": GENERATOR_VERSION,
        "seed": SEED,
        "pairs": len(pairs),
        "pairs_by_bucket": dict(sorted(by_kind.items())),
        "corpus_sha256": hashlib.sha256(corpus_bytes(pairs)).hexdigest(),
        "synthetic_only": True,
    }


def documents(repo_root: Path, contract: str, pairs: Sequence[Pair] | None = None) -> tuple[list[score.Document], dict[str, str]]:
    """Scored documents and the oracle record context for each, by uid."""
    pairs = generate() if pairs is None else pairs
    all_cells = cells(pairs)
    docs = [cell.to_document() for cell in all_cells]
    if contract == "v2":
        ruling = score.load_scored_label_contract(repo_root / CONTRACT_V2_PATH)
        docs = score.apply_scored_label_contract(docs, ruling)
    elif contract != "v1":
        raise CellError(f"unknown contract {contract!r}")
    return docs, {cell.uid: cell.context_json() for cell in all_cells}


@dataclass
class VariantTally:
    """Per (bucket, variant, role) byte changes between two arms.

    Probe buckets are match kinds; control buckets never add into one.

    Recovered gold and added false-positive bytes come from the scored view of
    the final protection trace, exactly as the byte score counts them.
    """

    by_uid: Mapping[str, Cell]
    baseline: dict[str, list[tuple[int, int]]]
    totals: Counter[tuple[str, str, str, str]]

    @classmethod
    def create(cls, pairs: Sequence[Pair]) -> "VariantTally":
        return cls({cell.uid: cell for cell in cells(pairs)}, {}, Counter())

    @staticmethod
    def _view(document: score.Document, response: Mapping[str, object]) -> list[tuple[int, int]]:
        if "pipeline_error_code" in response:
            raise CellError(f"{document.uid}: kind cells require successful responses")
        predictions = score.final_trace_predictions(document, dict(response))
        _, ignored, selected = score.contract_scoring_view(document, predictions)
        return score.subtract_intervals(score.merge_intervals((s.start, s.end) for s in selected), ignored)

    def record_baseline(self, document: score.Document, response: Mapping[str, object]) -> None:
        if document.uid in self.by_uid:
            self.baseline[document.uid] = self._view(document, response)

    def record_candidate(self, document: score.Document, response: Mapping[str, object]) -> None:
        cell = self.by_uid.get(document.uid)
        if cell is None:
            return
        before = self.baseline.pop(document.uid)
        after = self._view(document, response)
        gold = score.merge_intervals((s.start, s.end) for s in document.spans)
        new = score.subtract_intervals(after, before)
        removed = score.subtract_intervals(before, after)
        targets = score.merge_intervals((t.start, t.end) for t in cell.targets)
        key = (cell.bucket.value, cell.variant, cell.role.value)
        self.totals[(*key, "gold_recovered_bytes")] += score.intersection_length(new, gold)
        self.totals[(*key, "false_positive_added_bytes")] += score.interval_length(new) - score.intersection_length(new, gold)
        self.totals[(*key, "gold_lost_bytes")] += score.intersection_length(removed, gold)
        self.totals[(*key, "false_positive_removed_bytes")] += score.interval_length(removed) - score.intersection_length(removed, gold)
        self.totals[(*key, "target_bytes")] += score.interval_length(targets)
        self.totals[(*key, "target_protected_bytes")] += score.intersection_length(after, targets)
        self.totals[(*key, "documents")] += 1

    def result(self) -> dict[str, object]:
        if self.baseline:
            raise CellError("kind cells have unpaired baseline documents")
        fields = (
            "documents", "target_bytes", "target_protected_bytes", "gold_recovered_bytes",
            "false_positive_added_bytes", "gold_lost_bytes", "false_positive_removed_bytes",
        )
        rows = [
            {
                "bucket": bucket,
                "type": "probe" if bucket in {kind.value for kind in MatchKind} else "control",
                "variant": variant,
                "role": role,
                **{f: self.totals[(bucket, variant, role, f)] for f in fields},
            }
            for bucket, variant, role in sorted({key[:3] for key in self.totals})
        ]
        return {"schema_version": 1, "rows": rows}


# --------------------------------------------------------------------------
# Score-only proof: three arms over layer K alone.

# #718's default allowed kinds per record group (fa2aa8d9,
# `Context::record_allowed_match_kinds`); the OFF arm states them explicitly.
DEFAULT_MATCH_KINDS: dict[str, tuple[str, ...]] = {
    "name_single": ("exact", "case_folded"),
    "name_multi": (),
    "address_part": ("exact",),
    "custom:credit_card": ("exact",),
    "custom:iban": ("exact",),
    "custom:national_id": ("exact",),
    "custom:passport": ("exact",),
    "custom:phone": ("exact",),
    "custom:steuer_id": ("exact",),
}
# The probe kinds each group may enable (the parser refuses the others).
PROBE_KINDS_BY_GROUP: dict[str, tuple[str, ...]] = {
    "name_single": (MatchKind.CORROBORATED_SINGLE.value,),
    "name_multi": (MatchKind.WHITESPACE_FLEXIBLE.value, MatchKind.WHITESPACE_CASE_FOLDED.value),
}


def record_group(field: RecordField) -> str:
    if field.class_name == "Name":
        return "name_single" if len(field.raw.split()) == 1 else "name_multi"
    return "address_part" if field.class_name == "Location" else field.class_name


def arm_context(cell: Cell, probes_on: bool) -> str:
    """The cell's record with every group's allowed kinds stated explicitly."""
    context = json.loads(cell.context_json())
    kinds: dict[str, list[str]] = {}
    for field in cell.record:
        group = record_group(field)
        allowed = list(DEFAULT_MATCH_KINDS[group])
        if probes_on:
            allowed += PROBE_KINDS_BY_GROUP.get(group, (MatchKind.WHITESPACE_FLEXIBLE.value,))
        kinds[group] = allowed
    context["record_match_kinds"] = kinds
    return json.dumps(context, ensure_ascii=False)


def _run_arm(
    documents: Sequence[score.Document], contexts: Mapping[str, str] | None, *, known_record: bool, **kwargs: object
) -> tuple[dict[str, object], dict[str, dict[str, object]]]:
    """One scored run; the transport adds each document's context, as the oracle arm does."""
    responses: dict[str, dict[str, object]] = {}

    def observe(_config: str, document: score.Document, response: dict, _validators: object) -> None:
        responses[document.uid] = response

    base_transport = score.BenchSubprocess

    class ContextTransport(base_transport):  # type: ignore[misc, valid-type]
        def exchange(self, request: dict[str, object]) -> dict[str, object]:
            if contexts is not None:
                request = {**request, "context_json": contexts[str(request["fixture_id"])]}
            return super().exchange(request)

    environment = {key: value for key, value in os.environ.items() if key != "GAZE_BENCH_KNOWN_RECORD_ARM"}
    if known_record:
        environment["GAZE_BENCH_KNOWN_RECORD_ARM"] = "1"
    with patch.object(score, "BenchSubprocess", ContextTransport):
        result = score.run_config(documents=documents, base_environment=environment, record_document=observe, **kwargs)
    return result, responses


def _compare(pairs: Sequence[Pair], documents: Sequence[score.Document], before: Mapping, after: Mapping) -> dict[str, object]:
    tally = VariantTally.create(pairs)
    for document in documents:
        tally.record_baseline(document, before[document.uid])
    for document in documents:
        tally.record_candidate(document, after[document.uid])
    rows = tally.result()["rows"]
    rollup: dict[tuple[str, str], Counter[str]] = {}
    for row in rows:
        counts = rollup.setdefault((row["bucket"], row["role"]), Counter())
        counts.update({k: v for k, v in row.items() if isinstance(v, int)})
    return {
        "by_variant": rows,
        "by_bucket": [
            {"bucket": bucket, "role": role, **dict(sorted(counts.items()))}
            for (bucket, role), counts in sorted(rollup.items())
        ],
    }


def kind_switch_effective(off: Mapping[str, dict[str, object]], on: Mapping[str, dict[str, object]]) -> bool:
    """Whether turning the probe kinds on changed any protected span.

    A binary that builds its record recognizers once, before any request,
    ignores the per-request `record_match_kinds`; its two arms are identical.
    """
    def spans(response: Mapping[str, object]) -> list[tuple[int, int]]:
        return sorted((int(item["raw_start"]), int(item["raw_end"])) for item in response.get("final_protection_trace", []))

    return any(spans(off[uid]) != spans(on[uid]) for uid in off)


def prove(args: argparse.Namespace) -> dict[str, object]:
    repo = args.repo.resolve()
    pairs = generate()
    scored, _ = documents(repo, args.contract, pairs)
    by_uid = {cell.uid: cell for cell in cells(pairs)}
    kwargs = dict(
        repo_root=repo, binary=args.binary.resolve(), config="policy-file", model_dir=args.model_dir.resolve(),
        opf_command=None, opf_checkpoint=None, opf_daemon_socket=None, threshold=0.3,
        diagnostics_dir=args.output.parent / f"known-record-cells-{args.contract}-logs", policy_path=args.policy.resolve(),
    )
    arms = {
        "no_record": (None, False),
        "record_default_kinds": ({uid: arm_context(cell, False) for uid, cell in by_uid.items()}, True),
        "record_probe_kinds_on": ({uid: arm_context(cell, True) for uid, cell in by_uid.items()}, True),
    }
    results: dict[str, dict[str, object]] = {}
    responses: dict[str, Mapping[str, dict[str, object]]] = {}
    for name, (contexts, known) in arms.items():
        results[name], responses[name] = _run_arm(scored, contexts, known_record=known, **kwargs)
    return {
        "proof": "layer K only, score-only; aggregates without values",
        "contract": args.contract,
        "manifest": manifest(pairs),
        "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        "policy_sha256": hashlib.sha256(args.policy.read_bytes()).hexdigest(),
        "kind_switch_effective": kind_switch_effective(
            responses["record_default_kinds"], responses["record_probe_kinds_on"]
        ),
        "arms": {
            name: {key: result[key] for key in ("metrics", "pipeline_availability")}
            for name, result in results.items()
        },
        # Controls show their effect here; probe kinds are off in both arms.
        "default_kinds_vs_no_record": _compare(pairs, scored, responses["no_record"], responses["record_default_kinds"]),
        # Only the probe kinds differ between these arms.
        "probe_kinds_on_vs_default": _compare(pairs, scored, responses["record_default_kinds"], responses["record_probe_kinds_on"]),
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--jsonl", type=Path, help="write the cells as JSON lines")
    sub = parser.add_subparsers(dest="command")
    proof = sub.add_parser("prove", help="score layer K with no record, default kinds, and probe kinds on")
    proof.add_argument("--repo", type=Path, default=Path.cwd())
    proof.add_argument("--binary", required=True, type=Path, help="a Known-Record build of clean_for_bench")
    proof.add_argument("--policy", required=True, type=Path)
    proof.add_argument("--model-dir", required=True, type=Path)
    proof.add_argument("--contract", required=True, choices=("v1", "v2"))
    proof.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    if args.command == "prove":
        report = prove(args)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        return 0
    pairs = generate()
    if args.jsonl:
        args.jsonl.write_bytes(corpus_bytes(pairs))
    json.dump(manifest(pairs), sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
