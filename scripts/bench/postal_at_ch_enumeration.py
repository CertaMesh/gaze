#!/usr/bin/env python3
"""Mixed-document enumeration for `postal.at_ch`: base binary vs head binary.

Builds a deterministic document set from 25 fragments (Austrian/Swiss codes,
known false-positive shapes, German/US codes, unrelated PII noise): every
single fragment, every ordered pair under three separators, and 2,000 seeded
triples. Each document runs under 13 locale chains through two
`clean_for_bench` binaries (default `rule-floor-extended` cell).

Per document and chain it compares the raw bytes each binary protects (the
union of `final_protection_trace` spans, UTF-8 byte offsets):

* lost      = bytes the base protects and the head does not. Any lost byte is
              a regression: adding a recognizer must never unprotect a span.
* gold_gain = head-only bytes inside a gold fragment's code span.
* fp_gain   = head-only bytes inside a known false-positive fragment.
* restore   = the head's `restore.exact` must hold on every document.

Usage:
    python3 scripts/bench/postal_at_ch_enumeration.py BASE_BIN HEAD_BIN OUT.json

Exit status is 1 when any byte is lost, any head restore is inexact, or any
pipeline error occurs, so the claim "lost = 0" is checked, not eyeballed.
"""

from __future__ import annotations

import json
import random
import subprocess
import sys
from collections import defaultdict

# (kind, text, protected substring or None). `gold4` / `gold5` carry the code
# that must be tokenized; `fp4` carries the four digits that ideally stay raw.
FRAGMENTS = [
    ("gold4", "4020 Linz", "4020"),
    ("gold4", "A-1010 Wien", "A-1010"),
    ("gold4", "CH-8001 Zürich", "CH-8001"),
    ("gold4", "PLZ 1010", "1010"),
    ("gold4", "3100 St. Pölten", "3100"),
    ("gold4", "9000 St.Gallen", "9000"),
    ("gold4", "PLZ: 6020", "6020"),
    ("fp4", "1500 Euro", "1500"),
    ("fp4", "um 1430 Uhr", "1430"),
    ("fp4", "Rechnung 1500 Offen", "1500"),
    ("fp4", "Version 2024 Release", "2024"),
    ("fp4", "1500 EUR", "1500"),
    ("fp4", "Betrag 1500 CHF", "1500"),
    ("fp4", "Seite 1500 Kapitel", "1500"),
    ("fp4", "ab 1.1500 Stück", "1500"),
    ("fp4", "Bestellnummer 0815-0815 Artikel", "0815-0815"),
    ("fp4", "#4711 Fehler beheben", "4711"),
    ("gold5", "10115 Berlin", "10115"),
    ("gold5", "D-10115", "10115"),
    ("gold5", "PLZ 80331", "80331"),
    ("gold5", "Springfield, IL 90210", "90210"),
    ("noise", "Musterweg 12", None),
    ("noise", "Tel. +43 1 234 5678", None),
    ("noise", "IBAN AT61 1904 3002 3457 3201", None),
    ("noise", "max.muster@example.com", None),
]
SEPARATORS = [". ", "\n", ", "]
TRIPLES = 2000
SEED = 20260922

CHAINS = [
    ["global"],
    ["en-US"],
    ["de-DE"],
    ["de-AT"],
    ["de-CH"],
    ["de-AT", "de-DE"],
    ["de-DE", "de-AT"],
    ["de-CH", "de-DE"],
    ["de-DE", "de-CH"],
    ["de-AT", "en-US"],
    ["en-US", "de-AT"],
    ["de-AT", "de-CH"],
    # No-policy `core-extended` compatibility chain.
    ["global", "en-US", "de-DE", "de-AT", "de-CH"],
]


def build_documents():
    """Return [(doc_id, text, [(kind, byte_start, byte_end)])]."""
    rng = random.Random(SEED)
    combos = [([i], "") for i in range(len(FRAGMENTS))]
    for sep in SEPARATORS:
        for a in range(len(FRAGMENTS)):
            for b in range(len(FRAGMENTS)):
                if a != b:
                    combos.append(([a, b], sep))
    for _ in range(TRIPLES):
        combos.append((rng.sample(range(len(FRAGMENTS)), 3), rng.choice(SEPARATORS)))

    docs = []
    for n, (idxs, sep) in enumerate(combos):
        text = ""
        spans = []
        for k, i in enumerate(idxs):
            if k:
                text += sep
            kind, frag, code = FRAGMENTS[i]
            if code is not None:
                start = len(text.encode()) + len(frag[: frag.index(code)].encode())
                spans.append((kind, start, start + len(code.encode())))
            text += frag
        docs.append((f"enum-{n:05d}", text + ".", spans))
    return docs


def run(binary, requests):
    payload = "".join(json.dumps(r, ensure_ascii=False) + "\n" for r in requests)
    proc = subprocess.run(
        [binary], input=payload.encode(), capture_output=True, check=False
    )
    if proc.returncode != 0:
        sys.exit(f"{binary} exited {proc.returncode}: {proc.stderr.decode()[-2000:]}")
    out = {}
    for line in proc.stdout.decode().splitlines():
        row = json.loads(line)
        out[row["fixture_id"]] = row
    return out


def protected(row):
    covered = set()
    for item in row.get("final_protection_trace", []):
        covered.update(range(item["raw_start"], item["raw_end"]))
    return covered


def main():
    base_bin, head_bin, out_path = sys.argv[1:4]
    docs = build_documents()
    requests = []
    meta = {}
    for c, chain in enumerate(CHAINS):
        for doc_id, text, spans in docs:
            fid = f"{doc_id}-c{c:02d}"
            requests.append({"fixture_id": fid, "locale_chain": chain, "text": text})
            meta[fid] = (c, text, spans)

    base = run(base_bin, requests)
    head = run(head_bin, requests)

    per_chain = defaultdict(lambda: defaultdict(int))
    examples = defaultdict(list)
    failed = False
    for fid, (c, text, spans) in meta.items():
        stats = per_chain[",".join(CHAINS[c])]
        stats["docs"] += 1
        b, h = base.get(fid), head.get(fid)
        if b is None or h is None or "pipeline_error_code" in b or "pipeline_error_code" in h:
            stats["pipeline_errors"] += 1
            failed = True
            continue
        if not h["restore"]["exact"]:
            stats["head_restore_inexact"] += 1
            failed = True
        pb, ph = protected(b), protected(h)
        lost = pb - ph
        gained = ph - pb
        stats["lost"] += len(lost)
        stats["gained"] += len(gained)
        if lost:
            failed = True
            if len(examples["lost"]) < 20:
                examples["lost"].append({"fixture_id": fid, "text": text})
        for kind, s, e in spans:
            span = set(range(s, e))
            if kind in ("gold4", "gold5"):
                stats[f"{kind}_bytes"] += len(span)
                stats[f"{kind}_base_protected"] += len(span & pb)
                stats[f"{kind}_head_protected"] += len(span & ph)
            else:
                stats["fp4_head_gain"] += len(span & gained)
        if gained and len(examples[f"gain-c{c:02d}"]) < 5:
            raw = text.encode()
            runs, run_start = [], None
            for i in range(len(raw) + 1):
                if i in gained and run_start is None:
                    run_start = i
                elif i not in gained and run_start is not None:
                    runs.append(raw[run_start:i].decode(errors="replace"))
                    run_start = None
            examples[f"gain-c{c:02d}"].append({"text": text, "head_only": runs})

    report = {
        "fragments": len(FRAGMENTS),
        "documents_per_chain": len(docs),
        "chains": len(CHAINS),
        "seed": SEED,
        "per_chain": {k: dict(v) for k, v in per_chain.items()},
        "totals": {
            key: sum(v.get(key, 0) for v in per_chain.values())
            for key in ("docs", "lost", "gained", "fp4_head_gain", "head_restore_inexact", "pipeline_errors")
        },
        "examples": examples,
    }
    with open(out_path, "w") as fh:
        json.dump(report, fh, indent=2, ensure_ascii=False)
    print(json.dumps(report["totals"]))
    for chain, stats in report["per_chain"].items():
        print(
            f"{chain:40s} lost={stats.get('lost', 0):5d} gained={stats.get('gained', 0):6d} "
            f"gold4 {stats.get('gold4_base_protected', 0)}->{stats.get('gold4_head_protected', 0)}/{stats.get('gold4_bytes', 0)} "
            f"gold5 {stats.get('gold5_base_protected', 0)}->{stats.get('gold5_head_protected', 0)}/{stats.get('gold5_bytes', 0)} "
            f"fp4_gain={stats.get('fp4_head_gain', 0)}"
        )
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
