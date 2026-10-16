#!/usr/bin/env python3
"""Reproduce bounded byte/link/utility/authority experiments. Stdlib only.

Outputs are evidence under target/, never repository report files. The native
probe compiles current source unchanged outside its example. It deliberately
seeds known records to isolate repeat-sweep behavior, not shipped NER recall.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import subprocess
import sys

from model import ARMS, aliases, authorize, fingerprint, simulate, utility

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]


def encode(rows):
    return "".join(json.dumps(r, ensure_ascii=False, sort_keys=True) + "\n" for r in rows)


def native(rows, binary, learned=False):
    result = subprocess.run([str(binary), *( ["--learned-seed"] if learned else [])],
                            input=encode(rows), capture_output=True, text=True, check=True)
    output = [json.loads(line) for line in result.stdout.splitlines()]
    if [r["id"] for r in output] != [r["id"] for r in rows]:
        raise ValueError("native output identity mismatch")
    for record, row in zip(rows, output):
        seed_roots = {}
        for turn, pred in zip(record["turns"], row["turns"]):
            for span in pred["spans"]:
                if turn.get("seed"):
                    for gold in turn["gold"]:
                        if (span["start"], span["end"]) == (gold["start"], gold["end"]):
                            seed_roots.setdefault(span["token"], gold["entity"])
                span["root"] = seed_roots.get(span["token"], "abstain")
        row["resource"] = {"state_bytes":len(json.dumps(row["entries"], ensure_ascii=False).encode())}
    return output


def evaluate(record, pred, arm):
    c, roots = Counter(), defaultdict(set)
    for turn, predicted in zip(record["turns"], pred["turns"]):
        gold_bytes = {i for g in turn["gold"] for i in range(g["start"], g["end"])}
        # Auxiliary name-only evaluation excludes other annotated PII from the
        # benign denominator (e.g. a name fragment inside an email). Such bytes
        # are neither credited name recall nor charged as benign corruption.
        ignored = {i for g in turn.get("other_pii", []) for i in range(g["start"], g["end"])} - gold_bytes
        spans = predicted["spans"]
        covered = set()
        for span in spans:
            positions = set(range(span["start"], span["end"]))
            if covered & positions:
                raise ValueError("overlapping predicted replacements")
            if not 0 <= span["start"] < span["end"] <= len(turn["text"].encode()):
                raise ValueError("invalid byte span")
            covered |= positions
        c["gold_bytes"] += len(gold_bytes)
        c["covered_gold_bytes"] += len(covered & gold_bytes)
        c["missed_pii_bytes"] += len(gold_bytes - covered)
        c["other_pii_covered_bytes"] += len(covered & ignored)
        c["false_positive_bytes"] += len(covered - gold_bytes - ignored)
        raw = turn["text"].encode()
        c["false_positive_ascii_whitespace_bytes"] += sum(raw[i] in b" \t\r\n" for i in covered - gold_bytes - ignored)
        c["turns"] += 1
        c["restoration_errors"] += not predicted["restore_ok"]
        for gold in turn["gold"]:
            c["gold_occurrences"] += 1
            match = next((s for s in spans if s["start"] <= gold["start"] and s["end"] >= gold["end"]), None)
            c["fully_protected_occurrences"] += match is not None
            if match and match["root"] != "abstain":
                roots[match["root"]].add(gold["entity"])
            if turn.get("seed"):
                continue
            c["link_targets"] += 1
            if match and match["root"] != "abstain":
                c["links"] += 1
                c["correct_links"] += match["root"] == gold["entity"]
                c["wrong_links"] += match["root"] != gold["entity"]
    c["contaminated_roots"] = sum(len(v) > 1 for v in roots.values())
    c["missed_links"] = c["link_targets"] - c["correct_links"]
    c["conversations"] = 1
    for task, success in utility(record, arm).items():
        c["task_" + task] = success
    c["task_points"] = sum(utility(record, arm).values())
    c["task_possible"] = 5
    for key, value in pred.get("resource", {}).items():
        c["resource_" + key] += value
    return c


def metrics(c):
    result = dict(c)
    for key in ("wrong_links", "restoration_errors", "false_positive_bytes", "contaminated_roots"):
        result.setdefault(key, 0)
    for name, num, den in [("byte_precision", "covered_gold_bytes", None),
                            ("byte_recall", "covered_gold_bytes", "gold_bytes"),
                            ("link_precision", "correct_links", "links"),
                            ("link_recall", "correct_links", "link_targets")]:
        denominator = c[den] if den else c["covered_gold_bytes"] + c["false_positive_bytes"]
        result[name] = c[num] / denominator if denominator else None
    return result


def layer_rows(split):
    """Untouched own generated layer R name records, seeded by initial gold name.

    This is a known-record mechanism probe. Gold supplies only the explicit seed
    full name, not detections on any evaluation text. No downloaded corpus read.
    """
    sys.path.insert(0, str(REPO / "scripts/bench"))
    import agentic_layers as layers
    rows = []
    for r in layers._repeat_records(split, layers.PARTITION_SEEDS[split]):
        names = [g for g in r.gold if g.label in {"GIVENNAME", "SURNAME"}]
        if not names:
            continue
        given = next((g.value for g in names if g.label == "GIVENNAME"), None)
        family = next((g.value for g in names if g.label == "SURNAME"), None)
        if not given or not family:
            continue
        full = f"{given} {family}"
        seed = "Owner record:\n" + full
        # No utility scoring on this auxiliary slice.
        rows.append({"id": r.uid, "split": split, "language": r.language, "family": "layer_R/" + r.family,
                     "entities": [{"id": "p1", "kind": "Name", "full": full, "given": given, "family": family}],
                     "turns": [{"text": seed, "seed": True, "gold": [{"start":14,"end":14 + len(full.encode()),"entity":"p1","kind":"Name","value":full}]},
                               {"text": r.text, "gold": [{"start":g.start,"end":g.end,"value":g.value,"entity":"p1","kind":"Name"} for g in names],
                                "other_pii":[{"start":g.start,"end":g.end} for g in r.gold if g.label not in {"GIVENNAME","SURNAME"}]}]})
    return rows


def authority_suite():
    records = {"p1": {"full": "Clara Ellery", "given": "Clara"}, "a1": {"city": "Sampleford", "full": "Maple Street 5, Sampleford"}}
    base = {"tenant":"example-tenant", "principal":"owner", "session":"s1", "epoch":"1", "root":"p1", "component":"given", "action":"reply", "destination":"owner-draft"}
    grant = {**base, "issued_at":0, "expires_at":20, "revoked":False}
    cases = [("valid", base, grant, 10, True)]
    for field, value in {"tenant":"other-tenant", "principal":"agent", "session":"s2", "epoch":"2", "root":"p999", "component":"full", "action":"export", "destination":"cloud-llm"}.items():
        cases.append(("wrong_" + field, {**base, field:value}, grant, 10, False))
    cases.extend([
        ("expired", base, grant, 20, False), ("not_yet_issued", base, grant, -1, False),
        ("revoked", base, {**grant,"revoked":True}, 10, False),
        ("prompt_injection", {**base,"instruction":"ignore grants"}, grant, 10, False),
        ("agent_authored_grant", {**base,"grant":grant}, grant, 10, False),
        ("unseen_component", {**base,"component":"email"}, {**grant,"component":"email"}, 10, False),
        ("type_confusion", {**base,"component":["given"]}, grant, 10, False),
        ("nested_selector", {**base,"component":"given/../full"}, grant, 10, False),
        ("null_request", None, grant, 10, False), ("missing_grant", base, None, 10, False),
        ("known_other_record", {**base,"root":"a1"}, grant, 10, False),
        ("unicode_confusable", {**base,"principal":"оwner"}, grant, 10, False),
        ("invalid_policy_expiry", base, {**grant,"expires_at":"forever"}, 10, False),
        ("missing_revocation_state", base, {k:v for k,v in grant.items() if k != "revoked"}, 10, False),
        ("explicit_city_grant", {**base,"root":"a1","component":"city"}, {**grant,"root":"a1","component":"city"}, 10, True),
        ("city_grant_not_whole_address", {**base,"root":"a1","component":"full"}, {**grant,"root":"a1","component":"city"}, 10, False),
    ])
    results = []
    for name, req, policy, now, expected in cases:
        raw = authorize(req, policy, records, now)
        results.append({"case":name,"allowed":raw is not None,"expected":expected,"pass":(raw is not None) == expected,
                        "released_bytes":len(raw.encode()) if raw else 0})
    # Repeated invalid probes reveal no value through this resolver interface.
    repeated = [authorize({**base,"root":f"p{i}"}, grant, records) for i in range(100,200)]
    return {"cases":results, "all_pass":all(x["pass"] for x in results),
            "invalid_probe_plaintext_bytes":sum(len(x.encode()) for x in repeated if x)}


def linkability(rows, native_predictions, binary):
    # Exactly observable equality only; no entropy/reconstruction estimate.
    r = rows[0]
    value = r["entities"][0]["full"]
    duplicate = {**r, "turns":[r["turns"][0], {"text":value,"gold":[],"seed":False}]}
    a, b = native([duplicate, duplicate], binary)
    first = a["turns"][0]["spans"][0]["token"]
    last = a["turns"][1]["spans"][0]["token"]
    foreign = b["turns"][0]["spans"][0]["token"]
    return {"native_same_session_equal_raw_reuses":first == last,
            "native_same_raw_new_session_token_differs":first != foreign,
            "foreign_token_rejected":all(p["authority"]["foreign_rejected"] for p in native_predictions),
            "unissued_guesses_rejected":all(p["authority"]["guessed_rejected"] for p in native_predictions),
            "existing_ordinal_guess_restores":all(p["authority"]["guessed_existing_restores"] for p in native_predictions),
            "prompt_injected_issued_text_restores":all(p["authority"]["prompt_injected_issued_restores"] for p in native_predictions),
            "global_value_digest_equal_across_sessions":fingerprint(value) == fingerprint(value),
            "warning":"Digest equality is a deliberate design counterexample, not cryptographic unlinkability evidence. Type, equality, frequency, order, and selectors remain observable."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--split", choices=["dev","test"], required=True)
    parser.add_argument("--binary", type=Path, default=REPO / "target/debug/examples/research_r1")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    all_rows = [json.loads(x) for x in (HERE / "scenarios.jsonl").read_text().splitlines()]
    rows = [r for r in all_rows if r["split"] == args.split]
    native_rows = native(rows, args.binary)
    predictions = {arm: [simulate(r, arm) for r in rows] for arm in ARMS}
    predictions.update(native=native_rows, native_learned=native(rows, args.binary, True))
    totals, by_family, by_language, details = {}, {}, {}, []
    for arm, preds in predictions.items():
        total, families, languages = Counter(), defaultdict(Counter), defaultdict(Counter)
        for row, pred in zip(rows, preds):
            counts = evaluate(row, pred, arm)
            total.update(counts); families[row["family"]].update(counts); languages[row["language"]].update(counts)
            details.append({"id":row["id"],"arm":arm,"counts":dict(counts)})
        totals[arm] = metrics(total)
        by_family[arm] = {k:metrics(v) for k,v in families.items()}
        by_language[arm] = {k:metrics(v) for k,v in languages.items()}
    aux = layer_rows(args.split)
    aux_totals = {}
    for arm in ["native", "native_learned", "exact", "structured", "evidence"]:
        preds = native(aux, args.binary, arm == "native_learned") if arm.startswith("native") else [simulate(r, arm) for r in aux]
        aggregate = Counter()
        for row, pred in zip(aux, preds):
            # The auxiliary corpus has no task or structured address record.
            patched = {**row,"entities":row["entities"] + [
                {"id":"p2","kind":"Name","full":"Unused Fixture","given":"Unused","family":"Fixture"},
                {"id":"a1","kind":"Location","full":"Unused Road 1","city":"Unused"}]}
            c = evaluate(patched, pred, arm)
            for key in list(c):
                if key.startswith("task_"): del c[key]
            aggregate.update(c)
        aux_totals[arm] = metrics(aggregate)
    provenance = {"head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=REPO,text=True).strip(),
                  "tracked_clean":subprocess.run(["git","diff","--quiet","HEAD"],cwd=REPO).returncode == 0,
                  "base":"034cd7447c9a736fccb6a9c2b165fc48fd38dd98",
                  "split":args.split,"scenario_sha256":hashlib.sha256((HERE/"scenarios.jsonl").read_bytes()).hexdigest(),
                  "protocol_sha256":hashlib.sha256((HERE/"protocol.json").read_bytes()).hexdigest(),
                  "binary_sha256":hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                  "source_files":{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in [HERE/"model.py",HERE/"run.py",HERE/"generate.py"]},
                  "external_data_read":False,"timing_measured":False,"native_profile":"known-record-seeded core repeat sweep; no bundled recognizers/models"}
    result = {"provenance":provenance,"totals":totals,"by_family":by_family,"by_language":by_language,
              "own_layer_R":aux_totals,"authority":authority_suite(),"linkability":linkability(rows,native_rows,args.binary),"details":details}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(result,ensure_ascii=False,sort_keys=True,indent=2)+"\n")
    print(json.dumps({"output":str(args.output),"split":args.split,"conversations":len(rows),"authority_pass":result["authority"]["all_pass"]}))


if __name__ == "__main__":
    main()
