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

Causes, first match wins, in this order:

  d  partial span: a token covers part of the value; the rest stays raw
  b  vetoed: a validator vetoed an overlapping candidate
  c  lost in resolution: an overlapping candidate lost to another winner
  b  dropped before resolution (locale claim) or locale-gated recognizer
  e  repeat copy not swept: the same value is protected elsewhere in the document
  f  junk-shaped gold with no candidate at all (a candidate for an audited
     contract change, never excluded here)
  a  no candidate from any recognizer, with the closest recognizer noted
  g  other: a primary-resolution winner covers it, yet the final trace does not
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
from typing import Any, Iterable, Mapping, Sequence

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

CAUSES = {
    "a": "no candidate",
    "b": "vetoed",
    "c": "lost in resolution",
    "d": "partial span",
    "e": "repeat not swept",
    "f": "junk-shaped gold",
    "g": "other",
}
B_KINDS = ("validator", "pre_resolution_drop", "locale_gate")

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


def classify(
    row: Mapping[str, Any], text: bytes, trace: Sequence[Mapping[str, Any]],
    pool: Mapping[str, Sequence[Mapping[str, Any]]], compatible_pairs: frozenset[tuple[str, str]],
    losers: Sequence[tuple[str, str]] = (),
) -> dict[str, Any]:
    start, end, label = row["start"], row["end"], row["label"]
    value = text[start:end]
    junk = junk_shape(label, value)

    def compatible(predicted: str) -> bool:
        return (predicted, label) in compatible_pairs

    def cause(letter: str, **detail: Any) -> dict[str, Any]:
        return {**row, "cause": letter, "detail": detail, "junk_shape": junk}

    if row["covered"]:
        by = sorted({
            (item["class"], "+".join(item["provenance"]["source_ids"]), item["provenance"]["stage"])
            for item in trace if _overlaps(item, start, end)
        })
        return cause("d", by=[{"class": c, "sources": s, "stage": st} for c, s, st in by])
    vetoed = [item for item in pool["vetoed"] if _overlaps(item, start, end)]
    if vetoed:
        item = _best(vetoed, start, end, compatible)
        return cause("b", kind="validator", **_pool_ref(item), reason=item.get("reason"))
    detected = [item for item in pool["detected"] if _overlaps(item, start, end)]
    if detected:
        loser = _best(detected, start, end, compatible)
        same = [
            item for item in pool["resolved"]
            if (item["raw_start"], item["raw_end"], item["recognizer_id"])
            == (loser["raw_start"], loser["raw_end"], loser["recognizer_id"])
        ]
        if same:
            return cause("g", note="primary_winner_not_in_final_trace", **_pool_ref(loser))
        winners = [
            item for item in pool["resolved"]
            if _overlaps(item, loser["raw_start"], loser["raw_end"])
        ]
        if winners:
            winner = _best(winners, loser["raw_start"], loser["raw_end"], lambda _: False)
            # Audit loser rows name the tier but carry no span; a recognizer that
            # lost twice in one document under different tiers is reported as both.
            tiers = sorted({
                tier for recognizer, tier in losers
                if recognizer == loser["recognizer_id"] and tier != "ValidatorVeto"
            })
            return cause(
                "c", loser=loser["recognizer_id"], loser_class=loser["class"],
                winner=winner["recognizer_id"], winner_class=winner["class"],
                tier="+".join(tiers) if tiers else "unlogged",
            )
        return cause("b", kind="pre_resolution_drop", **_pool_ref(loser), reason=None)
    gated = [item for item in pool["locale_gated"] if _overlaps(item, start, end)]
    if gated:
        item = _best(gated, start, end, compatible)
        return cause("b", kind="locale_gate", **_pool_ref(item), reason=None)
    copy = _protected_copy(value, start, text, trace)
    if copy is not None:
        return cause("e", **copy)
    if junk is not None:
        return cause("f", closest=_closest(start, end, trace, pool, compatible))
    return cause("a", closest=_closest(start, end, trace, pool, compatible))


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
    crates_tree = _crates_tree(revision)
    if main_revision is not None and _crates_tree(main_revision) != crates_tree:
        raise LedgerError(f"{main_revision} has another crates/ tree than the record's {revision}")
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
        "crates_tree": crates_tree,
        "main_revision": main_revision or revision,
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


def _crates_tree(revision: str) -> str:
    """The `crates/` tree: equal trees run the same detection code."""
    return subprocess.run(
        ["git", "rev-parse", f"{revision}:crates"], cwd=ROOT, check=True,
        capture_output=True, text=True,
    ).stdout.strip()


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
    keys = ("layer", "document_id", "label", "start", "end", "leaked", "covered")
    if [tuple(row[key] for key in keys) for row in rows] != [
        tuple(row[key] for key in keys) for row in expected
    ]:
        raise LedgerError("ledger rows differ from the leaked spans the record re-derives")
    for row in rows:
        letter = row["cause"]
        if letter not in CAUSES:
            raise LedgerError(f"unknown cause {letter!r}")
        if (letter == "d") != (row["covered"] > 0):
            raise LedgerError(f"{row['document_id']}: partial-span cause disagrees with coverage")
        if letter == "b" and row["detail"].get("kind") not in B_KINDS:
            raise LedgerError(f"{row['document_id']}: vetoed row without a known kind")
        if letter == "f" and row["junk_shape"] is None:
            raise LedgerError(f"{row['document_id']}: junk cause without a junk shape")


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
        f"Main `{index['main_revision'][:12]}` (record measured on `{index['revision'][:12]}`, "
        f"same `crates/` tree `{index['crates_tree'][:12]}`), `gaze setup` policy `{index['policy_sha256'][:12]}`, "
        f"scored-label contract v{HEADLINE_CONTRACT}. Leaked bytes per gold span, summed per "
        "label (the scorecard's `per_label_recall`); overlapping gold counts once per span, so "
        f"the label sum ({totals[str(HEADLINE_CONTRACT)]['C']['labels']:,} B) can exceed the "
        f"headline leaked bytes ({totals[str(HEADLINE_CONTRACT)]['C']['headline_leaked']:,} B).",
        "",
        "Layer C by label and cause (bytes):",
        "",
        "| Label | " + " | ".join(f"{letter} {CAUSES[letter]}" for letter in letters) + " | Total |",
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
    lines += ["", "Top 10 layer C clusters (label, cause, mechanism):", "",
              "| Label | Cause | Mechanism | Spans | Bytes |", "| --- | --- | --- | ---: | ---: |"]
    for (label, letter, key), (spans, count) in clusters(scored_c)[:10]:
        lines.append(f"| {label} | {letter} {CAUSES[letter]} | {key} | {spans} | {count:,} |")
    junk = [row for row in scored_c if row["junk_shape"] is not None]
    lines += ["", f"Junk-shaped gold in layer C: {len(junk)} spans, "
              f"{sum(row['leaked'] for row in junk):,} leaked bytes "
              f"({sum(row['leaked'] for row in junk if row['cause'] == 'f'):,} B with no candidate, "
              "cause f). They stay in the target until an audited contract change."]
    return "\n".join(lines)


def mechanism(row: Mapping[str, Any]) -> str:
    detail = row["detail"]
    letter = row["cause"]
    if letter == "d":
        return "covered by " + ", ".join(sorted({item["class"] for item in detail["by"]}))
    if letter == "b":
        return f"{detail['kind']} `{detail['recognizer']}`"
    if letter == "c":
        return f"`{detail['loser']}` lost to `{detail['winner_class']}` ({detail['tier']})"
    if letter == "e":
        return "copy protected by " + ", ".join(detail["copy_stages"])
    if letter in {"a", "f"}:
        closest = detail["closest"]
        return "nothing near" if closest is None else (
            f"closest `{closest['class']}`" + (" (compatible)" if closest["compatible"] else ""))
    return detail["note"]


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
                          "on another commit with the same crates/ tree")
    commands.add_parser("check", help="fail if the ledger, its totals or its table drifted")
    commands.add_parser("render", help="rewrite the table from the committed ledger")
    args = parser.parse_args(argv)
    try:
        if args.command == "probe":
            probe(record_path=args.record, binary=args.binary, policy=args.policy,
                  dataset=args.dataset, model_dir=args.model_dir, machine=args.machine,
                  main_revision=args.main_revision)
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
