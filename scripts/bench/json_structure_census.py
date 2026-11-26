#!/usr/bin/env python3
"""Where false positives land in JSON and log structure, from one record.

A false positive inside an ordinary string value costs readability. One on a
JSON key, number, `true`/`false`/`null`, a tool selector, a tool-call argument
or the syntax around them can break the agent's workflow: the payload no
longer parses, changes type, or calls the wrong tool with the wrong
arguments. This report maps every false-positive byte to the innermost JSON
position it sits in and names the recognizer that detected it, so a fix can
be measured per position instead of by net bytes.

Positions (`ROLES`) are read from the source text with a strict parser.
String values that hold serialized JSON are decoded with a byte map and read
the same way, so a key three serializations deep maps back to its source
bytes. A protected range that cuts an escape sequence in two is flagged as
well. Tool selectors and argument subtrees are named by the carrier shapes in
`CARRIERS`: a declared list of provider envelopes, not an inference from
generic key names. On a log carrier a `key=` or `key:` token outside quotes
is a log key; an address inside a quoted value is not.

The census is diagnostic: it measures structural risk, not an executed
downstream failure, and it says nothing about whether a byte is PII. It reads
only documents whose text can be rebuilt (the generated layers and the
committed hard negatives); metadata-only holdout documents are counted apart.

    python scripts/bench/json_structure_census.py report RECORD --output census.json
    python scripts/bench/json_structure_census.py check RECORD --expect census.json
    python scripts/bench/json_structure_census.py gate BASE.json CANDIDATE.json
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from dataclasses import dataclass, field
import json
from pathlib import Path
import re
import sys
from typing import Iterable, Sequence

import agentic_layers as agentic
import gaze_bench_score as score
import scorecard_record as records

SCHEMA_VERSION = 1
ROOT = Path(__file__).resolve().parents[2]
NEGATIVE_CORPUS = ROOT / "crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl"

KEY, NUMBER, LITERAL, SELECTOR, ARGUMENT, STRING, SYNTAX, LOG_KEY = (
    "key", "number", "literal", "selector", "argument", "string", "syntax", "log_key")
PROSE = "prose"
ROLES = (KEY, NUMBER, LITERAL, SELECTOR, ARGUMENT, STRING, SYNTAX, LOG_KEY, PROSE)
# A change to any of these can break parsing, typing or tool dispatch.
CRITICAL_ROLES = frozenset({KEY, NUMBER, LITERAL, SELECTOR, ARGUMENT, SYNTAX, LOG_KEY})
LEARNED_SOURCES = ("ner", "nym-")

# Declared tool-call carriers. An object is an envelope when it has a string
# selector member (`name`, `tool` or `tool_name`) beside an argument member
# holding an object or serialized JSON: OpenAI `function` and legacy
# `function_call`, Anthropic `tool_use`, Gemini `functionCall`, MCP
# `tools/call` params. A bare `arguments`/`args` member is an argument
# subtree without a selector. `name` alone never makes a selector, and a
# JSON-Schema tool definition (`parameters`) is not a call.
CARRIERS = {
    "selectors": ("name", "tool", "tool_name"),
    "arguments": ("arguments", "args", "input"),
    "bare_arguments": ("arguments", "args"),
}
# Surfaces whose text is a log or field carrier (`key=value`, `key: value`).
_LOG_SURFACE = re.compile(r"(?:^|_)log(?:_|$)")
# Serialized JSON nested deeper than this stays an opaque string value.
MAX_SERIALIZATION_DEPTH = 16
_LOG_KEY = re.compile(r"[A-Za-z_][A-Za-z0-9_.-]*\s*[=:]")
_NUMBER = re.compile(r"-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?")
_WHITESPACE = " \t\n\r"


class CensusError(Exception):
    pass


# --------------------------------------------------------------------------
# Strict JSON parsing with source positions.


@dataclass
class Node:
    kind: str  # object, array, string, number, literal
    start: int  # index into the parsed str
    end: int
    members: list = field(default_factory=list)  # object: [(key Node, value Node)]
    items: list = field(default_factory=list)  # array
    value: str = ""  # decoded string
    chars: list = field(default_factory=list)  # string: [(start, end)] source chars per decoded char
    escapes: list = field(default_factory=list)  # string: [(start, end)] escape sequences


class _Parser:
    def __init__(self, text: str) -> None:
        self.s = text
        self.syntax: list[tuple[int, int]] = []

    def ws(self, i: int) -> int:
        start = i
        while i < len(self.s) and self.s[i] in _WHITESPACE:
            i += 1
        if i > start:
            self.syntax.append((start, i))
        return i

    def fail(self) -> None:
        raise ValueError

    def value(self, i: int) -> Node:
        if i >= len(self.s):
            self.fail()
        ch = self.s[i]
        if ch == "{":
            return self.object(i)
        if ch == "[":
            return self.array(i)
        if ch == '"':
            return self.string(i)
        for word in ("true", "false", "null"):
            if self.s.startswith(word, i):
                return Node("literal", i, i + len(word))
        match = _NUMBER.match(self.s, i)
        if not match:
            self.fail()
        return Node("number", i, match.end())

    def object(self, i: int) -> Node:
        node = Node("object", i, i)
        self.syntax.append((i, i + 1))
        i = self.ws(i + 1)
        seen = set()
        if i < len(self.s) and self.s[i] == "}":
            self.syntax.append((i, i + 1))
            node.end = i + 1
            return node
        while True:
            if i >= len(self.s) or self.s[i] != '"':
                self.fail()
            key = self.string(i)
            if key.value in seen:  # duplicate keys are ambiguous: no structure
                self.fail()
            seen.add(key.value)
            i = self.ws(key.end)
            if i >= len(self.s) or self.s[i] != ":":
                self.fail()
            self.syntax.append((i, i + 1))
            value = self.value(self.ws(i + 1))
            node.members.append((key, value))
            i = self.ws(value.end)
            if i < len(self.s) and self.s[i] == ",":
                self.syntax.append((i, i + 1))
                i = self.ws(i + 1)
                continue
            if i < len(self.s) and self.s[i] == "}":
                self.syntax.append((i, i + 1))
                node.end = i + 1
                return node
            self.fail()

    def array(self, i: int) -> Node:
        node = Node("array", i, i)
        self.syntax.append((i, i + 1))
        i = self.ws(i + 1)
        if i < len(self.s) and self.s[i] == "]":
            self.syntax.append((i, i + 1))
            node.end = i + 1
            return node
        while True:
            item = self.value(i)
            node.items.append(item)
            i = self.ws(item.end)
            if i < len(self.s) and self.s[i] == ",":
                self.syntax.append((i, i + 1))
                i = self.ws(i + 1)
                continue
            if i < len(self.s) and self.s[i] == "]":
                self.syntax.append((i, i + 1))
                node.end = i + 1
                return node
            self.fail()

    def string(self, i: int) -> Node:
        node = Node("string", i, i)
        self.syntax.append((i, i + 1))
        i += 1
        decoded = []
        while True:
            if i >= len(self.s):
                self.fail()
            ch = self.s[i]
            if ch == '"':
                self.syntax.append((i, i + 1))
                node.end = i + 1
                node.value = "".join(decoded)
                return node
            if ord(ch) < 0x20:
                self.fail()
            if ch != "\\":
                decoded.append(ch)
                node.chars.append((i, i + 1))
                i += 1
                continue
            escape = self.s[i + 1:i + 2]
            if escape in ('"', "\\", "/", "b", "f", "n", "r", "t"):
                decoded.append(json.loads(f'"\\{escape}"'))
                node.chars.append((i, i + 2))
                node.escapes.append((i, i + 2))
                i += 2
                continue
            if escape != "u" or not re.fullmatch(r"[0-9a-fA-F]{4}", self.s[i + 2:i + 6]):
                self.fail()
            code, end = int(self.s[i + 2:i + 6], 16), i + 6
            if 0xD800 <= code <= 0xDBFF:
                low = self.s[end:end + 6]
                if not re.fullmatch(r"\\u[dD][c-fC-F][0-9a-fA-F]{2}", low):
                    self.fail()
                code = 0x10000 + ((code - 0xD800) << 10) + (int(low[2:], 16) - 0xDC00)
                end += 6
            elif 0xDC00 <= code <= 0xDFFF:
                self.fail()
            decoded.append(chr(code))
            node.chars.append((i, end))
            node.escapes.append((i, end))
            i = end


def _complete_values(text: str) -> Iterable[tuple[Node, list[tuple[int, int]]]]:
    """Every complete object, or array holding an object or array, in `text`.

    A bracketed prose fragment such as `[1]` or `[draft]` is not JSON
    structure, so a bare scalar array never counts.
    """
    i = 0
    while i < len(text):
        if text[i] not in "{[":
            i += 1
            continue
        parser = _Parser(text)
        try:
            node = parser.value(i)
        except (ValueError, IndexError, RecursionError):
            i += 1
            continue
        if node.kind == "object" or any(item.kind in ("object", "array") for item in node.items):
            yield node, parser.syntax
            i = node.end
        else:
            i += 1


# --------------------------------------------------------------------------
# Byte roles.


@dataclass
class Structure:
    """Per source byte: innermost role (None outside JSON and log keys), the
    string leaf it belongs to, and the source ranges of escape sequences."""

    roles: list
    leaves: list
    escapes: list = field(default_factory=list)

    def critical(self, offset: int) -> bool:
        return self.roles[offset] in CRITICAL_ROLES


def _envelope(node: Node) -> tuple[set[str], set[str]]:
    """Selector and argument member names this object declares, if any."""
    members = {key.value: value for key, value in node.members}

    def structured(value: Node) -> bool:
        return value.kind == "object" or (
            value.kind == "string" and value.value.lstrip(_WHITESPACE)[:1] in ("{", "["))

    arguments = {name for name in CARRIERS["arguments"]
                 if name in members and structured(members[name])}
    selectors = {name for name in CARRIERS["selectors"]
                 if members.get(name) is not None and members[name].kind == "string"} if arguments else set()
    if not selectors:
        arguments &= set(CARRIERS["bare_arguments"])
    return selectors, arguments


def structure(text: str, *, log_carrier: bool = False) -> Structure:
    encoded_offsets = [0]
    for ch in text:
        encoded_offsets.append(encoded_offsets[-1] + len(ch.encode("utf-8")))
    size = encoded_offsets[-1]
    result = Structure([None] * size, [None] * size)
    leaf_ids = iter(range(1, 1 << 62))

    def mark(source: Sequence[tuple[int, int]], start: int, end: int, role: str, leaf=None) -> None:
        for a, b in source[start:end]:
            for offset in range(a, b):
                result.roles[offset] = role
                if leaf is not None:
                    result.leaves[offset] = leaf

    def walk(node: Node, source, depth: int, in_arguments: bool, selector: bool) -> None:
        if node.kind == "object":
            selectors, arguments = _envelope(node)
            for key, value in node.members:
                mark(source, key.start, key.end, KEY)
                walk(value, source, depth, in_arguments or key.value in arguments,
                     key.value in selectors)
        elif node.kind == "array":
            for item in node.items:
                walk(item, source, depth, in_arguments, False)
        elif node.kind == "number":
            mark(source, node.start, node.end, NUMBER)
        elif node.kind == "literal":
            mark(source, node.start, node.end, LITERAL)
        else:
            role = SELECTOR if selector else ARGUMENT if in_arguments else STRING
            leaf = next(leaf_ids)
            mark(source, node.start + 1, node.end - 1, role, leaf)
            for a, b in node.escapes:
                spans = source[a:b]
                result.escapes.append((spans[0][0], spans[-1][1]))
            if depth < MAX_SERIALIZATION_DEPTH and node.value.lstrip(_WHITESPACE)[:1] in ("{", "["):
                inner = [(source[a][0], source[b - 1][1]) for a, b in node.chars]
                scan(node.value, inner, depth + 1, in_arguments)

    def scan(value: str, source, depth: int, in_arguments: bool) -> None:
        for node, syntax in _complete_values(value):
            for a, b in syntax:
                mark(source, a, b, SYNTAX)
            walk(node, source, depth, in_arguments, False)

    chars = [(encoded_offsets[i], encoded_offsets[i + 1]) for i in range(len(text))]
    scan(text, chars, 0, False)
    if log_carrier:
        _mark_log_keys(text, encoded_offsets, result)
    return result


def _mark_log_keys(text: str, offsets: Sequence[int], result: Structure) -> None:
    """`key=` / `key:` tokens outside quotes and outside JSON, at a token start.

    A quote opens only at a value start (after `=`, `:` or whitespace), a
    backslash escapes the next character inside it, and a newline ends it, so
    an apostrophe in prose or an escaped quote cannot hide the next key.
    """
    quote = None
    i = 0
    while i < len(text):
        ch = text[i]
        if quote:
            if ch == "\\":
                i += 2
                continue
            if ch == quote or ch == "\n":
                quote = None
            i += 1
            continue
        if ch in "\"'" and (i == 0 or text[i - 1] in "=: \t\n"):
            quote = ch
            i += 1
            continue
        boundary = i == 0 or text[i - 1] in " \t\n;,"
        match = _LOG_KEY.match(text, i) if boundary else None
        if match and all(result.roles[b] is None for b in range(offsets[i], offsets[match.end()])):
            for b in range(offsets[i], offsets[match.end()]):
                result.roles[b] = LOG_KEY
            i = match.end()
            continue
        i += 1


# --------------------------------------------------------------------------
# False-positive fragments from an observation.


def _is_learned(source: str) -> bool:
    return source == "ner" or source.startswith("nym-")


@dataclass(frozen=True)
class Trace:
    start: int
    end: int
    label: str
    sources: tuple[str, ...]  # detecting recognizers overlapping each byte, joined per byte


def _byte_sources(trace: dict) -> dict[int, tuple[str, ...]]:
    detecting = records.DETECTING_ROLES
    per_byte: dict[int, set[str]] = defaultdict(set)
    for item in trace["provenance"]["contributions"]:
        if item["role"] not in detecting:
            continue
        for offset in range(max(trace["raw_start"], item["raw_start"]), min(trace["raw_end"], item["raw_end"])):
            per_byte[offset].add(item["recognizer_id"])
    return {offset: tuple(sorted(names)) for offset, names in per_byte.items()}


def _bridging_whitespace(raw: bytes, gold: Sequence[tuple[int, int]], traces: Sequence[dict]) -> set[int]:
    """Whitespace between two consecutive gold spans that one trace covers.

    The annotation left the separator of a multi-part value out of gold; a
    span that protects the whole value is not a structural false positive.
    """
    ordered = sorted(gold)
    excluded = set()
    for (_, left_end), (right_start, _) in zip(ordered, ordered[1:]):
        gap = raw[left_end:right_start]
        if not gap or gap.strip():
            continue
        if any(t["raw_start"] < left_end and t["raw_end"] > right_start for t in traces):
            excluded.update(range(left_end, right_start))
    return excluded


def fragments(document_id: str, layer: str, raw: bytes, gold: Sequence[tuple[int, int]],
              traces: Sequence[dict], shape: Structure | None) -> list[dict]:
    """Maximal runs of false-positive bytes sharing role, class and detector."""
    gold_bytes = {b for start, end in gold for b in range(start, end)}
    excluded = _bridging_whitespace(raw, gold, traces)
    escapes = shape.escapes if shape else []
    owner: dict[int, tuple[str, tuple[str, ...], bool]] = {}
    for trace in traces:
        sources = _byte_sources(trace)
        # A protected range whose own edge falls inside an escape sequence
        # replaces half of it, whatever position the bytes are in.
        cuts = any(a < edge < b for a, b in escapes for edge in (trace["raw_start"], trace["raw_end"]))
        for offset in range(trace["raw_start"], trace["raw_end"]):
            if offset in gold_bytes or offset in excluded:
                continue
            label, known, cut = owner.get(offset, (trace["class"], (), False))
            owner[offset] = (label, tuple(sorted(set(known) | set(sources.get(offset, ())))), cut or cuts)
    rows: list[dict] = []
    for offset in sorted(owner):
        label, sources, cut = owner[offset]
        role = shape.roles[offset] if shape and shape.roles[offset] else PROSE
        key = (role, label, sources, cut)
        if rows and rows[-1]["end"] == offset and rows[-1]["_key"] == key:
            rows[-1]["end"] += 1
            continue
        rows.append({"document_id": document_id, "layer": layer, "start": offset, "end": offset + 1,
                     "role": role, "class": label, "sources": list(sources), "_key": key, "_cut": cut})
    for row in rows:
        del row["_key"]
        row["splits_escape"] = key_cut = row.pop("_cut")
        row["critical"] = row["role"] in CRITICAL_ROLES or key_cut
    return rows


def _learned_only(trace: dict) -> bool:
    sources = {s for names in _byte_sources(trace).values() for s in names}
    return bool(sources) and all(_is_learned(s) for s in sources)


def learned_only_gold(gold: Sequence[tuple[int, int]], traces: Sequence[dict]) -> int:
    """Gold bytes only learned-only traces protect: what any learned veto could expose."""
    gold_intervals = score.merge_intervals(gold)
    def covered(selected):
        protected = score.merge_intervals([(t["raw_start"], t["raw_end"]) for t in selected])
        return score.interval_length(gold_intervals) - score.interval_length(
            score.subtract_intervals(gold_intervals, protected))
    return covered(traces) - covered([t for t in traces if not _learned_only(t)])


def learned_veto_exposure(gold: Sequence[tuple[int, int]], traces: Sequence[dict],
                          shape: Structure | None) -> dict[str, int]:
    """Gold bytes a blanket learned-detector veto would expose, per veto.

    Each veto drops every final trace detected only by learned recognizers
    that the veto matches, then counts gold bytes no remaining trace covers.
    `outside_one_string_leaf` drops learned traces that touch JSON but do not
    fit inside one string or argument value.
    """
    if shape is None:
        return {}
    learned_only = _learned_only

    def touches(trace: dict, role: str) -> bool:
        return any(shape.roles[b] == role for b in range(trace["raw_start"], trace["raw_end"]))

    def outside_leaf(trace: dict) -> bool:
        span = range(trace["raw_start"], trace["raw_end"])
        if all(shape.roles[b] is None for b in span):
            return False
        leaves = {shape.leaves[b] for b in span}
        return (len(leaves) != 1 or None in leaves
                or any(shape.roles[b] not in (STRING, ARGUMENT) for b in span))

    vetoes = {KEY: lambda t: touches(t, KEY), LITERAL: lambda t: touches(t, LITERAL),
              SELECTOR: lambda t: touches(t, SELECTOR), NUMBER: lambda t: touches(t, NUMBER),
              "outside_one_string_leaf": outside_leaf}
    gold_intervals = score.merge_intervals(gold)
    before = score.merge_intervals([(t["raw_start"], t["raw_end"]) for t in traces])
    protected_before = score.interval_length(gold_intervals) - score.interval_length(
        score.subtract_intervals(gold_intervals, before))
    exposure = {}
    for name, veto in vetoes.items():
        kept = [t for t in traces if not (learned_only(t) and veto(t))]
        after = score.merge_intervals([(t["raw_start"], t["raw_end"]) for t in kept])
        protected_after = score.interval_length(gold_intervals) - score.interval_length(
            score.subtract_intervals(gold_intervals, after))
        exposure[name] = protected_before - protected_after
    return exposure


# --------------------------------------------------------------------------
# Reading a record.


def _texts(header: dict) -> tuple[dict[str, tuple[str, str]], str]:
    """Rebuildable texts by document ID, with the surface used for log carriers."""
    identity = header.get("layer_identity") or {}
    generated = agentic.generate(agentic.PUBLISHED_PARTITION)
    corpus = agentic.manifest(agentic.PUBLISHED_PARTITION, generated)
    if identity and (identity.get("generator_version") != corpus["generator_version"]
                     or identity.get("generator_corpus_sha256") != corpus["corpus_sha256"]):
        raise CensusError("record measured a different generated corpus; rebuild it with "
                          f"generator v{identity.get('generator_version')}")
    texts = {record.uid: (record.text, record.surface) for record in generated}
    for line in NEGATIVE_CORPUS.read_text(encoding="utf-8").splitlines():
        row = json.loads(line)
        texts[row["id"]] = (row["text"], row.get("category") or "")
    return texts, corpus["corpus_sha256"]


def _selected_rows(header: dict, rows: Sequence[dict], config: str) -> list[dict]:
    """The observations of one config, checked complete against its populations.

    A record captured without `--policy` holds several configs; mixing them
    double-counts, and a truncated record would read as fewer false positives.
    """
    card = header["scorecard"]
    runs = [("C", run) for run in card.get("runs", [])]
    runs += [(layer, run) for layer in ("A", "D", "R")
             for run in card.get("layers", {}).get(layer, {}).get("runs", [])]
    expected = set()
    for layer, run in runs:
        if run["config"] == config:
            expected.update((layer, uid) for uid in run["scored_population"]["document_ids"]
                            + run["failed_closed_population"]["document_ids"])
    selected = [row for row in rows if row["config"] == config]
    observed = [(row["layer"], row["document_id"]) for row in selected]
    if not expected or len(set(observed)) != len(observed) or set(observed) != expected:
        raise CensusError(f"record does not hold exactly one complete population for config {config}")
    return selected


def census(record_path: Path, *, config: str | None = None) -> dict:
    header, rows = records._read(record_path)
    config = config or agentic.production_config(header["scorecard"])
    documents = {row["id"]: row for row in header["documents"]}
    texts, corpus_sha256 = _texts(header)
    ledger: list[dict] = []
    exposure: Counter = Counter()
    exposure_documents: Counter = Counter()
    unreadable = Counter()
    unmeasured_exposure = Counter()
    refused = Counter()
    for row in _selected_rows(header, rows, config):
        uid = row["document_id"]
        document = documents.get(uid)
        if document is None or document["layer"] != row["layer"]:
            raise CensusError(f"observation disagrees with document inventory: {uid}")
        response = row["response"]
        if response.get("refused"):
            refused[row["layer"]] += 1
            continue
        gold = [(start, end) for start, end, _ in document["gold"]]
        traces = response["final_protection_trace"]
        if not all(records.has_lineage(t) for t in traces):
            raise CensusError("record has no trace lineage (observation schema v2 is required)")
        if uid not in texts:
            unreadable[row["layer"]] += score.interval_length(score.subtract_intervals(
                score.merge_intervals([(t["raw_start"], t["raw_end"]) for t in traces]),
                score.merge_intervals(gold)))
            unmeasured_exposure[row["layer"]] += learned_only_gold(gold, traces)
            continue
        text, surface = texts[uid]
        raw = text.encode("utf-8")
        if len(raw) != document["utf8_bytes"]:
            raise CensusError(f"rebuilt text length disagrees with the record: {uid}")
        shape = structure(text, log_carrier=bool(_LOG_SURFACE.search(surface)))
        ledger.extend(fragments(uid, row["layer"], raw, gold, traces, shape))
        for name, value in learned_veto_exposure(gold, traces, shape).items():
            if value:
                exposure[(row["layer"], name)] += value
                exposure_documents[(row["layer"], name)] += 1
    return {
        "schema_version": SCHEMA_VERSION,
        "report_only": True,
        "config": config,
        "record_sha256": score.sha256_file(record_path),
        "benchmark_spec_id": header.get("benchmark_spec_id"),
        "generator_corpus_sha256": corpus_sha256,
        "summary": summarize(ledger),
        # Measured on rebuildable documents only; for the others the upper bound
        # is every gold byte that only learned-only traces protect.
        "learned_veto_exposure": [
            {"layer": layer, "veto": name, "gold_bytes": exposure[(layer, name)],
             "documents": exposure_documents[(layer, name)]}
            for layer, name in sorted(exposure)],
        "unmeasured_learned_veto_exposure_upper_bound": dict(sorted(unmeasured_exposure.items())),
        "unreadable_false_positive_bytes": dict(sorted(unreadable.items())),
        "refused_documents": dict(sorted(refused.items())),
        # Value-free: offsets only. The gate needs the critical ones.
        "critical_fragments": sorted((r for r in ledger if r["critical"]),
                                     key=lambda r: (r["document_id"], r["start"])),
    }


def summarize(ledger: Sequence[dict]) -> dict:
    def block(rows: Sequence[dict]) -> dict:
        return {"fragments": len(rows), "documents": len({r["document_id"] for r in rows}),
                "bytes": sum(r["end"] - r["start"] for r in rows)}

    critical = [r for r in ledger if r["critical"]]
    groups: dict[tuple, list] = defaultdict(list)
    for row in critical:
        groups[("+".join(row["sources"]) or "unattributed", row["class"], row["role"])].append(row)
    by_role: dict[str, list] = defaultdict(list)
    for row in ledger:
        by_role[row["role"]].append(row)
    by_layer: dict[str, list] = defaultdict(list)
    for row in critical:
        by_layer[row["layer"]].append(row)
    return {
        "all": block(ledger),
        "critical": block(critical),
        "critical_by_layer": {layer: block(rows) for layer, rows in sorted(by_layer.items())},
        "by_role": {role: block(rows) for role, rows in sorted(by_role.items())},
        "critical_by_source": [
            {"source": source, "class": label, "role": role, **block(rows)}
            for (source, label, role), rows in sorted(groups.items())],
    }


# --------------------------------------------------------------------------
# Gate: no new critical false-positive bytes.


def gate(base: dict, candidate: dict) -> list[str]:
    """F2: a candidate may not add a critical false-positive byte.

    A byte is new when the base had no critical false positive at that
    document offset. Both censuses must read the same generated corpus.
    """
    if base["generator_corpus_sha256"] != candidate["generator_corpus_sha256"]:
        raise CensusError("base and candidate measured different corpora")
    if base.get("benchmark_spec_id") != candidate.get("benchmark_spec_id"):
        raise CensusError("base and candidate use different benchmark specifications")
    if base.get("config") != candidate.get("config"):
        raise CensusError("base and candidate measured different configs")
    known = defaultdict(set)
    for row in base["critical_fragments"]:
        known[row["document_id"]].update(range(row["start"], row["end"]))
    failures = []
    for row in candidate["critical_fragments"]:
        new = [b for b in range(row["start"], row["end"]) if b not in known[row["document_id"]]]
        if new:
            failures.append(f"{row['document_id']} [{new[0]},{new[-1] + 1}) {row['role']} "
                            f"{row['class']} by {'+'.join(row['sources']) or 'unattributed'}")
    return failures


def markdown(value: dict) -> str:
    summary = value["summary"]
    lines = ["| Source | Class | Position | Documents | Fragments | FP bytes |",
             "| --- | --- | --- | ---: | ---: | ---: |"]
    for row in summary["critical_by_source"]:
        lines.append(f"| {row['source']} | {row['class']} | {row['role']} | {row['documents']} "
                     f"| {row['fragments']} | {row['bytes']} |")
    total = summary["critical"]
    lines.append(f"| **total** | | | {total['documents']} | {total['fragments']} | {total['bytes']} |")
    return "\n".join(lines) + "\n"


def _load(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if value.get("schema_version") != SCHEMA_VERSION:
        raise CensusError(f"{path}: unknown census schema")
    return value


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    report_parser = commands.add_parser("report", help="write the census of one record")
    report_parser.add_argument("record", type=Path)
    report_parser.add_argument("--output", type=Path)
    report_parser.add_argument("--config", help="observation config (default: the production config)")
    report_parser.add_argument("--markdown", action="store_true")
    check_parser = commands.add_parser("check", help="recompute a committed census")
    check_parser.add_argument("record", type=Path)
    check_parser.add_argument("--expect", type=Path, required=True)
    gate_parser = commands.add_parser("gate", help="fail on new critical false-positive bytes")
    gate_parser.add_argument("base", type=Path)
    gate_parser.add_argument("candidate", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "report":
            value = census(args.record, config=args.config)
            text = markdown(value) if args.markdown else json.dumps(value, indent=1, sort_keys=True) + "\n"
            if args.output:
                args.output.write_text(text, encoding="utf-8")
            else:
                sys.stdout.write(text)
            return 0
        if args.command == "check":
            if census(args.record) != _load(args.expect):
                print(f"json structure census: {args.expect} is stale", file=sys.stderr)
                return 1
            return 0
        failures = gate(_load(args.base), _load(args.candidate))
        for failure in failures:
            print(f"new critical false positive: {failure}", file=sys.stderr)
        return 1 if failures else 0
    except (CensusError, records.RecordError, OSError, KeyError, ValueError) as error:
        print(f"json structure census: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
