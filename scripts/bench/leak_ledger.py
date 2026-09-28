#!/usr/bin/env python3
"""Leak ledger: every leaked gold byte of one measured commit, by root cause.

`probe` joins a runner's observation record with the candidate pool that the
bench producer dumps under `GAZE_BENCH_CANDIDATE_POOL=1` (the same pipeline, so
each document's final trace must equal the record's, or the probe refuses) and
writes one value-free row per leaked gold span: layer, document ID, label, byte
offsets, leaked bytes, cause and the rule IDs behind it. No raw value is kept.

`check` needs no corpus, model or binary. From the committed record alone it
re-derives which gold spans leak and by how many bytes, requires the ledger to
hold exactly those rows, requires every label's total to equal the scorecard's
`per_label_recall` leaked bytes under every scored-label contract, and fails if
the rendered table drifted. Only the cause column comes from the probe.

Causes come from one ordered table, `STEPS`; the first step that matches wins.
`CAUSES` gives each cause letter its title, the detail fields a row of it must
carry and how the table names its mechanism. `classify`, `validate_rows` and
the renderer all read these two tables.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path
from dataclasses import dataclass
from typing import Any, Callable, Iterable, Mapping, Sequence

import gaze_bench_score as score
import scorecard_record as record
from render_benchmark_doc import begin_marker, end_marker


ROOT = Path(__file__).resolve().parents[2]
BENCH_DIR = ROOT / "docs/reference/benchmarks"
DOC = BENCH_DIR / "README.md"
INDEX = BENCH_DIR / "leak-ledger.json"
EVIDENCE_DIR = BENCH_DIR / "leak-ledger"
BLOCK = "leak-ledger"
SCHEMA_VERSION = 1
HEADLINE_CONTRACT = 3
CONFIG = "policy-file"
LAYERS = ("C", "A", "D", "R")


# Identifier labels: a real value carries at least one digit and more than
# four alphanumerics. A value that does not is junk-shaped (the bare word
# `IBAN`, a lone `7623`). Free-text labels have no such shape and never are.
DIGIT_IDENTIFIER_LABELS = frozenset({
    "CREDITCARDNUMBER", "DRIVERLICENSENUM", "IBAN", "IDCARDNUM", "LICENSEPLATENUM",
    "NATIONALID", "PASSPORTID", "PHONENUMBER", "SSN", "TAXNUM",
})
CLOSEST_WINDOW = 16


class LedgerError(ValueError):
    pass


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _write_rows(path: Path, rows: Iterable[Mapping[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0, compresslevel=9) as out:
            for row in rows:
                out.write(json.dumps(row, sort_keys=True, separators=(",", ":")).encode() + b"\n")


def _read_rows(path: Path) -> list[dict[str, Any]]:
    with gzip.open(path, "rt", encoding="utf-8") as stream:
        return [json.loads(line) for line in stream]


# --------------------------------------------------------------------------
# leaked spans, value-free (shared by probe and check)
# --------------------------------------------------------------------------


def leaked_spans(path: Path) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    """Every gold span (all labels, every layer) with at least one raw byte.

    Coverage is the scorer's: the merged raw spans of the whole final trace,
    measured per gold span, which is what `per_label_recall` sums.
    """
    header, observations = record._read(path)
    inventory = {row["id"]: row for row in header["documents"]}
    rows: list[dict[str, Any]] = []
    refused = []
    for observation in observations:
        if observation["config"] != CONFIG:
            raise LedgerError(f"record config {observation['config']!r} is not {CONFIG}")
        response = observation["response"]
        document = inventory[observation["document_id"]]
        if response.get("refused"):
            refused.append(document["id"])
            continue
        predicted = score.merge_intervals(
            (item["raw_start"], item["raw_end"]) for item in response["final_protection_trace"]
        )
        for start, end, label in document["gold"]:
            covered = score.intersection_length([(start, end)], predicted)
            if covered < end - start:
                rows.append({
                    "layer": observation["layer"], "document_id": document["id"],
                    "label": label, "start": start, "end": end,
                    "leaked": end - start - covered, "covered": covered,
                })
    rows.sort(key=_row_key)
    return {"header": header, "refused": sorted(refused)}, rows


def _row_key(row: Mapping[str, Any]) -> tuple:
    return (LAYERS.index(row["layer"]), row["document_id"], row["start"], row["end"], row["label"])


# --------------------------------------------------------------------------
# probe: classify with the candidate pool
# --------------------------------------------------------------------------


def _overlaps(item: Mapping[str, Any], start: int, end: int) -> bool:
    return item["raw_start"] < end and start < item["raw_end"]


def _overlap_length(item: Mapping[str, Any], start: int, end: int) -> int:
    return max(0, min(item["raw_end"], end) - max(item["raw_start"], start))


def junk_shape(label: str, value: bytes) -> str | None:
    text = value.decode("utf-8", errors="replace")
    alnum = [ch for ch in text if ch.isalnum()]
    if not alnum:
        return "no_alphanumeric"
    if label in DIGIT_IDENTIFIER_LABELS:
        if not any(ch.isdigit() for ch in alnum):
            return "identifier_without_digit"
        if len(alnum) <= 4:
            return "identifier_four_or_fewer_alphanumerics"
    return None


def _pool_ref(item: Mapping[str, Any]) -> dict[str, Any]:
    return {"recognizer": item["recognizer_id"], "class": item["class"]}


def _best(items: Sequence[Mapping[str, Any]], start: int, end: int, compatible) -> Mapping[str, Any]:
    return max(items, key=lambda item: (
        compatible(item["class"]), _overlap_length(item, start, end),
        -(item["raw_end"] - item["raw_start"]), item["recognizer_id"], item["raw_start"],
    ))


@dataclass(frozen=True)
class Span:
    """One leaked gold span with everything a step may read."""

    row: Mapping[str, Any]
    text: bytes
    trace: Sequence[Mapping[str, Any]]
    pool: Mapping[str, Sequence[Mapping[str, Any]]]
    compatible_pairs: frozenset[tuple[str, str]]
    losers: Sequence[tuple[str, str]]

    @property
    def start(self) -> int:
        return self.row["start"]

    @property
    def end(self) -> int:
        return self.row["end"]

    def compatible(self, predicted: str) -> bool:
        return (predicted, self.row["label"]) in self.compatible_pairs

    def overlapping(self, part: str) -> list[Mapping[str, Any]]:
        return [item for item in self.pool[part] if _overlaps(item, self.start, self.end)]

    def best(self, items: Sequence[Mapping[str, Any]]) -> Mapping[str, Any]:
        return _best(items, self.start, self.end, self.compatible)

    def tier(self, recognizer: str) -> str:
        # Audit loser rows name the tier but carry no span; a recognizer that
        # lost twice in one document under different tiers is reported as both.
        tiers = sorted({
            tier for name, tier in self.losers if name == recognizer and tier != "ValidatorVeto"
        })
        return "+".join(tiers) if tiers else "unlogged"

    def resolved(self, item: Mapping[str, Any]) -> bool:
        key = (item["raw_start"], item["raw_end"], item["recognizer_id"])
        return any(
            (other["raw_start"], other["raw_end"], other["recognizer_id"]) == key
            for other in self.pool["resolved"]
        )


def _partial(span: Span) -> dict[str, Any] | None:
    """d: a token covers part of the value. The sub-kind says whether a candidate
    that would have covered more lost resolution, was vetoed, won primary
    resolution yet left the trace, or never existed."""
    covered = span.row["covered"]
    if not covered:
        return None
    by = sorted({
        (item["class"], "+".join(item["provenance"]["source_ids"]), item["provenance"]["stage"])
        for item in span.trace if _overlaps(item, span.start, span.end)
    })
    fuller = {
        part: [item for item in span.overlapping(part)
               if _overlap_length(item, span.start, span.end) > covered]
        for part in ("detected", "vetoed")
    }
    # A vetoed candidate is missing from the resolved winners too; it did not lose resolution.
    vetoed = {(item["raw_start"], item["raw_end"], item["recognizer_id"]) for item in span.pool["vetoed"]}
    lost = [
        item for item in fuller["detected"]
        if not span.resolved(item)
        and (item["raw_start"], item["raw_end"], item["recognizer_id"]) not in vetoed
    ]
    emitted = [item for item in fuller["detected"] if span.resolved(item)]
    if lost:
        kind = "lost_resolution"
    elif fuller["vetoed"]:
        kind = "vetoed"
    elif emitted:
        kind = "resolved_not_emitted"
    else:
        kind = "no_full_candidate"
    losing = sorted({
        (item["recognizer_id"], item["class"], span.tier(item["recognizer_id"])) for item in lost
    } | {
        (item["recognizer_id"], item["class"], "ValidatorVeto") for item in fuller["vetoed"]
    })
    return {
        "kind": kind,
        "by": [{"class": c, "sources": s, "stage": st} for c, s, st in by],
        "lost": [{"recognizer": r, "class": c, "tier": t} for r, c, t in losing],
    }


def _validator_veto(span: Span) -> dict[str, Any] | None:
    vetoed = span.overlapping("vetoed")
    if not vetoed:
        return None
    item = span.best(vetoed)
    return {"kind": "validator", **_pool_ref(item), "reason": item.get("reason")}


def _loser(span: Span) -> Mapping[str, Any] | None:
    detected = span.overlapping("detected")
    return span.best(detected) if detected else None


def _winner_not_emitted(span: Span) -> dict[str, Any] | None:
    loser = _loser(span)
    if loser is None or not span.resolved(loser):
        return None
    return {"note": "primary_winner_not_in_final_trace", **_pool_ref(loser)}


def _lost_to_winner(span: Span) -> dict[str, Any] | None:
    loser = _loser(span)
    if loser is None:
        return None
    winners = [
        item for item in span.pool["resolved"]
        if _overlaps(item, loser["raw_start"], loser["raw_end"])
    ]
    if not winners:
        return None
    winner = _best(winners, loser["raw_start"], loser["raw_end"], lambda _: False)
    return {
        "loser": loser["recognizer_id"], "loser_class": loser["class"],
        "winner": winner["recognizer_id"], "winner_class": winner["class"],
        "tier": span.tier(loser["recognizer_id"]),
    }


def _pre_resolution_drop(span: Span) -> dict[str, Any] | None:
    loser = _loser(span)
    if loser is None:
        return None
    return {"kind": "pre_resolution_drop", **_pool_ref(loser), "reason": None}


def _locale_gated(span: Span) -> dict[str, Any] | None:
    gated = span.overlapping("locale_gated")
    if not gated:
        return None
    return {"kind": "locale_gate", **_pool_ref(span.best(gated)), "reason": None}


def _repeat(span: Span) -> dict[str, Any] | None:
    return _protected_copy(span.text[span.start:span.end], span.start, span.text, span.trace)


def _junk(span: Span) -> dict[str, Any] | None:
    if junk_shape(span.row["label"], span.text[span.start:span.end]) is None:
        return None
    return {"closest": _closest(span.start, span.end, span.trace, span.pool, span.compatible)}


def _nothing(span: Span) -> dict[str, Any]:
    return {"closest": _closest(span.start, span.end, span.trace, span.pool, span.compatible)}


@dataclass(frozen=True)
class Step:
    letter: str
    predicate: Callable[[Span], dict[str, Any] | None]


# First match wins. `_nothing` always matches, so every span gets a cause.
STEPS = (
    Step("d", _partial),
    Step("b", _validator_veto),
    Step("g", _winner_not_emitted),
    Step("c", _lost_to_winner),
    Step("b", _pre_resolution_drop),
    Step("b", _locale_gated),
    Step("e", _repeat),
    Step("f", _junk),
    Step("a", _nothing),
)


def _closest_name(detail: Mapping[str, Any]) -> str:
    closest = detail["closest"]
    if closest is None:
        return "nothing near"
    return f"closest `{closest['class']}`" + (" (compatible)" if closest["compatible"] else "")


def _partial_name(detail: Mapping[str, Any]) -> str:
    covered = ", ".join(sorted({item["class"] for item in detail["by"]}))
    if detail["kind"] == "no_full_candidate":
        return f"covered by {covered}; no fuller candidate"
    losing = ", ".join(sorted({f"`{item['recognizer']}`" for item in detail["lost"]}))
    return f"{detail['kind']}: {losing or 'resolved winner'} over {covered}"


@dataclass(frozen=True)
class Cause:
    title: str
    fields: frozenset[str]
    name: Callable[[Mapping[str, Any]], str]
    kinds: tuple[str, ...] = ()


CAUSES = {
    "a": Cause("no candidate", frozenset({"closest"}), _closest_name),
    "b": Cause("vetoed", frozenset({"kind", "recognizer", "class", "reason"}),
               lambda d: f"{d['kind']} `{d['recognizer']}`",
               ("validator", "pre_resolution_drop", "locale_gate")),
    "c": Cause("lost in resolution",
               frozenset({"loser", "loser_class", "winner", "winner_class", "tier"}),
               lambda d: f"`{d['loser']}` lost to `{d['winner_class']}` ({d['tier']})"),
    "d": Cause("partial span", frozenset({"kind", "by", "lost"}), _partial_name,
               ("lost_resolution", "vetoed", "resolved_not_emitted", "no_full_candidate")),
    "e": Cause("repeat not swept", frozenset({"copy_classes", "copy_stages", "copy_sources"}),
               lambda d: "copy protected by " + ", ".join(d["copy_stages"])),
    "f": Cause("junk-shaped gold", frozenset({"closest"}), _closest_name),
    "g": Cause("other", frozenset({"note", "recognizer", "class"}), lambda d: d["note"]),
}
ROW_FIELDS = frozenset({
    "layer", "document_id", "label", "start", "end", "leaked", "covered",
    "cause", "detail", "junk_shape",
})


def classify(
    row: Mapping[str, Any], text: bytes, trace: Sequence[Mapping[str, Any]],
    pool: Mapping[str, Sequence[Mapping[str, Any]]], compatible_pairs: frozenset[tuple[str, str]],
    losers: Sequence[tuple[str, str]] = (),
) -> dict[str, Any]:
    span = Span(row, text, trace, pool, compatible_pairs, losers)
    junk = junk_shape(row["label"], text[row["start"]:row["end"]])
    for step in STEPS:
        detail = step.predicate(span)
        if detail is not None:
            return {**row, "cause": step.letter, "detail": detail, "junk_shape": junk}
    raise AssertionError("the last step always matches")


def _protected_copy(
    value: bytes, start: int, text: bytes, trace: Sequence[Mapping[str, Any]]
) -> dict[str, Any] | None:
    predicted = score.merge_intervals((item["raw_start"], item["raw_end"]) for item in trace)
    offset = text.find(value)
    while offset >= 0 and value:
        end = offset + len(value)
        if (offset != start and score.interval_is_covered((offset, end), predicted)
                and score.gold_gap_on_word_boundary(text, offset, end)):
            items = [item for item in trace if _overlaps(item, offset, end)]
            return {
                "copy_classes": sorted({item["class"] for item in items}),
                "copy_stages": sorted({item["provenance"]["stage"] for item in items}),
                "copy_sources": sorted({"+".join(item["provenance"]["source_ids"]) for item in items}),
            }
        offset = text.find(value, offset + 1)
    return None


def _closest(start, end, trace, pool, compatible) -> dict[str, Any] | None:
    options = []
    for source, items in (("pool", [*pool["detected"], *pool["locale_gated"], *pool["vetoed"]]),
                          ("trace", [{"recognizer_id": "+".join(item["provenance"]["source_ids"]),
                                      "class": item["class"], "raw_start": item["raw_start"],
                                      "raw_end": item["raw_end"]} for item in trace])):
        for item in items:
            gap = max(item["raw_start"] - end, start - item["raw_end"], 0)
            if gap <= CLOSEST_WINDOW:
                options.append((not compatible(item["class"]), gap, item["recognizer_id"],
                                item["class"], source))
    if not options:
        return None
    incompatible, gap, recognizer, predicted, source = min(options)
    return {"recognizer": recognizer, "class": predicted, "gap": gap,
            "compatible": not incompatible, "from": source}


AUDIT_LOSER = re.compile(
    r'recognizer_id: Some\("(?P<recognizer>[^"]*)"\).*?conflict_loser: true, decided_by: (?P<tier>\w+)'
)


def audit_losers(rows: Sequence[str]) -> list[tuple[str, str]]:
    """(recognizer ID, conflict tier) of every loser row the pipeline logged."""
    return [
        (match["recognizer"], match["tier"])
        for match in map(AUDIT_LOSER.search, rows) if match is not None
    ]


def _documents_with_text(repo_root: Path, dataset: Path) -> dict[str, score.Document]:
    import agentic_layers as agentic
    import dataiku_en_de_gaze_bench as dataiku
    import run_no_opf_benchmark as runner

    dataiku.verify_dataset(dataset)
    positives, _ = dataiku.load_documents(dataset)
    negatives, _ = runner.load_negative_documents(repo_root / runner.NEGATIVE_CORPUS)
    generated = [item.to_document() for item in agentic.generate(agentic.PUBLISHED_PARTITION)]
    documents: dict[str, score.Document] = {}
    for document in [*positives, *negatives, *generated]:
        if document.uid in documents:
            raise LedgerError(f"document ID {document.uid} appears twice")
        documents[document.uid] = document
    return documents


def _trace_identity(trace: Sequence[Mapping[str, Any]]) -> list[tuple]:
    return [
        (item["raw_start"], item["raw_end"], item["class"], item["action"],
         item["provenance"]["stage"], item["provenance"]["decision"],
         tuple(item["provenance"]["source_ids"]))
        for item in trace
    ]


def probe(
    *, record_path: Path, binary: Path, policy: Path, dataset: Path, model_dir: Path,
    machine: str, threshold: float = 0.3, main_revision: str | None = None,
    main_binary: Path | None = None, record_binary_sha256: str | None = None,
) -> dict[str, Any]:
    import run_no_opf_benchmark as runner

    meta, rows = leaked_spans(record_path)
    header = meta["header"]
    card = header["scorecard"]
    if card["parameters"]["policy_sha256"] != sha256(policy):
        raise LedgerError("policy file differs from the record's policy")
    inventory = {row["id"]: row for row in header["documents"]}
    documents = _documents_with_text(ROOT, dataset)
    for uid, row in inventory.items():
        document = documents.get(uid)
        if document is None:
            raise LedgerError(f"{uid}: in the record but not in the corpus")
        text = document.text.encode("utf-8")
        gold = sorted([s.start, s.end, s.label] for s in (*document.spans, *document.excluded_spans))
        if len(text) != row["utf8_bytes"] or gold != sorted(row["gold"]):
            raise LedgerError(f"{uid}: corpus text or gold differs from the record")
    _, observations = record._read(record_path)
    traces = {
        (item["layer"], item["document_id"]): item["response"]
        for item in observations
    }
    compatible = score.load_scored_label_contract(
        BENCH_DIR / "scored-labels-v3.json"
    ).gold_gap.compatible
    environment = runner.build_no_opf_environment(os.environ)
    environment.update({
        "GAZE_BENCH_POLICY": str(policy), "GAZE_NER_MODEL_DIR": str(model_dir),
        "GAZE_NER_THRESHOLD": str(threshold), "GAZE_BENCH_CANDIDATE_POOL": "1",
        "GAZE_BENCH_AUDIT_ROWS": "1",
    })
    wanted = defaultdict(list)
    for row in rows:
        wanted[(row["layer"], row["document_id"])].append(row)
    classified: list[dict[str, Any]] = []
    checked = 0
    process = subprocess.Popen(
        [str(binary), "--config", CONFIG], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        env=environment, cwd=ROOT, text=True, encoding="utf-8",
    )
    try:
        for (layer, uid), response in sorted(traces.items(), key=lambda item: (LAYERS.index(item[0][0]), item[0][1])):
            if response.get("refused"):
                continue
            document = documents[uid]
            process.stdin.write(json.dumps({
                "fixture_id": uid, "locale_chain": document.locale_chain, "text": document.text,
            }) + "\n")
            process.stdin.flush()
            line = process.stdout.readline()
            if not line:
                raise LedgerError(f"{uid}: producer exited")
            probed = json.loads(line)
            if "candidate_pool" not in probed:
                raise LedgerError(f"{uid}: producer returned no candidate pool (old binary?)")
            if _trace_identity(probed["final_protection_trace"]) != _trace_identity(
                response["final_protection_trace"]
            ):
                raise LedgerError(f"{uid}: probe trace differs from the record; not the same build")
            checked += 1
            text = document.text.encode("utf-8")
            for row in wanted.get((layer, uid), ()):
                classified.append(classify(
                    row, text, probed["final_protection_trace"], probed["candidate_pool"], compatible,
                    audit_losers(probed["audit_rows"]),
                ))
            if checked % 500 == 0:
                print(f"leak_ledger: probed {checked}/{len(traces)}", file=sys.stderr)
    finally:
        process.stdin.close()
        process.wait()
    if len(classified) != len(rows):
        raise LedgerError("classified rows differ from leaked spans")
    revision = card["gaze"]["revision"]
    if card["gaze"]["dirty"]:
        raise LedgerError("the record was measured on a dirty tree")
    reuse = None
    if main_revision is not None and main_revision != revision:
        # A record from another commit stands for main only when main's own release
        # producer is byte-identical to the one that measured the record.
        if main_binary is None or record_binary_sha256 is None:
            raise LedgerError("--main-revision needs --main-binary and --record-binary-sha256")
        main_binary_sha256 = sha256(main_binary)
        if main_binary_sha256 != record_binary_sha256:
            raise LedgerError(f"{main_revision}'s producer differs from the record's; run a fresh record")
        reuse = {"record_revision": revision, "producer_sha256": main_binary_sha256}
    stem = revision[:12]
    EVIDENCE_DIR.mkdir(parents=True, exist_ok=True)
    record_target = EVIDENCE_DIR / f"observations-{stem}.jsonl.gz"
    rows_target = EVIDENCE_DIR / f"ledger-{stem}.jsonl.gz"
    shutil.copyfile(record_path, record_target)
    _write_rows(rows_target, classified)
    index = {
        "schema_version": SCHEMA_VERSION,
        "comment": "Leak ledger: every leaked gold byte of one commit by root cause. "
                   "Written by scripts/bench/leak_ledger.py probe; checked by its check command.",
        "revision": revision,
        "main_revision": main_revision or revision,
        "reused_record": reuse,
        "corpus_sha256": header["corpus_sha256"],
        "policy_sha256": card["parameters"]["policy_sha256"],
        "sampling_seed": card["parameters"]["sampling_seed"],
        "record": {"file": record_target.relative_to(ROOT).as_posix(), "sha256": sha256(record_target)},
        "rows": {"file": rows_target.relative_to(ROOT).as_posix(), "sha256": sha256(rows_target)},
        "attested": {
            "probe_binary_sha256": sha256(binary), "machine": machine,
            "documents_trace_checked": checked,
            "note": "The probe binary is the bench producer with the candidate-pool dump; "
                    "every document's final trace equalled the record's.",
        },
    }
    INDEX.write_text(json.dumps(index, indent=2) + "\n", encoding="utf-8")
    return index


# --------------------------------------------------------------------------
# check and render
# --------------------------------------------------------------------------


def _contracts() -> dict[int, score.ScoredLabelContract]:
    import mechanism_arms as arms
    return {version: arms._contract(version) for version in arms.required_contracts()}


def _layer_contract() -> score.ScoredLabelContract:
    import agentic_layers as agentic
    return agentic.load_contract(ROOT)


def _scored(label: str, contract: score.ScoredLabelContract) -> bool:
    if label in contract.excluded_labels:
        return False
    return contract.scored_labels is None or label in contract.scored_labels


def validate_rows(rows: Sequence[Mapping[str, Any]], expected: Sequence[Mapping[str, Any]]) -> None:
    """The rows are exactly the record's leaked spans, and each one is a
    well-formed row of its cause: `CAUSES` fixes the detail fields and kinds, so
    relabelling a row's cause without its evidence fails here, for every label."""
    for row in rows:
        if set(row) != ROW_FIELDS:
            raise LedgerError(f"row fields {sorted(set(row) ^ ROW_FIELDS)} differ from the schema")
    keys = ("layer", "document_id", "label", "start", "end", "leaked", "covered")
    if [tuple(row[key] for key in keys) for row in rows] != [
        tuple(row[key] for key in keys) for row in expected
    ]:
        raise LedgerError("ledger rows differ from the leaked spans the record re-derives")
    for row in rows:
        where = f"{row['layer']}/{row['document_id']}:{row['start']}-{row['end']}"
        cause = CAUSES.get(row["cause"])
        if cause is None:
            raise LedgerError(f"{where}: unknown cause {row['cause']!r}")
        detail = row["detail"]
        if not isinstance(detail, Mapping) or set(detail) != cause.fields:
            raise LedgerError(f"{where}: detail does not match cause {row['cause']!r}")
        if cause.kinds and detail["kind"] not in cause.kinds:
            raise LedgerError(f"{where}: {detail['kind']!r} is no kind of cause {row['cause']!r}")
        if row["cause"] == "d":
            lost = detail["lost"]
            kind = detail["kind"]
            if (not isinstance(lost, list)
                    or (kind in ("lost_resolution", "vetoed") and not lost)
                    or (kind == "no_full_candidate" and lost)):
                raise LedgerError(f"{where}: partial-span lost evidence disagrees with {kind!r}")
        if (row["cause"] == "d") != (row["covered"] > 0):
            raise LedgerError(f"{where}: partial-span cause disagrees with coverage")
        if row["cause"] == "f" and row["junk_shape"] is None:
            raise LedgerError(f"{where}: junk cause without a junk shape")


def reconcile(
    rows: Sequence[Mapping[str, Any]], record_path: Path,
    contracts: Mapping[int, score.ScoredLabelContract],
    layer_contract: score.ScoredLabelContract,
) -> dict[str, Any]:
    """Every label's ledger bytes equal the scorecard's, per contract and layer."""
    result: dict[str, Any] = {}
    for version, contract in sorted(contracts.items()):
        card = record.rescore(record_path, contract, layer_contract)
        views = {"C": (contract, card["runs"])}
        if "layers" in card:
            for layer in ("A", "D", "R"):
                views[layer] = (layer_contract, card["layers"][layer]["runs"])
        per_version = {}
        for layer, (selected, runs) in views.items():
            (run,) = [item for item in runs if item["config"] == CONFIG]
            expected = {
                label: block["leaked_utf8_bytes"]
                for label, block in run["per_label_recall"].items()
                if block["leaked_utf8_bytes"]
            }
            ledger = Counter()
            for row in rows:
                if row["layer"] == layer and _scored(row["label"], selected):
                    ledger[row["label"]] += row["leaked"]
            if dict(ledger) != expected:
                diff = {label: (ledger.get(label, 0), expected.get(label, 0))
                        for label in sorted(set(ledger) | set(expected))
                        if ledger.get(label, 0) != expected.get(label, 0)}
                raise LedgerError(f"v{version} layer {layer}: ledger vs scorecard {diff}")
            per_version[layer] = {
                "labels": sum(expected.values()),
                "headline_leaked": run["metrics"]["utf8_bytes"]["leaked"],
            }
        result[str(version)] = per_version
    return result


def _fmt(value: int) -> str:
    return f"{value:,}" if value else "·"


def render(rows: Sequence[Mapping[str, Any]], index: Mapping[str, Any],
           totals: Mapping[str, Any], contract: score.ScoredLabelContract,
           layer_contract: score.ScoredLabelContract) -> str:
    scored_c = [row for row in rows if row["layer"] == "C" and _scored(row["label"], contract)]
    letters = list(CAUSES)
    lines = [
        f"Main `{index['main_revision'][:12]}`"
        + (f" (record measured on `{index['revision'][:12]}` with a byte-identical release "
           f"producer `{index['reused_record']['producer_sha256'][:12]}`)"
           if index["reused_record"] else "")
        + f", `gaze setup` policy `{index['policy_sha256'][:12]}`, "
        f"scored-label contract v{HEADLINE_CONTRACT}. Leaked bytes per gold span, summed per "
        "label (the scorecard's `per_label_recall`); overlapping gold counts once per span, so "
        f"the label sum ({totals[str(HEADLINE_CONTRACT)]['C']['labels']:,} B) can exceed the "
        f"headline leaked bytes ({totals[str(HEADLINE_CONTRACT)]['C']['headline_leaked']:,} B).",
        "",
        "Layer C by label and cause (bytes):",
        "",
        "| Label | " + " | ".join(f"{letter} {CAUSES[letter].title}" for letter in letters)
        + " | Total |",
        "| --- | " + " | ".join("---:" for _ in letters) + " | ---: |",
    ]
    by_label: dict[str, Counter] = defaultdict(Counter)
    for row in scored_c:
        by_label[row["label"]][row["cause"]] += row["leaked"]
    for label in sorted(by_label, key=lambda item: (-sum(by_label[item].values()), item)):
        counts = by_label[label]
        lines.append(f"| {label} | " + " | ".join(_fmt(counts[letter]) for letter in letters)
                     + f" | {sum(counts.values()):,} |")
    total = Counter()
    for counts in by_label.values():
        total.update(counts)
    lines.append("| **all** | " + " | ".join(f"**{_fmt(total[letter])}**" for letter in letters)
                 + f" | **{sum(total.values()):,}** |")
    lines += ["", "Agentic layers by cause (bytes, agentic layer contract):", "",
              "| Layer | " + " | ".join(letters) + " | Total |",
              "| --- | " + " | ".join("---:" for _ in letters) + " | ---: |"]
    for layer in ("A", "D", "R"):
        counts = Counter()
        for row in rows:
            if row["layer"] == layer and _scored(row["label"], layer_contract):
                counts[row["cause"]] += row["leaked"]
        lines.append(f"| {layer} | " + " | ".join(_fmt(counts[letter]) for letter in letters)
                     + f" | {sum(counts.values()):,} |")
    kinds = CAUSES["d"].kinds
    lines += ["", "Partial spans (d) by what a fuller candidate met (bytes):", "",
              "| Layer | " + " | ".join(kinds) + " | Total |",
              "| --- | " + " | ".join("---:" for _ in kinds) + " | ---: |"]
    for layer, selected in (("C", contract), ("A", layer_contract), ("R", layer_contract)):
        counts = Counter()
        for row in rows:
            if row["layer"] == layer and row["cause"] == "d" and _scored(row["label"], selected):
                counts[row["detail"]["kind"]] += row["leaked"]
        lines.append(f"| {layer} | " + " | ".join(_fmt(counts[kind]) for kind in kinds)
                     + f" | {sum(counts.values()):,} |")
    outside = [row for row in rows if row["layer"] == "C" and not _scored(row["label"], contract)]
    if outside:
        lines += ["", f"Layer C labels outside contract v{HEADLINE_CONTRACT} (scored by v1 only):",
                  "", "| Label | " + " | ".join(letters) + " | Total |",
                  "| --- | " + " | ".join("---:" for _ in letters) + " | ---: |"]
        outside_by_label: dict[str, Counter] = defaultdict(Counter)
        for row in outside:
            outside_by_label[row["label"]][row["cause"]] += row["leaked"]
        for label in sorted(outside_by_label):
            counts = outside_by_label[label]
            lines.append(f"| {label} | " + " | ".join(_fmt(counts[letter]) for letter in letters)
                         + f" | {sum(counts.values()):,} |")
    lines += ["", "Top 10 layer C clusters (label, cause, mechanism):", "",
              "| Label | Cause | Mechanism | Spans | Bytes |", "| --- | --- | --- | ---: | ---: |"]
    for (label, letter, key), (spans, count) in clusters(scored_c)[:10]:
        lines.append(f"| {label} | {letter} {CAUSES[letter].title} | {key} | {spans} | {count:,} |")
    junk = [row for row in scored_c if row["junk_shape"] is not None]
    lines += ["", f"Junk-shaped gold in layer C: {len(junk)} spans, "
              f"{sum(row['leaked'] for row in junk):,} leaked bytes "
              f"({sum(row['leaked'] for row in junk if row['cause'] == 'f'):,} B with no candidate, "
              "cause f). They stay in the target until an audited contract change."]
    return "\n".join(lines)


def mechanism(row: Mapping[str, Any]) -> str:
    return CAUSES[row["cause"]].name(row["detail"])


def clusters(rows: Sequence[Mapping[str, Any]]) -> list:
    grouped: dict[tuple[str, str, str], list[int]] = defaultdict(lambda: [0, 0])
    for row in rows:
        entry = grouped[(row["label"], row["cause"], mechanism(row))]
        entry[0] += 1
        entry[1] += row["leaked"]
    return sorted(grouped.items(), key=lambda item: (-item[1][1], item[0]))


def apply(document: str, body: str) -> str:
    begin, end = begin_marker(BLOCK), end_marker(BLOCK)
    start, stop = document.find(begin), document.find(end)
    if start < 0 or stop < start:
        raise LedgerError(f"document is missing the {BLOCK!r} generated block")
    return document[: start + len(begin)] + "\n\n" + body + "\n\n" + document[stop:]


def load(root: Path = ROOT) -> tuple[dict[str, Any], Path, list[dict[str, Any]]]:
    index = json.loads((root / INDEX.relative_to(ROOT)).read_text(encoding="utf-8"))
    if index.get("schema_version") != SCHEMA_VERSION:
        raise LedgerError("unknown leak-ledger schema")
    for key in ("record", "rows"):
        path = root / index[key]["file"]
        if sha256(path) != index[key]["sha256"]:
            raise LedgerError(f"{path.name} does not match its pinned SHA-256")
    record_path = root / index["record"]["file"]
    rows = _read_rows(root / index["rows"]["file"])
    header, _ = record._read(record_path)
    if header["scorecard"]["gaze"]["revision"] != index["revision"]:
        raise LedgerError("record revision differs from the ledger")
    return index, record_path, rows


def derive(root: Path = ROOT) -> tuple[dict[str, Any], str]:
    index, record_path, rows = load(root)
    _, expected = leaked_spans(record_path)
    validate_rows(rows, expected)
    contracts = _contracts()
    layer_contract = _layer_contract()
    totals = reconcile(rows, record_path, contracts, layer_contract)
    body = render(rows, index, totals, contracts[HEADLINE_CONTRACT], layer_contract)
    return totals, body


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    add = commands.add_parser("probe", help="classify a record's leaked spans with the candidate pool")
    add.add_argument("--record", type=Path, required=True,
                     help="runner observations-v1.jsonl.gz (setup policy, clean tree)")
    add.add_argument("--binary", type=Path, required=True,
                     help="clean_for_bench built from the same commit plus the pool dump")
    add.add_argument("--policy", type=Path, required=True)
    add.add_argument("--dataset", type=Path,
                     default=ROOT / "target/bench-data/dataiku-en-de/test.parquet")
    add.add_argument("--model-dir", type=Path,
                     default=Path("~/.local/share/gaze/models/davlan-mbert-ner-hrl").expanduser())
    add.add_argument("--machine", required=True)
    add.add_argument("--main-revision",
                     help="the main commit this ledger stands for, when the record was measured "
                          "on another commit whose release producer is byte-identical")
    add.add_argument("--main-binary", type=Path,
                     help="release clean_for_bench built from --main-revision, unmodified")
    add.add_argument("--record-binary-sha256",
                     help="SHA-256 of the release producer that measured the record")
    commands.add_parser("check", help="fail if the ledger, its totals or its table drifted")
    commands.add_parser("render", help="rewrite the table from the committed ledger")
    args = parser.parse_args(argv)
    try:
        if args.command == "probe":
            probe(record_path=args.record, binary=args.binary, policy=args.policy,
                  dataset=args.dataset, model_dir=args.model_dir, machine=args.machine,
                  main_revision=args.main_revision, main_binary=args.main_binary,
                  record_binary_sha256=args.record_binary_sha256)
        totals, body = derive()
        original = DOC.read_text(encoding="utf-8")
        rendered = apply(original, body)
        if args.command == "check":
            if rendered != original:
                raise LedgerError("leak-ledger table differs from the committed ledger")
            print("leak ledger matches its record under contracts "
                  + ", ".join(f"v{version}" for version in sorted(totals)))
        else:
            DOC.write_text(rendered, encoding="utf-8")
            print(f"wrote {DOC.relative_to(ROOT)} leak-ledger table")
        return 0
    except (LedgerError, record.RecordError, KeyError, OSError, json.JSONDecodeError) as error:
        parser.exit(2, f"leak_ledger: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
