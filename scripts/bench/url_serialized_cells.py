"""Inactive additions, with insertion gold independent of the URL detector."""
from itertools import product

from agentic_layers import (
    BENIGN, REPEAT, UNCHECKED, LayerError, PARTITION_SEEDS, Record, Rng, _Builder,
)
from url_cells import DOCS_PER_CELL, _account

SLASHES = ("/", r"\/", r"\u002f", r"\u002F")
SCHEMES = tuple(left + right for left, right in product(SLASHES, repeat=2))
VARIANTS = (
    "account_first", "account_middle", "account_final", "bmp_lower", "bmp_upper",
    "bmp_mixed", "surrogate", "literal_unicode", "path_separator", "query_owner",
    "query_separators", "terminal_unicode_slash", "structural_data",
    "reference_docs", "reference_repo",
) + tuple(f"scheme_{index:02d}" for index in range(len(SCHEMES)))
MALFORMED_TAILS = (
    r"\u", r"\u0", r"\u00", r"\u006", r"\u00xz", r"\U0061", r"\qtail", "\\", r"\\u0061",
)
MALFORMED_SCHEMES = (
    r"https:\u002", r"https:\u002x\u002f", r"https:\U002f\u002f",
    r"https:\\u002f\\u002f", r"https:\q\/", r"https:\u002f",
)
JSON_SURFACES = ("json_compact", "json_nested")
HTML_SURFACES = ("html_img", "html_link")
CELLS = (
    tuple((layer, variant, surface) for layer in ("A", "D", "R")
          for variant in VARIANTS for surface in JSON_SURFACES)
    + tuple(("A", f"malformed_tail_{index}", "raw_precision")
            for index in range(len(MALFORMED_TAILS)))
    + tuple(("D", f"malformed_scheme_{index}", "raw_precision")
            for index in range(len(MALFORMED_SCHEMES)))
    + tuple((layer, "selfclosing", surface) for layer in ("A", "D", "R")
            for surface in HTML_SURFACES)
)


def _escaped_character(value: str, position: int, upper: bool = False) -> str:
    code = format(ord(value[position]), "04X" if upper else "04x")
    return value[:position] + "\\u" + code + value[position + 1:]


def raw_value(variant: str, account: str, index: int, *, anchored: bool = True) -> str:
    """Choose finite source spellings before insertion, never detector-derived gold."""
    anchor = "https://"
    host = "portal.example.invalid"
    path = "/users/" + account
    if variant.startswith("scheme_"):
        anchor = "https:" + SCHEMES[int(variant.removeprefix("scheme_"))]
        path = r"/users/\u" + format(ord(account[0]), "04x") + account[1:] + r"\u002F"
    elif variant in {"account_first", "account_middle", "account_final"}:
        position = {"account_first": 0, "account_middle": len(account) // 2,
                    "account_final": len(account) - 1}[variant]
        path = "/users/" + _escaped_character(account, position, bool(index % 2))
    elif variant.startswith("bmp_"):
        units = {
            "bmp_lower": (r"\u00df", r"\u00e9", r"\u6771", r"\u4eac"),
            "bmp_upper": (r"\u00DF", r"\u00E9", r"\u6771", r"\u4EAC"),
            "bmp_mixed": (r"\u00Df", r"\u00e9", r"\u6771\u4EaC", r"\u00E9\u00df"),
        }[variant]
        path += "/" + units[index]
    elif variant == "surrogate":
        path += "/" + (r"\uD83D\uDE80", r"\ud83d\ude80", r"\uD83d\udE80", r"\ud83D\uDe80")[index]
    elif variant == "literal_unicode":
        path += "/" + ("é", "ß", "東京", "🚀")[index] + r"\u00df"
    elif variant == "path_separator":
        path = "/users" + (r"\u002f", r"\u002F", r"\/", r"\u002f")[index] + _escaped_character(account, index)
    elif variant == "query_owner":
        path += "?owner=" + _escaped_character(account, index)
    elif variant == "query_separators":
        path += r"\u003fowner\u003d" + _escaped_character(account, index) + r"\u0026next\u003d\u002Forders\u0023settings"
    elif variant == "terminal_unicode_slash":
        path = path.replace("/", r"\/") + (r"\u002f" if index % 2 else r"\u002F")
    elif variant == "structural_data":
        path += r"/\u0022\u003C\u003e\u007B\u007d\u005c"
    elif variant in {"reference_docs", "reference_repo"}:
        host = "docs.example.invalid" if variant == "reference_docs" else "github.example.invalid"
        path = "/guide/" + _escaped_character(account, index)
    elif variant != "selfclosing":
        raise LayerError("unknown serialized URL variant")
    return (anchor if anchored else "") + host + path


def render(partition: str, surface: str, kind: str, values: tuple[str, ...],
           *, benign: bool = False, near: str = "", tail: str = ""):
    """Append source values and decoys through the canonical byte-span builder."""
    if partition not in PARTITION_SEEDS or surface not in (*JSON_SURFACES, *HTML_SURFACES, "raw_precision"):
        raise LayerError("unknown serialized URL partition or surface")
    builder = _Builder()
    note = "ß" if partition == "dev" else "é"
    if surface in HTML_SURFACES:
        tag, key = ("img", "src") if surface == "html_img" else ("link", "href")
        builder.text(f"{note}:case={kind} ")
        for index, value in enumerate(values):
            if index:
                builder.text(" then ")
            builder.text(f"<{tag} {key}='")
            (builder.decoy if benign else builder.gold)(value, "benign" if benign else "URL")
            builder.text("'/>")
        builder.text(" units=81.9")
        if near:
            builder.text(" near=")
            builder.decoy(near, "near_miss")
    else:
        key = "profile" if partition == "dev" else "account"
        builder.text(f'{{"note":"{note}","case":"{kind}",')
        if surface == "json_nested":
            builder.text(f'"{key}":{{')
        builder.text('"w":"')
        for index, value in enumerate(values):
            if index:
                builder.text('","again":"')
            (builder.decoy if benign else builder.gold)(value, "benign" if benign else "URL")
            if tail:
                builder.decoy(tail, "unsupported_escape")
        builder.text('"')
        if surface == "json_nested":
            builder.text("}")
        if near:
            builder.text(',"near":"')
            builder.decoy(near, "near_miss")
            builder.text('"')
        builder.text(',"n":42,"status":"ready"}' if partition == "dev" else ',"n":81.9,"status":"open"}')
    return builder.build()


def _record(partition: str, layer: str, kind: str, surface: str, index: int) -> Record:
    rng = Rng(PARTITION_SEEDS[partition], f"url/serialized/{layer}/{kind}/{surface}/{index}")
    account = _account(rng, partition)
    near = ""
    tail = ""
    if kind.startswith("malformed_tail_"):
        values = (raw_value("selfclosing", account, index),)
        tail = MALFORMED_TAILS[int(kind.removeprefix("malformed_tail_"))]
    elif kind.startswith("malformed_scheme_"):
        values = (MALFORMED_SCHEMES[int(kind.removeprefix("malformed_scheme_"))]
                  + "portal.example.invalid/users/" + account,)
    else:
        value = raw_value(kind, account, index, anchored=layer != "D")
        values = (value, value) if layer == "R" else (value,)
        if layer == "R":
            near = raw_value(kind, account + "x", index, anchored=False)
    text, gold, decoys = render(partition, surface, kind, values, benign=layer == "D", near=near, tail=tail)
    family = "url_serialized_" + kind
    return Record(
        uid=f"agentic-{partition}-{layer}-{family}-{index:03d}-url_serialized_{surface}",
        partition=partition, layer=layer, family=family, surface="url_serialized_" + surface,
        validity=BENIGN if layer == "D" else REPEAT if layer == "R" else UNCHECKED,
        group=f"{partition}-{layer}-{family}-{surface}-{index:03d}",
        template=f"url/serialized/{surface}/{kind}/{partition}", language="en", region="US",
        text=text, gold=gold, decoys=decoys,
    )


def generate(partition: str) -> list[Record]:
    if partition not in PARTITION_SEEDS:
        raise LayerError("unknown serialized URL partition")
    records = [_record(partition, layer, kind, surface, index)
               for layer, kind, surface in CELLS for index in range(DOCS_PER_CELL)]
    check(records, partition)
    return records


def check(records: list[Record], partition: str) -> None:
    if partition not in PARTITION_SEEDS or any(r.partition != partition for r in records):
        raise LayerError("serialized URL partition mismatch")
    expected_ids = {
        f"agentic-{partition}-{layer}-url_serialized_{kind}-{index:03d}-url_serialized_{surface}"
        for layer, kind, surface in CELLS for index in range(DOCS_PER_CELL)
    }
    if len(records) != len(expected_ids) or {r.uid for r in records} != expected_ids:
        raise LayerError("serialized URL population or counterweight mismatch")
    for record in records:
        if record.partition != partition or not record.template.endswith("/" + partition):
            raise LayerError("serialized URL partition mismatch")
        raw = record.text.encode()
        for span in (*record.gold, *record.decoys):
            if not 0 <= span.start < span.end <= len(raw) or raw[span.start:span.end] != span.value.encode():
                raise LayerError("serialized URL span does not select source bytes")
        if len(record.gold) != {"A": 1, "D": 0, "R": 2}[record.layer] or any(g.label != "URL" for g in record.gold):
            raise LayerError("serialized URL gold cardinality or label mismatch")
        if any(a.end >= b.start for a, b in zip(record.gold, record.gold[1:])):
            raise LayerError("serialized URL repeat overlaps structural neighbors")
        kind = record.family.removeprefix("url_serialized_")
        malformed = kind.startswith("malformed_tail_")
        expected_decoys = 1 if record.layer in {"D", "R"} or malformed else 0
        if len(record.decoys) != expected_decoys:
            raise LayerError("serialized URL decoy cardinality mismatch")
        values = tuple(g.value for g in (record.decoys if record.layer == "D" else record.gold))
        tail = MALFORMED_TAILS[int(kind.removeprefix("malformed_tail_"))] if malformed else ""
        near = record.decoys[0].value if record.layer == "R" else ""
        rebuilt = render(partition, record.surface.removeprefix("url_serialized_"), kind, values,
                         benign=record.layer == "D", near=near, tail=tail)
        if rebuilt != (record.text, record.gold, record.decoys):
            raise LayerError("serialized URL surrounding bytes or decoy ownership changed")
