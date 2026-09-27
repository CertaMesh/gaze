#!/usr/bin/env python3
"""TypeSafe tiebreak for the contested cards of the contract v3 gold-gap audit.

Three model judges (Claude Opus, Codex, TypeSafe) ruled on every sampled card.
A card is contested when they do not all agree. The rule, fixed before any
re-ask ran:

  1  TypeSafe sides with a 2-of-3 majority: the majority verdict stands.
  2  TypeSafe is the lone dissent: one TypeSafe Score call with the other two
     judges' reasons as context. Confidence >= 0.8 decides (2a); otherwise
     the Opus + Codex majority stands (2b).

The inputs quote synthetic Dataiku benchmark text, so they live in the private
audit archive, not here; pass that folder as --archive. This file is the run
that produced fixtures/gold-gap-tiebreak-v3.json with only its input and
output paths turned into arguments. `gold_gap_evidence.py accept` checks the
sample's verdicts against that results file.
"""
import argparse
import hashlib
import json
import os
import time
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path

RESULTS_PATH = Path(__file__).resolve().parent / "fixtures" / "gold-gap-tiebreak-v3.json"

RUBRIC = (
    "Decide whether the highlighted repeat is genuine PII of the kind the labelled value represents. "
    "For a person name it must refer to the same individual; for a place to the same place; "
    "for an organisation to the same organisation; and for a date, ID, email, phone, or ZIP "
    "to the same value used as the same personal datum. Same bytes alone do not prove this. "
    "Two other judges' verdicts and reasons are given in `other_judges`; weigh their reasoning "
    "against the text, do not defer to them."
)
LEVELS = ["no", "uncertain", "yes"]
CRITERIA = [
    "The repeat has a different referent or meaning than the labelled value despite matching bytes (different person, different place or administrative unit, different datum, homonym, generic use).",
    "The context does not establish whether the repeat has the same referent or meaning as the labelled value.",
    "The repeat is the same person, place, organisation, or personal datum as the labelled value.",
]
THRESHOLD = 0.8


def load(archive: Path):
    cards = {c["id"]: c for c in json.load(open(archive / "cards.json"))}
    props = json.load(open(archive / "proposals.json"))
    codex = {d["id"]: d for d in json.load(open(archive / "verdicts-judge2.json"))}
    ts = {d["id"]: d for d in json.load(open(archive / "verdicts-typesafe.json"))}
    return cards, props, codex, ts


def ctx(w):
    return (w["before"] + "[[" + w["span"] + "]]" + w["after"]).replace("\x00", " ")


def payload_for(cid, cards, props, codex):
    c = cards[cid]
    state = {
        "gold_label": c["gold_label"],
        "question": c["question"],
        "labelled_context": ctx(c["gold"]),
        "repeat_context": ctx(c["cand"]),
        "other_judges": [
            {"judge": "A", "verdict": props[cid]["proposal"], "reason": props[cid]["reason"]},
            {"judge": "B", "verdict": codex[cid]["verdict"], "reason": codex[cid]["reason"]},
        ],
    }
    payload = {"model": "jev-latest", "state": state, "questions": {"referent": {
        "type": "score",
        "instructions": RUBRIC + " How clearly does the highlighted repeat in `repeat_context` refer to the same thing as the highlighted value in `labelled_context`?",
        "criteria": CRITERIA,
    }}}
    return payload


def ask(payload, cid):
    req = urllib.request.Request(
        "https://api.typesafe.ai/v1/systemone",
        data=json.dumps(payload, ensure_ascii=False).encode(),
        headers={"Authorization": "Bearer " + os.environ["TYPESAFE_API_KEY"], "Content-Type": "application/json"},
    )
    for attempt in range(5):
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return json.loads(r.read())["answers"]["referent"]
        except urllib.error.HTTPError as e:
            if e.code not in (429, 500, 502, 503, 504):
                raise RuntimeError(f"HTTP {e.code} on {cid}") from None
        except (TimeoutError, urllib.error.URLError):
            pass
        time.sleep(2 ** attempt)
    raise RuntimeError(f"TypeSafe failed on {cid}")


def contested_ids(props, codex, ts):
    return sorted(i for i in props if not (props[i]["proposal"] == codex[i]["verdict"] == ts[i]["verdict"]))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--archive", type=Path, required=True, help="folder with cards.json, proposals.json and both judge files")
    parser.add_argument("--out", type=Path, default=RESULTS_PATH)
    parser.add_argument("--payloads-only", type=Path, help="write the re-ask payloads here and call nothing")
    args = parser.parse_args()
    cards, props, codex, ts = load(args.archive)
    contested = contested_ids(props, codex, ts)
    assert len(contested) == 12, contested

    if args.payloads_only:
        lone = [cid for cid in contested if ts[cid]["verdict"] not in (props[cid]["proposal"], codex[cid]["verdict"])]
        payloads = {cid: payload_for(cid, cards, props, codex) for cid in lone}
        args.payloads_only.write_text(json.dumps(payloads, ensure_ascii=False, indent=1) + "\n")
        print(f"wrote {len(payloads)} payloads to {args.payloads_only}")
        return

    rows = []
    for cid in contested:
        o, x, t = props[cid]["proposal"], codex[cid]["verdict"], ts[cid]["verdict"]
        row = {"id": cid, "document_id": cards[cid]["document_id"], "opus": o, "codex": x, "typesafe": t}
        if t == o or t == x:
            row.update(rule="1: TypeSafe in 2-of-3 majority", final=t, tiebreak=None)
        else:
            assert o == x, cid  # lone dissent with Opus and Codex agreeing
            a = ask(payload_for(cid, cards, props, codex), cid)
            probs = {LEVELS[int(k)]: v for k, v in a["probabilities"].items()}
            top = max(probs, key=probs.get)
            row["tiebreak"] = {"type": "score", "score": a["score"], "confidence": a["confidence"], "probabilities": probs, "argmax": top}
            if a["confidence"] >= THRESHOLD:
                row.update(rule="2a: TypeSafe re-ask, confidence >= 0.8", final=top)
            else:
                row.update(rule="2b: TypeSafe low-confidence -> Opus + Codex majority", final=o)
        rows.append(row)
        print(json.dumps(row, ensure_ascii=False), flush=True)

    args.out.write_text(json.dumps(rows, ensure_ascii=False, indent=1) + "\n")
    fails = [r["id"] for r in rows if r["final"] in ("no", "uncertain")]
    print("contested final:", dict(Counter(r["final"] for r in rows)), "fails:", fails)
    print("sha256", hashlib.sha256(args.out.read_bytes()).hexdigest())


if __name__ == "__main__":
    main()
