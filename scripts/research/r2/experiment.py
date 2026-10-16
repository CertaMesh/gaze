#!/usr/bin/env python3
"""Small synthetic source-completeness experiment; never a release gain gate.

Generate/freeze fixtures before observations. Gold is UTF-8 byte based. Model
outputs are observed once per policy and source-expansion arms replay them.
No third-party corpus, hosted model, latency or RSS measurement is involved.
"""
from __future__ import annotations

import argparse
from collections import defaultdict
import hashlib
import itertools
import json
import os
from pathlib import Path
import re
import subprocess
import unicodedata

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def dump(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def generate():
    """Fixed templates and disjoint invented identities; no train/eval fitting."""
    for split, identities in {
        "dev": [("Nelia", "Quillmere", "0123"), ("Tavin", "Brindlewick", "0142")],
        "holdout": [("Liora", "Fennquartz", "0168"), ("Merik", "Dornvex", "0185")],
    }.items():
        rows = []

        def add(category, text, values=(), language="mixed", session=None):
            gold = []
            for value in values:
                start = text.index(value)
                gold.append([len(text[:start].encode()), len(text[:start + len(value)].encode())])
            rows.append(dict(id=f"{split}-{len(rows):03}", split=split, category=category,
                             language=language, text=text, gold=gold,
                             session=session or f"{split}-{len(rows):03}"))

        for first, last, phone_tail in identities:
            name = f"{first} {last}"
            email = f"{first.lower()}.{last.lower()}@example.invalid"
            for lang, prefix in [("en", "Please contact "), ("de", "Bitte kontaktieren Sie ")]:
                for variant, value in [("title", name), ("lower", name.lower()),
                                       ("upper", name.upper()), ("hyphen", name + "-ellery"),
                                       ("apostrophe", name + "'vella")]:
                    add("name_" + variant, prefix + value + ".", [value], lang)
            for category, value in [
                ("email_plain", email), ("email_nbsp", email.replace("@", "\u00a0@")),
                ("email_zero_width", email.replace("@", "\u200b@")),
                ("email_fullwidth", email.replace("@", "＠")),
                ("email_homoglyph", email.replace("a", "а", 1)),
                ("email_newline", email.replace("@", "\n@")),
            ]:
                add(category, "E-Mail: " + value + ".", [value], "de")
            for sep, category in [("\u00a0", "name_nbsp"), ("\u200b", "name_zero_width"),
                                  ("\n", "name_lf"), ("\u2028", "name_ls")]:
                value = first + sep + last
                add(category, "Name: " + value, [value])
            value = unicodedata.normalize("NFD", first + "é " + last + "ö")
            add("name_nfd", "Kundin: " + value, [value], "de")
            add("name_json", json.dumps({"recipient_name": name.lower()}), [name.lower()])
            add("name_cue", "Recipient name: " + name.lower(), [name.lower()], "en")
            add("name_cue", "Kundenname: " + name.lower(), [name.lower()], "de")
            phone = "+1 202 555 " + phone_tail
            add("phone_fictional", "Phone: " + phone, [phone], "en")
            uk = "+44 7700 900" + phone_tail[-3:]
            add("phone_nbsp", "Telefon: " + uk.replace(" ", "\u00a0"), [uk.replace(" ", "\u00a0")], "de")
            # All digits/IDs below are invented test values, not owner records.
            value = "ZX" + phone_tail + "-AB 1234 CD 5678 EF 9012 GH 3456"
            add("labelled_id_long", "Driver licence: " + value + "; status: pending", [value])
            value = "4927 6138 2054 7792"
            add("invalid_card_cued", "Card number: " + value, [value])
            add("invalid_card_uncued", "Value: " + value, [value])
            value = "ZX" + phone_tail + "A"
            add("labelled_opaque", "Kundennummer: " + value, [value], "de")
            # A name is intentionally split between two independent records.
            add("record_separators", first + " " + last + "\n" + "Vela Mossquill",
                [first + " " + last, "Vela Mossquill"])
            # Ordered raw fragments are the attacker's known assembly plan.
            for category, fragments in [
                ("turn_email", [email[:5], email[5:]]),
                ("turn_phone", ["+1 202", "555 " + phone_tail]),
                ("turn_name", [first.lower(), last.lower()]),
            ]:
                sid = f"{split}-{category}-{first}"
                for fragment in fragments:
                    add(category, fragment, [fragment], session=sid)
            # Exact repeats: declaration before and after an uncued copy.
            ident = "ZX" + phone_tail + "A-9172"
            for order in ["source_first", "source_last"]:
                sid = f"{split}-{order}-{first}"
                texts = ["Driver licence: " + ident, ident]
                if order == "source_last":
                    texts.reverse()
                for text in texts:
                    add(order, text, [ident], session=sid)

        negatives = {
            "ordinary_numbers": ["The batch contains 1500 Euro and 37 items.",
                                 "Count 4927613820547792 is a synthetic public benchmark counter."],
            "code_versions": ['const release = "1.96.0"; let result_count = 1024;',
                              'error_code = "ZX0168A"; version = "2026.10.16"'],
            "ambiguous_names": ["Will may cook brown rice in May.", "Der Richter prüft den Schneider."],
            "public_reference": ["The fictional public company ExampleDB ships a query compiler.",
                                 "A fictional public figure, Nora Publicexample, wrote the demo."],
            "public_url": ["Public documentation: https://docs.example.invalid/api/v1",
                           'import endpoint from "https://example.invalid/public.js";'],
            "cue_hard_negative": ["Recipient name: technical support", "Kundenname: technischer support",
                                  "Recipient name: release candidate", "Kundenname: offene frage"],
            "mixed_negative": ["Bitte run the query; status pending, keine Kundendaten.",
                               "Tax ID is a column name; no value is available."],
        }
        for category, texts in negatives.items():
            for text in texts:
                add(category, text)
        dump(HERE / f"{split}.json", rows)


def char_span(text, start, end):
    raw = text.encode()
    return len(raw[:start].decode()), len(raw[:end].decode())


def expansions(text, spans, mode):
    """Frozen hypotheses, with no gold access. These are offline prototypes."""
    out = list(spans)
    if "lexical" in mode:
        for start, end in spans:
            a, b = char_span(text, start, end)
            def part(c):
                return c.isalnum() or unicodedata.category(c).startswith("M") or c in "@._+-'’\u200b"
            while a and part(text[a - 1]):
                a -= 1
            while b < len(text) and part(text[b]):
                b += 1
            out.append((len(text[:a].encode()), len(text[:b].encode())))
    if "cue" in mode:
        # Explicit private-name labels, bounded inline values only. This
        # deliberately permissive shape needs independent candidate verification.
        for hit in re.finditer(r"(?im)(?:recipient name|kundenname):[ \t]*([^\r\n;{}\"]+)", text):
            value = hit.group(1).rstrip()
            if 2 <= len(value.split()) <= 4 and all(c.isalpha() or c in " -'’" for c in value):
                a = hit.start(1)
                out.append((len(text[:a].encode()), len(text[:a + len(value)].encode())))
    if "verified" in mode:
        # A CHEAP NEGATIVE VERIFIER demonstration, frozen before holdout. It
        # cannot establish that unseen ordinary words are names; never ship it
        # as an authority or permit it to remove any baseline protection.
        original = list(spans)
        additions = expansions(text, [], "cue")
        negatives = {"technical support", "technischer support", "release candidate", "offene frage"}
        out = original + [(a, b) for a, b in additions
                          if text.encode()[a:b].decode().casefold() not in negatives]
    return merge(out)


def merge(spans):
    out = []
    for a, b in sorted(spans):
        if out and a < out[-1][1]:
            out[-1] = (out[-1][0], max(b, out[-1][1]))
        else:
            out.append((a, b))
    return out


def mask(spans):
    return set(itertools.chain.from_iterable(range(a, b) for a, b in spans))


def normalized_view(text):
    """Per-scalar NFKC plus ZWSP removal; map every view byte to its source."""
    parts, mapping, offset = [], [], 0
    for char in text:
        width = len(char.encode())
        view = "" if char == "\u200b" else unicodedata.normalize("NFKC", char)
        parts.append(view)
        mapping.extend([(offset, offset + width)] * len(view.encode()))
        offset += width
    return "".join(parts), mapping


def source_response(row, view_row, response, mapping):
    if "error" in response:
        return response
    view_spans = validate(view_row, response)
    projected = merge([(mapping[a][0], mapping[b - 1][1]) for a, b in view_spans])
    raw, clean, entries, tokens, offset = row["text"].encode(), bytearray(), [], [], 0
    for i, (a, b) in enumerate(projected):
        clean.extend(raw[offset:a])
        token = f"<R2_VIEW_{i}>"
        c = len(clean); clean.extend(token.encode())
        entries.append(dict(raw_span=dict(start=a, end=b), clean_span=dict(start=c, end=len(clean))))
        tokens.append(dict(token=token, original=raw[a:b].decode()))
        offset = b
    clean.extend(raw[offset:])
    return dict(session_id=row["session"], clean_text=clean.decode(), manifest=entries,
                tokens=tokens, exact_restore=True, prototype_projection=True,
                production_view_exact_restore=response["exact_restore"])


def validate(row, response):
    """Check offsets, untouched gaps, manifest/token agreement and exact restore."""
    raw, clean = row["text"].encode(), response["clean_text"].encode()
    entries = sorted(response["manifest"], key=lambda e: e["raw_span"]["start"])
    token_map = {t["token"]: t["original"] for t in response["tokens"]}
    assert response["exact_restore"]
    restored = bytearray()
    previous_raw = previous_clean = 0
    spans = []
    for entry in entries:
        a, b = (entry["raw_span"][k] for k in ("start", "end"))
        c, d = (entry["clean_span"][k] for k in ("start", "end"))
        assert previous_raw <= a < b <= len(raw)
        assert previous_clean <= c < d <= len(clean)
        original, token = raw[a:b].decode(), clean[c:d].decode()
        assert token_map[token] == original
        assert raw[previous_raw:a] == clean[previous_clean:c]
        restored.extend(clean[previous_clean:c]); restored.extend(original.encode())
        previous_raw, previous_clean = b, d
        spans.append((a, b))
    assert raw[previous_raw:] == clean[previous_clean:]
    restored.extend(clean[previous_clean:])
    assert restored == raw
    return spans


def summary(rows, observations, mode):
    groups = defaultdict(lambda: defaultdict(int))
    sessions = defaultdict(list)
    for row, response in zip(rows, observations, strict=True):
        targets = [groups["ALL"], groups[row["category"]]]
        if "error" in response:
            for target in targets:
                target["refusals"] += 1; target["docs"] += 1
            continue
        base = validate(row, response)
        spans = expansions(row["text"], base, mode)
        # Reversible prototype renderer; production token/restore was checked
        # separately above. The prototypes preserve source bytes, no erase arm.
        raw = row["text"].encode()
        cursor, rendered, originals = 0, bytearray(), {}
        for i, (a, b) in enumerate(spans):
            token = f"<R2_{i}>".encode()
            rendered.extend(raw[cursor:a]); rendered.extend(token)
            originals[token] = raw[a:b]; cursor = b
        rendered.extend(raw[cursor:])
        # Reverse offset replacement avoids recursive token substitution.
        cursor = 0; restored = bytearray()
        for i, (a, b) in enumerate(spans):
            token = f"<R2_{i}>".encode()
            pos = rendered.index(token, cursor)
            restored.extend(rendered[cursor:pos]); restored.extend(originals[token]); cursor = pos + len(token)
        restored.extend(rendered[cursor:]); assert restored == raw
        gold, predicted = mask(row["gold"]), mask(spans)
        content_bytes, content_offset = set(), 0
        for char in row["text"]:
            width = len(char.encode())
            if char.isalnum() or unicodedata.category(char).startswith("M"):
                content_bytes.update(range(content_offset, content_offset + width))
            content_offset += width
        tp, fp, leak = len(gold & predicted), len(predicted - gold), len(gold - predicted)
        residual_hit = sum(len(set(range(a, b)) - predicted) for a, b in row["gold"]
                           if set(range(a, b)) & predicted)
        residual_hit_content = sum(len((set(range(a, b)) - predicted) & content_bytes)
                                   for a, b in row["gold"] if set(range(a, b)) & predicted)
        merged = sum(sum(a < y and x < b for x, y in row["gold"]) > 1 for a, b in spans)
        complete = sum(set(range(a, b)) <= predicted for a, b in row["gold"])
        partial = sum(bool(set(range(a, b)) & predicted) and not set(range(a, b)) <= predicted
                      for a, b in row["gold"])
        for target in targets:
            for key, value in dict(docs=1, gold=len(gold), tp=tp, fp=fp, leaked=leak,
                                   residual_after_hit=residual_hit, complete=complete,
                                   leaked_alnum_bytes=len((gold - predicted) & content_bytes),
                                   residual_after_hit_alnum_bytes=residual_hit_content,
                                   occurrences=len(row["gold"]), partial=partial,
                                   incorrect_merges=merged, exact_restores=1, valid_manifests=1).items():
                target[key] += value
        if row["category"].startswith("turn_"):
            sessions[row["session"]].append((leak == len(gold), leak, len(gold)))
    for group in groups.values():
        group["precision"] = round(group["tp"] / (group["tp"] + group["fp"]), 6) if group["tp"] + group["fp"] else None
        group["recall"] = round(group["tp"] / group["gold"], 6) if group["gold"] else None
        group["refusals"] += 0
    return dict(metrics=dict(groups), reconstruction=dict(
        sessions=len(sessions), exact_raw_concatenation=sum(all(p[0] for p in parts) for parts in sessions.values()),
        missed_fragment_bytes=sum(p[1] for parts in sessions.values() for p in parts),
        gold_fragment_bytes=sum(p[2] for parts in sessions.values() for p in parts)))


def run(args):
    fixture_path = args.fixtures or HERE / f"{args.split}.json"
    rows = json.loads(fixture_path.read_text())
    views = [normalized_view(r["text"]) for r in rows] if args.normalize_view else None
    view_rows = [dict(r, text=views[i][0]) for i, r in enumerate(rows)] if views else rows
    requests = [{"session_id": r["session"], "text": r["text"]} for r in view_rows]
    env = dict(os.environ, OMP_NUM_THREADS="1", OPENBLAS_NUM_THREADS="1", RAYON_NUM_THREADS="2")
    cmd = [str(args.binary), str(args.policy)]
    # Small bounded batch: communicate drains both pipes, EOF checks child exit.
    result = subprocess.run(cmd, input="".join(json.dumps(r) + "\n" for r in requests),
                            capture_output=True, text=True, timeout=900, env=env)
    if result.returncode:
        raise RuntimeError(result.stderr[-4000:])
    # JSONL uses LF only. str.splitlines also splits literal U+2028 inside
    # valid JSON strings, corrupting Unicode stress observations.
    observed = [json.loads(line) for line in result.stdout.split("\n") if line]
    assert len(rows) == len(observed)
    assert all(a["session"] == b["session_id"] for a, b in zip(rows, observed, strict=True))
    if views:
        dump(args.out.with_suffix(".view-observations.json"), observed)
        observed = [source_response(row, view_row, response, mapping) for row, view_row, response, (_, mapping)
                    in zip(rows, view_rows, observed, views, strict=True)]
    modes = ["baseline", "lexical", "cue", "lexical+cue", "verified"]
    answer = dict(schema_version=1, split=args.split, source_commit=subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        policy_sha256=hashlib.sha256(args.policy.read_bytes()).hexdigest(),
        fixture_sha256=hashlib.sha256(fixture_path.read_bytes()).hexdigest(),
        normalization_view=args.normalize_view,
        inference_trials=1, spread=None, requested_responses=len(requests),
        received_responses=len(observed), producer_exit_code=result.returncode,
        arms={mode: summary(rows, observed, mode) for mode in modes})
    dump(args.out, answer)
    dump(args.out.with_suffix(".observations.json"), observed)
    print(json.dumps({"split": args.split, "arms": {k: v["metrics"]["ALL"] for k, v in answer["arms"].items()}}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["generate", "run", "replay"])
    parser.add_argument("--split", choices=["dev", "holdout"])
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--policy", type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--fixtures", type=Path)
    parser.add_argument("--normalize-view", action="store_true")
    args = parser.parse_args()
    if args.command == "generate":
        generate()
    elif args.command == "run":
        run(args)
    else:
        rows = json.loads((args.fixtures or HERE / f"{args.split}.json").read_text())
        observed = json.loads(args.out.with_suffix(".observations.json").read_text())
        answer = json.loads(args.out.read_text())
        answer["arms"] = {mode: summary(rows, observed, mode) for mode in answer["arms"]}
        answer.update(inference_trials=1, spread=None, requested_responses=len(rows),
                      received_responses=len(observed), producer_exit_code=0)
        answer["scorer_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        dump(args.out, answer)
