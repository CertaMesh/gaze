"""Inactive URL raw-span cells for later agentic generator integration.

No import or call from the active generator: root must allocate a version and
extend its explicit label contract before these cells can be measured.
"""
from __future__ import annotations

from dataclasses import dataclass

from agentic_layers import BENIGN, REPEAT, UNCHECKED, Gold, LayerError, PARTITION_SEEDS, Record, Rng

# Separate source templates, chosen before generating either partition.
TEMPLATES = {
    "tool_json": {
        "dev": ('ß:{"profile":"', '","count":42,"state":"ready"}'),
        "test": ('é:{"account":{"link":"', '"},"amount":81.9,"status":"open"}'),
    },
    "html_double": {
        "dev": ('ß:<a href="', '">Profile</a> count=42'),
        "test": ('é:<a href="', '">Account</a> units=81.9'),
    },
    "html_single": {
        "dev": ("ß:<a href='", "'>Profile</a> count=42"),
        "test": ("é:<a href='", "' rel='next'>Account</a> units=81.9"),
    },
    "markdown": {
        "dev": ('ß:[profile](', ') units=42'),
        "test": ('é:[account](', ') count=81.9'),
    },
    "prose": {
        "dev": ('ß:Profile link <', '> ends here; count 42.'),
        "test": ('é:Account link (', '). Count 81.9 stays visible.'),
    },
    "log_kv": {
        "dev": ('ß:op=profile href="', '" count=42 state=ready'),
        "test": ('é:event=account link="', '" units=81.9 result=open'),
    },
}
SEPARATORS = {
    "tool_json": {"dev": '","repeat":"', "test": '","again":"'},
    "html_double": '" data-again="',
    "html_single": "' data-again='",
    "markdown": ') [again](',
    "prose": {"dev": '> then <', "test": ') and ('},
    "log_kv": {"dev": '" repeat="', "test": '" again="'},
}


def render(
    partition: str, surface: str, values: tuple[str, ...], decoy: str = "", *, benign: bool = False
) -> tuple[str, tuple[Gold, ...], tuple[Gold, ...]]:
    """Append original serialized values; label only the bytes actually inserted."""
    if partition not in PARTITION_SEEDS or surface not in TEMPLATES:
        raise LayerError("unknown URL partition or surface")
    if surface == "html_single" and any("'" in value for value in values):
        raise LayerError("literal apostrophe in a single-quoted attribute is ambiguous")
    prefix, suffix = TEMPLATES[surface][partition]
    parts = [prefix]
    gold = []
    decoys = []
    offset = len(prefix.encode("utf-8"))
    for index, value in enumerate(values):
        if index:
            separator = SEPARATORS[surface]
            if isinstance(separator, dict):
                separator = separator[partition]
            parts.append(separator)
            offset += len(separator.encode("utf-8"))
        span = Gold(offset, offset + len(value.encode("utf-8")), "benign" if benign else "URL", value)
        (decoys if benign else gold).append(span)
        parts.append(value)
        offset += len(value.encode("utf-8"))
    parts.append(suffix)
    offset += len(suffix.encode("utf-8"))
    if decoy:
        parts.append(" near=")
        offset += len(" near=".encode("utf-8"))
        decoys.append(Gold(offset, offset + len(decoy.encode("utf-8")), "near_miss", decoy))
        parts.append(decoy)
    return "".join(parts), tuple(gold), tuple(decoys)


DOCS_PER_CELL = 4
SHAPES = (
    "https", "http", "uppercase", "www", "escaped", "scheme_left", "scheme_right",
    "mixed_path", "terminal_slash", "query_fragment", "apostrophe", "encoded_delimiters",
    "reference_docs", "reference_repo", "escaped_query_fragment", "escaped_apostrophe",
    "escaped_www", "host_https", "host_www", "http_uppercase", "escaped_uppercase", "escaped_www_scheme",
)
COUNTERWEIGHTS = (
    "bare_host", "bare_path", "scheme_fragment", "www_fragment", "filename", "numeric", "version",
)


@dataclass(frozen=True)
class UrlCell:
    shape: str
    surface: str


# Invalid single-quoted attributes do not define a detector boundary contract.
URL_CELLS = tuple(
    UrlCell(shape, surface) for shape in SHAPES for surface in TEMPLATES
    if surface != "html_single" or shape not in {"apostrophe", "escaped_apostrophe"}
)


def _account(rng: Rng, partition: str) -> str:
    return ("duser" if partition == "dev" else "tuser") + "".join(
        rng.choice(tuple("abcdefghjkmnpqrstuvwxyz")) for _ in range(10)
    )


def _url(shape: str, account: str) -> str:
    base = f"https://portal.example.invalid/users/{account}"
    return {
        "https": base,
        "http": base.replace("https:", "http:", 1),
        "uppercase": base.upper(),
        "www": f"www.profile.example.invalid/users/{account}",
        "escaped": base.replace("/", "\\/"),
        "scheme_left": base.replace("//", "\\//", 1),
        "scheme_right": base.replace("//", "/\\/", 1),
        "mixed_path": base.replace("/users/", "/users\\/"),
        "terminal_slash": base.replace("/", "\\/") + "\\/",
        "query_fragment": base + f"?next=/orders&owner={account}#settings",
        "apostrophe": base + "/O'Brien?q=O'Brien",
        "encoded_delimiters": base + "/%22%3C%3E%7B%7D%5C?q=O%27Brien",
        "escaped_query_fragment": (base + f"?next=/orders&owner={account}#settings").replace("/", "\\/"),
        "escaped_apostrophe": (base + "/O'Brien?q=O'Brien").replace("/", "\\/"),
        "escaped_uppercase": base.replace("https:", "http:", 1).upper().replace("/", "\\/"),
        "escaped_www_scheme": f"https://www.example.invalid/users/{account}".replace("/", "\\/"),
        "escaped_www": f"www.profile.example.invalid/users/{account}".replace("/", "\\/"),
        "host_https": f"https://{account}.example.invalid",
        "host_www": f"www.{account}.example.invalid",
        "http_uppercase": base.replace("https:", "http:", 1).upper(),
        "reference_docs": f"https://docs.example.invalid/guide/{account}",
        "reference_repo": f"https://github.example.invalid/org/{account}",
    }[shape]


def _benign(kind: str, account: str, index: int) -> str:
    return {
        "bare_host": f"{account}.example.invalid",
        "bare_path": f"portal.example.invalid/users/{account}",
        "scheme_fragment": ("https://", "http://", r"https:\/", r"HTTP:\/\/")[index],
        "www_fragment": ("www.", "WWW.", "www", "WWW")[index],
        "filename": f"quarterly.{account}.report.pdf",
        "numeric": ("81.9", "400.13", "21.6", "72.4")[index],
        "version": ("v3.1.16", "v4.8.22", "v5.2.31", "v6.9.12")[index],
    }[kind]


def _record(partition: str, layer: str, family: str, surface: str, index: int,
            values: tuple[str, ...], decoy: str = "") -> Record:
    text, gold, decoys = render(partition, surface, values, decoy, benign=layer == "D")
    return Record(
        uid=f"agentic-{partition}-{layer}-{family}-{index:03d}-url_{surface}",
        partition=partition, layer=layer, family=family, surface="url_" + surface,
        validity=BENIGN if layer == "D" else REPEAT if layer == "R" else UNCHECKED,
        group=f"{partition}-{layer}-{family}-{index:03d}",
        template=f"url/{surface}/{partition}", language="en", region="US",
        text=text, gold=gold, decoys=decoys,
    )


def generate(partition: str) -> list[Record]:
    """Return additive A/D/R records without changing the active generator."""
    if partition not in PARTITION_SEEDS:
        raise LayerError(f"unknown URL partition {partition!r}")
    records = []
    for cell in URL_CELLS:
        rng = Rng(PARTITION_SEEDS[partition], f"url/A/{cell.shape}/{cell.surface}")
        for index in range(DOCS_PER_CELL):
            value = _url(cell.shape, _account(rng, partition))
            records.append(_record(partition, "A", "url_" + cell.shape, cell.surface, index, (value,)))
    for kind in COUNTERWEIGHTS:
        for surface in TEMPLATES:
            rng = Rng(PARTITION_SEEDS[partition], f"url/D/{kind}/{surface}")
            for index in range(DOCS_PER_CELL):
                value = _benign(kind, _account(rng, partition), index)
                if kind == "bare_path" and index % 2:
                    value = value.replace("/", "\\/")
                records.append(_record(partition, "D", "url_" + kind, surface, index, (value,)))
    for kind in ("plain", "escaped", "mixed"):
        for surface in TEMPLATES:
            rng = Rng(PARTITION_SEEDS[partition], f"url/R/{kind}/{surface}")
            for index in range(DOCS_PER_CELL):
                account = _account(rng, partition)
                plain = _url("https", account)
                escaped = _url("escaped", account)
                # These are two raw occurrences, never a promised canonical identity.
                values = (plain, plain) if kind == "plain" else (escaped, escaped) if kind == "escaped" else (plain, escaped)
                near_miss = f"portal.example.invalid/users/{account}x"
                records.append(_record(partition, "R", "url_repeat_" + kind, surface, index, values, near_miss))
    check(records, partition)
    return records


def check(records: list[Record], partition: str) -> None:
    """Reject missing cells, crossed partitions and corrupt original-byte spans."""
    if partition not in PARTITION_SEEDS or any(r.partition != partition for r in records):
        raise LayerError("URL partition mismatch")
    expected = {
        ("A", "url_" + cell.shape, cell.surface, index)
        for cell in URL_CELLS for index in range(DOCS_PER_CELL)
    } | {
        ("D", "url_" + kind, surface, index)
        for kind in COUNTERWEIGHTS for surface in TEMPLATES for index in range(DOCS_PER_CELL)
    } | {
        ("R", "url_repeat_" + kind, surface, index)
        for kind in ("plain", "escaped", "mixed") for surface in TEMPLATES for index in range(DOCS_PER_CELL)
    }
    expected_ids = {
        f"agentic-{partition}-{layer}-{family}-{index:03d}-url_{surface}"
        for layer, family, surface, index in expected
    }
    if len(records) != len(expected_ids) or {r.uid for r in records} != expected_ids:
        raise LayerError("URL population or counterweight mismatch")
    for record in records:
        raw = record.text.encode("utf-8")
        for span in (*record.gold, *record.decoys):
            if not (0 <= span.start < span.end <= len(raw)) or raw[span.start:span.end] != span.value.encode("utf-8"):
                raise LayerError(f"{record.uid}: span does not select original source bytes")
        expected_gold = {"A": 1, "D": 0, "R": 2}[record.layer]
        if len(record.gold) != expected_gold or any(g.label != "URL" for g in record.gold):
            raise LayerError(f"{record.uid}: URL gold cardinality or label mismatch")
        if any(left.end >= right.start for left, right in zip(record.gold, record.gold[1:])):
            raise LayerError(f"{record.uid}: repeat spans overlap or lack surrounding bytes")
        values = tuple(g.value for g in (record.decoys if record.layer == "D" else record.gold))
        near = record.decoys[0].value if record.layer == "R" and len(record.decoys) == 1 else ""
        rebuilt = render(partition, record.surface.removeprefix("url_"), values, near, benign=record.layer == "D")
        if rebuilt != (record.text, record.gold, record.decoys):
            raise LayerError(f"{record.uid}: surrounding bytes or decoy ownership changed")


def generate_extended(partition: str) -> list[Record]:
    """Keep the frozen prefix, then append inactive serialization coverage."""
    from url_serialized_cells import generate as serialized
    return generate(partition) + serialized(partition)
