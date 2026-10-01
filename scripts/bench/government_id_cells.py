"""Synthetic cue/value ownership cells for tax and government identifiers.

This module defines benchmark semantics, not a production detector. Leading
zeroes invalidate US SSN areas, Swiss AHV prefixes and German Steuer-ID values;
other schemes are deliberately invented zero-led test identifiers. Partial
SSNs expose only independently generated tails, never an assignable full SSN.
"""
from dataclasses import dataclass
from enum import Enum
from types import ModuleType
from typing import TYPE_CHECKING
import re

if TYPE_CHECKING:
    from agentic_layers import Record, Rng


class Shape(str, Enum):
    TAX_NINE = "tax_nine"
    TAX_ELEVEN = "tax_eleven"
    TAX_GROUPED = "tax_grouped"
    TAX_SLASH = "tax_slash"
    TAX_SLASH_LONG = "tax_slash_long"
    TAX_PREFIX = "tax_prefix"
    TAX_SUFFIX = "tax_suffix"
    TAX_DOTTED = "tax_dotted"
    SSN_US = "ssn_us"
    SSN_TRIPLE = "ssn_triple"
    SSN_SWISS_DOT = "ssn_swiss_dot"
    SSN_SWISS_DASH = "ssn_swiss_dash"
    SSN_MIXED = "ssn_mixed"
    SSN_TAIL = "ssn_tail"
    CARD_GROUPED = "card_grouped"
    CARD_PREFIX = "card_prefix"
    CARD_SUFFIX = "card_suffix"
    NATIONAL_COMPACT = "national_compact"
    NATIONAL_GROUPED = "national_grouped"
    NATIONAL_EMBEDDED = "national_embedded"

    @property
    def label(self) -> str:
        return {"tax": "TAXNUM", "ssn": "SSN", "card": "IDCARDNUM", "national": "NATIONALID"}[self.value.split("_")[0]]


PATTERNS = {
    Shape.TAX_NINE: r"0\d{8}",
    Shape.TAX_ELEVEN: r"0\d{10}",
    Shape.TAX_GROUPED: r"0\d \d{3} \d{3} \d{3}",
    Shape.TAX_SLASH: r"0\d/\d{3}/\d{5}",
    Shape.TAX_SLASH_LONG: r"0\d{2}/\d{4}/\d{5}",
    Shape.TAX_PREFIX: r"ZZ0\d{8}",
    Shape.TAX_SUFFIX: r"0\d{6}ZZ",
    Shape.TAX_DOTTED: r"ZZ0\d{2}\.\d{3}\.\d{3}Z",
    Shape.SSN_US: r"000[-. ]\d{2}[-. ]\d{4}",
    Shape.SSN_TRIPLE: r"000[- ]\d{3}[- ]\d{3}",
    Shape.SSN_SWISS_DOT: r"000\.\d{4}\.\d{4}\.\d{2}",
    Shape.SSN_SWISS_DASH: r"000-\d{4}-\d{4}-\d{2}",
    Shape.SSN_MIXED: r"ZZ0\d{5}Z",
    Shape.SSN_TAIL: r"\d{4}",
    Shape.CARD_GROUPED: r"000\d{3}-\d{2}-\d{4}",
    Shape.CARD_PREFIX: r"ZZ0\d{6}",
    Shape.CARD_SUFFIX: r"000\d{4}Z",
    Shape.NATIONAL_COMPACT: r"000\d{6,9}",
    Shape.NATIONAL_GROUPED: r"00 \d{3} \d{3} \d",
    Shape.NATIONAL_EMBEDDED: r"000\d{3}Z\d{4}",
}
INVALID_TAX_SHAPES = frozenset((Shape.TAX_ELEVEN, Shape.TAX_GROUPED))

CUES = {
    "TAXNUM": r"(?:Steuer-ID|Steueridentifikationsnummer|steuerliche Identifikationsnummer|Steuernummer|tax identification number|taxpayer identification number|tax number|tax ID|TIN)",
    "SSN": r"(?:social security number|SSN|social insurance number|Sozialversicherungsnummer|AHV number)",
    "IDCARDNUM": r"(?:identity card number|ID card number|identification card|Personalausweisnummer)",
    "NATIONALID": r"(?:national identification number|national ID number|national identity number|nationale Identifikationsnummer)",
}
# Shape-only mutants drop semantics. Near-cue mutants retain an exact shape but
# allow a second field to own the digits. Both must pay for their own shape.
BROAD = {shape: rf"\b{pattern}\b" for shape, pattern in PATTERNS.items()}
NARROW = {shape: rf"(?i){CUES[shape.label]}[^\d\n]{{0,100}}?\b{pattern}\b" for shape, pattern in PATTERNS.items()}


class Relation(str, Enum):
    DIRECT = "direct"
    COPULA = "copula"
    PARENTHETICAL = "parenthetical"
    RECORDED = "recorded"
    LINE = "line"
    PARTIAL = "partial"


RELATIONS = {
    Relation.DIRECT: (": ", " = "),
    Relation.COPULA: (" lautet ", " is "),
    Relation.PARENTHETICAL: (" (TIN): ", " (identifier) is "),
    Relation.RECORDED: (" is recorded as ", " ist hinterlegt als "),
    Relation.LINE: (":\n  ", " =\r\n "),
    Relation.PARTIAL: (" ends with ", " last four digits are "),
}
# A field transition is deliberately absent from the reference relation grammar.
CONNECTOR = r'(?:\s*[:=]\s*|\s+(?:lautet|is|is recorded as|ist hinterlegt als)\s+|\s+\((?:TIN|SSN|AHV|ID|NID|identifier)\)(?::| is)\s*)'
PARTIAL = r"\s+(?:ends with|last four digits are)\s+"


@dataclass(frozen=True)
class Cell:
    family: str
    shape: Shape
    surface: str
    relation: Relation
    templates: dict[str, str]
    gold: bool
    near_cue: bool = False


def _cells() -> tuple[tuple[Cell, ...], tuple[Cell, ...]]:
    gold, twins = [], []
    for index, shape in enumerate(Shape):
        stem = shape.value.split("_")[0]
        cues = {
            "tax": ("Steuer-ID", "tax identification number"),
            "ssn": ("social security number", "SSN"),
            "card": ("identity card number", "ID card number"),
            "national": ("national identification number", "national ID number"),
        }[stem]
        if stem == "tax":
            cues = {
                Shape.TAX_NINE: ("TIN", "tax identification number"),
                Shape.TAX_ELEVEN: ("Steueridentifikationsnummer", "taxpayer identification number"),
                Shape.TAX_GROUPED: ("Steuer-ID", "steuerliche Identifikationsnummer"),
                Shape.TAX_SLASH: ("Steuernummer", "tax number"),
                Shape.TAX_SLASH_LONG: ("Steuer-ID", "TIN"),
                Shape.TAX_PREFIX: ("tax ID", "Steueridentifikationsnummer"),
                Shape.TAX_SUFFIX: ("tax identification number", "taxpayer identification number"),
                Shape.TAX_DOTTED: ("Steuer-ID", "tax identification number"),
            }[shape]
        if shape in (Shape.SSN_SWISS_DOT, Shape.SSN_SWISS_DASH, Shape.SSN_MIXED):
            cues = ("Sozialversicherungsnummer", "social insurance number")
        if shape == Shape.SSN_SWISS_DOT:
            cues = ("AHV number", "Sozialversicherungsnummer")
        if shape == Shape.CARD_PREFIX:
            cues = ("Personalausweisnummer", "identification card")
        relation = list(Relation)[index % 5]
        if shape == Shape.SSN_TAIL:
            relation = Relation.PARTIAL
        surface = ("prose", "log_kv", "tool_json", "tool_result")[index % 4]
        if shape == Shape.SSN_US:
            surface, relation = "prose", Relation.PARENTHETICAL
        # Line relations stay prose: embedded newlines would break JSON strings.
        if relation == Relation.LINE:
            surface = "prose"
        templates, bare, near = {}, {}, {}
        for partition, cue, connector in zip(("dev", "test"), cues, RELATIONS[relation]):
            if surface == "tool_json":
                templates[partition] = '{"' + cue + '": "{V}", "status": "synthetic"}'
                key = "reference" if partition == "dev" else "batch reference"
                bare[partition] = '{"' + key + '": "{V}", "status": "synthetic"}'
                near[partition] = '{"' + cue + ' checked": "yes", "order reference": "{V}"}'
                relation = Relation.DIRECT
            else:
                prefix = {"prose": "", "log_kv": "event=synthetic ", "tool_result": "Tool result: "}[surface]
                acronym = {"tax": "TIN", "ssn": "SSN", "card": "ID", "national": "NID"}[stem]
                templates[partition] = prefix + cue + connector.replace("(TIN)", f"({acronym})") + "{V}"
                bare[partition] = prefix + ("Invoice reference: " if partition == "dev" else "Technical licence: ") + "{V}"
                near[partition] = prefix + cue + (" checked; order reference: " if partition == "dev" else " verified; vehicle ID: ") + "{V}"
        gold.append(Cell("gov_" + shape.value, shape, "gov_" + surface, relation, templates, True))
        twins.append(Cell("gov_twin_" + shape.value, shape, "gov_" + surface, relation, bare, False))
        twins.append(Cell("gov_near_" + shape.value, shape, "gov_" + surface, relation, near, False, True))
    # Scalars explicitly tied to tax documents must remain amounts, not IDs.
    for kind, unit in (("rate", "%"), ("amount", " EUR")):
        twins.append(Cell("gov_tax_" + kind, Shape.TAX_NINE, "gov_prose", Relation.DIRECT,
                          {"dev": "TIN checked; tax " + kind + ": {V}" + unit,
                           "test": "Steuer-ID verified; invoice " + kind + " = {V}" + unit}, False, True))
    return tuple(gold), tuple(twins)


CELLS, TWINS = _cells()
TAX_GUARD_FAMILIES = tuple(twin.family for twin in TWINS if twin.shape in INVALID_TAX_SHAPES)
DOCS = {"A": 4, "D": 2}
FAMILY_LABELS = {cell.family: cell.shape.label for cell in CELLS}


def value(shape: Shape, rng: "Rng", partition: str, index: int) -> str:
    # Partition marker belongs inside every value, including four-digit tails.
    mark = "1" if partition == "dev" else "8"
    d = lambda n: mark + rng.digits(n - 1)
    values = {
        Shape.TAX_NINE: lambda: "000" + d(6),
        Shape.TAX_ELEVEN: lambda: "000" + d(8),
        Shape.TAX_GROUPED: lambda: "00 " + d(3) + " " + d(3) + " " + d(3),
        Shape.TAX_SLASH: lambda: "00/" + d(3) + "/" + d(5),
        Shape.TAX_SLASH_LONG: lambda: "000/" + d(4) + "/" + d(5),
        Shape.TAX_PREFIX: lambda: "ZZ000" + d(6),
        Shape.TAX_SUFFIX: lambda: "000" + d(4) + "ZZ",
        Shape.TAX_DOTTED: lambda: "ZZ000." + d(3) + "." + d(3) + "Z",
        Shape.SSN_US: lambda: "000" + ("-", ".", " ", "-")[index % 4] + d(2) + ("-", ".", " ", "-")[index % 4] + d(4),
        Shape.SSN_TRIPLE: lambda: "000-" + d(3) + "-" + d(3),
        Shape.SSN_SWISS_DOT: lambda: "000." + d(4) + "." + d(4) + "." + d(2),
        Shape.SSN_SWISS_DASH: lambda: "000-" + d(4) + "-" + d(4) + "-" + d(2),
        Shape.SSN_MIXED: lambda: "ZZ0" + d(5) + "Z",
        Shape.SSN_TAIL: lambda: d(4),
        Shape.CARD_GROUPED: lambda: "000" + d(3) + "-" + d(2) + "-" + d(4),
        Shape.CARD_PREFIX: lambda: "ZZ0" + d(6),
        Shape.CARD_SUFFIX: lambda: "000" + d(4) + "Z",
        Shape.NATIONAL_COMPACT: lambda: "000" + d(6 + index % 4),
        Shape.NATIONAL_GROUPED: lambda: "00 " + d(3) + " " + d(3) + " " + mark,
        Shape.NATIONAL_EMBEDDED: lambda: "000" + d(3) + "Z" + d(4),
    }
    return values[shape]()


def reading(cell: Cell, text: str, start: int) -> bool:
    relation = PARTIAL if cell.shape == Shape.SSN_TAIL else CONNECTOR
    return re.search(rf"(?i){CUES[cell.shape.label]}[\"']?{relation}[\"']?$", text[:start]) is not None


def records(api: ModuleType, partition: str) -> list["Record"]:
    out = []
    for cell in (*CELLS, *TWINS):
        layer = "A" if cell.gold else "D"
        rng = api.Rng(api.PARTITION_SEEDS[partition], f"government/{cell.family}")
        for index in range(DOCS[layer]):
            v = value(cell.shape, rng, partition, index)
            text, gold, decoys = api._fill_with_decoys(cell.templates[partition],
                {"V": (v, cell.shape.label if cell.gold else api.DECOY_PREFIX + "benign")})
            validity = api.INVALID if cell.gold and cell.shape in INVALID_TAX_SHAPES else api.UNCHECKED if cell.gold else api.BENIGN
            out.append(api.Record(uid=f"agentic-{partition}-{layer}-{cell.family}-{index:03d}-{cell.surface}",
                partition=partition, layer=layer, family=cell.family, surface=cell.surface, validity=validity,
                group=f"{partition}-{layer}-{cell.family}-{index:03d}", template=f"government/{cell.family}/{partition}",
                language="de" if partition == "dev" else "en", region="DE" if partition == "dev" else "US",
                text=text, gold=gold, decoys=decoys))
    check(api, out)
    return out


def check(api: ModuleType, records: list["Record"]) -> None:
    cells = {c.family: c for c in (*CELLS, *TWINS)}
    population = {}
    paid = {kind: set() for kind in ("broad", "narrow")}
    scored = set()
    for record in records:
        if not record.surface.startswith("gov_"):
            continue
        cell = cells.get(record.family)
        if cell is None:
            raise api.LayerError(f"{record.uid}: unknown government family")
        if record.partition not in api.PARTITIONS:
            raise api.LayerError(f"{record.uid}: unknown government partition")
        population[record.uid] = population.get(record.uid, 0) + 1
        spans = record.gold if cell.gold else record.decoys
        if len(spans) != 1 or (cell.gold and record.decoys) or (not cell.gold and record.gold):
            raise api.LayerError(f"{record.uid}: expected one whole {'gold' if cell.gold else 'decoy'}")
        span = spans[0]
        encoded = record.text.encode('utf-8')
        if (type(span.start) is not int or type(span.end) is not int
                or not 0 <= span.start < span.end <= len(encoded)
                or encoded[span.start:span.end] != span.value.encode('utf-8')):
            raise api.LayerError(f"{record.uid}: government offsets do not select the inserted value")
        start, end = api._char_span(record.text, span)
        if not re.fullmatch(PATTERNS[cell.shape], span.value):
            raise api.LayerError(f"{record.uid}: value violates government shape")
        if span.label != (cell.shape.label if cell.gold else api.DECOY_PREFIX + 'benign'):
            raise api.LayerError(f"{record.uid}: wrong government label")
        if re.search(r"[A-Za-z0-9]$", record.text[:start]) or re.match(r"[A-Za-z0-9]", record.text[end:]):
            raise api.LayerError(f"{record.uid}: incomplete government value")
        if reading(cell, record.text, start) != cell.gold:
            raise api.LayerError(f"{record.uid}: government cue ownership disagrees with gold")
        if cell.shape in INVALID_TAX_SHAPES and api.steuer_id_valid(span.value):
            raise api.LayerError(f"{record.uid}: synthetic Steuer-ID must fail checksum")
        if not cell.gold and bool(re.search(CUES[cell.shape.label], record.text, re.I)) != cell.near_cue:
            raise api.LayerError(f"{record.uid}: wrong near-cue counterweight")
        layer = 'A' if cell.gold else 'D'
        validity = api.INVALID if cell.gold and cell.shape in INVALID_TAX_SHAPES else api.UNCHECKED if cell.gold else api.BENIGN
        if (record.layer != layer or record.surface != cell.surface
                or record.validity != validity
                or record.template != f'government/{cell.family}/{record.partition}'
                or record.language != ('de' if record.partition == 'dev' else 'en')
                or record.region != ('DE' if record.partition == 'dev' else 'US')
                or record.text != cell.templates[record.partition].replace('{V}', span.value)
                or record.uid not in {
                    f'agentic-{record.partition}-{layer}-{cell.family}-{i:03d}-{cell.surface}'
                    for i in range(DOCS[layer])
                }
                or record.group != record.uid.removeprefix('agentic-').removesuffix('-' + cell.surface)):
            raise api.LayerError(f"{record.uid}: government lineage or schema differs")
        if cell.gold:
            scored.add(cell.shape)
        for kind, table in (("broad", BROAD), ("narrow", NARROW)):
            matches = api._overlaps(table[cell.shape], record.text, start, end)
            # A newline is a deliberate ownership case a same-line mutant misses.
            if cell.gold and kind == "broad" and not matches:
                raise api.LayerError(f"{record.uid}: broad mutant misses gold")
            if not cell.gold and matches:
                paid[kind].add(cell.shape)
    for kind, shapes in paid.items():
        missing = scored - shapes
        if missing:
            raise api.LayerError(f"government shapes with no {kind} D cost: {sorted(s.value for s in missing)}")
    partitions = {r.partition for r in records if r.surface.startswith("gov_")}
    if len(partitions) != 1:
        raise api.LayerError('government population must contain one complete partition')
    expected = {
        f"agentic-{partition}-{'A' if cell.gold else 'D'}-{cell.family}-{index:03d}-{cell.surface}": 1
        for partition in partitions for cell in cells.values()
        for index in range(DOCS['A' if cell.gold else 'D'])
    }
    if population != expected:
        raise api.LayerError('government population has missing, duplicate or unexpected records')
