#!/usr/bin/env python3
"""Contract v3 gold-gap evidence: replay saved predictions, draw the audit sample.

The gold-gap column is a diagnostic until a human audit passes, so this script
does three things, all from the same saved predictions (no model runs):

  replay   score one per-document trace under v2 and v3 and prove the leaked,
           true-positive and false-positive bytes are identical, then print the
           gold-gap column per label.
  sample   draw the seeded, stratified 200-candidate audit sample from the
           final eligibility set and write it without any document text.
  sheet    render a local audit sheet (value plus context) for the person who
           fills the verdicts; it goes under target/, never into the repo.

The trace is JSON Lines, one document per line, as the bench triage recorded
it: uid, language, negative_category, text, gold and excluded spans as
[byte_start, byte_end, label], error, and the final protection trace items
{s, e, class, ...}.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
import re
import sys
from collections import defaultdict
from pathlib import Path
from typing import Iterable, Sequence

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gaze_bench_score as score  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]
V2_PATH = "docs/reference/benchmarks/scored-labels-v2.json"
V3_PATH = "docs/reference/benchmarks/scored-labels-v3.json"
SAMPLE_PATH = "docs/reference/benchmarks/gold-gap-sample-v3.json"

SAMPLE_SEED = 20260922
SAMPLE_SIZE = 200
SAMPLE_FLOORS = {"FIRSTNAME": 40, "SURNAME": 40, "CITY": 40}
# Ambiguous candidates are drawn at this multiple of the plain candidates'
# rate within a label; the per-entry design weight undoes it when estimating a
# population rate.
AMBIGUOUS_OVERSAMPLE = 2
# The declared bound is a one-sided 95 % upper limit of at most 5 % on the
# false-credit rate. Exactly (Clopper-Pearson) that allows 4 failures of 200:
# 4 gives 4.52 %, 5 would give 5.18 %.
MAX_FAILURES = 4
MAX_UPPER_BOUND = 0.05
CONTEXT_BYTES = 60
LEARNED_CLASSES = frozenset({"name", "location", "organization"})
# Sampling-design heuristic only, never a scoring input: German common nouns
# that are also surnames or places, plus three English months, so a
# same-document repeat may be the other sense (`Wohnort: Essen. Das Essen ist
# fertig.`). A hit only moves the candidate into the oversampled ambiguous
# sub-stratum.
GERMAN_NOUN_SURNAME_SEEDS = frozenset(
    {
        "Adler", "Bauer", "Berg", "Braun", "Essen", "Fuchs", "Hahn", "Halle",
        "Jung", "Klein", "Koch", "Kraus", "Lang", "Schwarz", "Sommer", "Stein",
        "Vogel", "Weiss", "Winter", "Wolf",
        "June", "March", "May",
    }
)

PERSON_LABELS = frozenset({"FIRSTNAME", "SURNAME"})
PLACE_LABELS = frozenset({"BUILDINGNUM", "CITY", "COUNTRY", "REGION", "STATE", "STREET"})
ORGANISATION_LABELS = frozenset({"COMPANYNAME", "ORGANIZATION"})


class TraceDocument:
    def __init__(self, row: dict) -> None:
        self.uid: str = row["uid"]
        self.negative_category: str | None = row["negative_category"]
        self.error: str | None = row["error"]
        spans = [score.Span(s, e, label) for s, e, label in row["gold"] + row["excluded"]]
        self.document = score.Document(
            uid=row["uid"],
            text=row["text"],
            language=row["language"],
            region="",
            source_dataset="negative" if row["negative_category"] else "dataiku",
            spans=tuple(sorted(spans, key=lambda span: (span.start, span.end, span.label))),
            negative_category=row["negative_category"],
        )
        self.predictions = [score.Span(item["s"], item["e"], item["class"]) for item in row["trace"]]


def load_trace(path: Path) -> list[TraceDocument]:
    with path.open(encoding="utf-8") as handle:
        return [TraceDocument(json.loads(line)) for line in handle if line.strip()]


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def contract(relative: str) -> score.ScoredLabelContract:
    return score.load_scored_label_contract(REPO_ROOT / relative, display_path=relative)


def scored(trace: Sequence[TraceDocument], rule: score.ScoredLabelContract) -> dict[str, object]:
    """Accumulate like run_config: pipeline errors are fail-closed, not scored."""
    documents = score.apply_scored_label_contract([item.document for item in trace], rule)
    cells = {
        "holdout": score.MetricAccumulator(),
        "negatives": score.MetricAccumulator(),
        "overall": score.MetricAccumulator(),
    }
    for item, document in zip(trace, documents, strict=True):
        if item.error is not None:
            continue
        cell = "negatives" if item.negative_category else "holdout"
        cells[cell].add(document, item.predictions)
        cells["overall"].add(document, item.predictions)
    return {name: accumulator.result() for name, accumulator in cells.items()}


def replay(trace_path: Path) -> dict[str, object]:
    trace = load_trace(trace_path)
    v2 = scored(trace, contract(V2_PATH))
    v3 = scored(trace, contract(V3_PATH))
    for cell, result in v3.items():
        without = {key: value for key, value in result.items() if key != "gold_gap"}
        if without != v2[cell]:
            raise SystemExit(f"{cell}: v3 changed a v2 number; refusing the evidence")
        if result["utf8_bytes"]["leaked"] != v2[cell]["utf8_bytes"]["leaked"]:
            raise SystemExit(f"{cell}: leaked bytes moved")
    rows = {}
    for cell in v2:
        bytes_v2 = v2[cell]["utf8_bytes"]
        gap = v3[cell]["gold_gap"]
        rows[cell] = {
            "leaked_bytes_v2": bytes_v2["leaked"],
            "leaked_bytes_v3": v3[cell]["utf8_bytes"]["leaked"],
            "true_positive_bytes": bytes_v2["true_positive"],
            "false_positive_bytes_v2": bytes_v2["false_positive"],
            "false_positive_bytes_after_gold_gap": gap["false_positive_bytes_after_gold_gap"],
            "gold_gap_protected_bytes": gap["gold_gap_protected_bytes"],
            "gold_gap_protected_ranges": gap["gold_gap_protected_ranges"],
            "precision_v2": bytes_v2["precision"],
            "adjusted_precision": gap["adjusted_precision"],
            "gold_gap_protected_bytes_by_label": gap["gold_gap_protected_bytes_by_label"],
            "gold_gap_protected_ranges_by_label": gap["gold_gap_protected_ranges_by_label"],
        }
    return {
        "trace": trace_path.name,
        "trace_sha256": sha256_file(trace_path),
        "documents": len(trace),
        "failed_closed_documents": sum(item.error is not None for item in trace),
        "v2_numbers_identical_under_v3": True,
        "cells": rows,
    }


def eligible(trace: Sequence[TraceDocument]) -> list[dict[str, object]]:
    """The final eligibility set: exactly the ranges the v3 scorer credits.

    Each entry carries the predicted class and attributed gold span the
    scorer itself recorded for that credit, so the sample cannot disagree
    with the score about who earned a range.
    """
    rule = contract(V3_PATH)
    documents = score.apply_scored_label_contract([item.document for item in trace], rule)
    population: list[dict[str, object]] = []
    for item, document in zip(trace, documents, strict=True):
        if item.error is not None or item.negative_category is not None:
            continue
        gold, ignored, predictions = score.contract_scoring_view(document, item.predictions)
        for credit in score.gold_gap_credits(document, predictions, gold, ignored):
            population.append(
                {
                    "document_id": item.uid,
                    "byte_start": credit.start,
                    "byte_end": credit.end,
                    "gold_label": credit.label,
                    "predicted_class": credit.predicted_class,
                    "attributed_gold_span": [credit.attributed_start, credit.attributed_end],
                }
            )
    return population


def unlabelled_use_elsewhere(
    trace: Sequence[TraceDocument], values: Iterable[bytes]
) -> set[bytes]:
    """Capitalised values used unlabelled in a holdout document that never
    labels them: the corpus's own evidence that the word is not only a name."""
    documents = []
    for item in trace:
        if item.negative_category is not None:
            continue
        text = item.document.text.encode("utf-8")
        spans = item.document.spans
        gold_values = {text[span.start : span.end] for span in spans}
        gold = score.merge_intervals((span.start, span.end) for span in spans)
        documents.append((text, gold_values, gold))
    found: set[bytes] = set()
    for value in values:
        if not value[:1].decode("utf-8", errors="ignore").isupper():
            continue
        for text, gold_values, gold in documents:
            if value in gold_values:
                continue
            at = text.find(value)
            while at != -1:
                end = at + len(value)
                if not score.interval_overlaps((at, end), gold) and (
                    score.gold_gap_on_word_boundary(text, at, end)
                ):
                    found.add(value)
                    break
                at = text.find(value, at + 1)
            if value in found:
                break
    return found


def ambiguity_signals(
    trace: Sequence[TraceDocument], population: Sequence[dict[str, object]], wordlist: Path
) -> tuple[dict[tuple[str, int, int], list[str]], dict[str, object]]:
    """Why a candidate is oversampled; the flag only moves design weights."""
    english = {
        word.strip()
        for word in wordlist.read_text(encoding="utf-8", errors="ignore").splitlines()
        if word.strip() and word.strip().islower()
    }
    # Plain lowercase words outside gold: whitespace tokens made only of
    # letters (after edge punctuation), so `emma.clarke@mail` or a lowercase
    # username never makes a name look like a common word.
    lowercase_words: set[str] = set()
    for item in trace:
        if item.negative_category is not None:
            continue
        text = item.document.text.encode("utf-8")
        gold = score.merge_intervals((span.start, span.end) for span in item.document.spans)
        for match in re.finditer(rb"\S+", text):
            if score.interval_overlaps((match.start(), match.end()), gold):
                continue
            token = match.group().decode("utf-8").strip(".,;:!?()[]{}\"'«»„“”‚‘’")
            if token.isalpha() and token.islower():
                lowercase_words.add(token)
    texts = {item.uid: item.document.text.encode("utf-8") for item in trace}
    unlabelled_elsewhere = unlabelled_use_elsewhere(
        trace,
        {
            texts[entry["document_id"]][entry["byte_start"] : entry["byte_end"]]
            for entry in population
        },
    )
    signals: dict[tuple[str, int, int], list[str]] = {}
    for entry in population:
        key = (entry["document_id"], entry["byte_start"], entry["byte_end"])
        raw = texts[entry["document_id"]][entry["byte_start"] : entry["byte_end"]]
        value = raw.decode("utf-8")
        reasons = []
        if value.lower() in english:
            reasons.append("english_dictionary_word")
        if value[:1].isupper() and value.lower() in lowercase_words:
            reasons.append("lowercase_use_in_corpus")
        if raw in unlabelled_elsewhere:
            reasons.append("unlabelled_use_elsewhere")
        if value in GERMAN_NOUN_SURNAME_SEEDS:
            reasons.append("german_noun_surname_seed")
        signals[key] = reasons
    basis = {
        "english_wordlist": str(wordlist),
        "english_wordlist_sha256": sha256_file(wordlist),
        "english_wordlist_rule": "lowercase-only entries; the value lowercased is one of them",
        "lowercase_use_rule": (
            "a capitalised value whose lowercase form is a letters-only whitespace token "
            "outside gold somewhere in the holdout text"
        ),
        "unlabelled_use_elsewhere_rule": (
            "a capitalised value that occurs on word boundaries outside gold in another "
            "holdout document where that value is never gold (German nouns are always "
            "capitalised, so Essen the meal never shows up lowercase)"
        ),
        "german_noun_surname_seeds": sorted(GERMAN_NOUN_SURNAME_SEEDS),
    }
    return signals, basis


def largest_remainder(total: int, weights: dict[str, int], caps: dict[str, int]) -> dict[str, int]:
    allocation = {key: 0 for key in weights}
    remaining = total
    while remaining > 0:
        open_keys = [key for key in weights if allocation[key] < caps[key]]
        if not open_keys:
            raise SystemExit("population is smaller than the sample")
        mass = sum(weights[key] for key in open_keys)
        shares = {key: remaining * weights[key] / mass for key in open_keys}
        granted = 0
        for key in open_keys:
            add = min(math.floor(shares[key]), caps[key] - allocation[key])
            allocation[key] += add
            granted += add
        if granted == 0:
            key = max(open_keys, key=lambda k: (shares[k] - math.floor(shares[k]), k))
            allocation[key] += 1
            granted = 1
        remaining -= granted
    return allocation


def allocate(
    total: int, groups: dict[str, tuple[int, int]]
) -> tuple[dict[str, int], dict[str, tuple[int, int]]]:
    """Per-label sample sizes, and each label's (ambiguous, plain) split.

    `groups` maps a gold label to its (ambiguous, plain) sub-stratum sizes.
    Every non-empty sub-stratum gets one draw first, so no candidate has a
    zero inclusion probability; the label floors apply on top, and the rest
    goes proportionally to label size (largest remainder).
    """
    sizes = {label: ambiguous + plain for label, (ambiguous, plain) in groups.items()}
    floors = {
        label: max(
            min(SAMPLE_FLOORS.get(label, 0), sizes[label]),
            sum(1 for size in groups[label] if size),
        )
        for label in groups
    }
    extra = largest_remainder(
        total - sum(floors.values()),
        sizes,
        {label: sizes[label] - floors[label] for label in sizes},
    )
    allocation = {label: floors[label] + extra[label] for label in sizes}
    split = {}
    for label, (ambiguous, plain) in groups.items():
        n = allocation[label]
        n_ambiguous = 0
        if ambiguous:
            # Ambiguous members are drawn at AMBIGUOUS_OVERSAMPLE times the
            # plain members' rate, which stays a real split even when most of
            # a label is flagged; one draw stays reserved for each side.
            boosted = AMBIGUOUS_OVERSAMPLE * ambiguous
            target = round(n * boosted / (boosted + plain))
            n_ambiguous = min(ambiguous, max(1, target), n - (1 if plain else 0))
        n_plain = min(plain, n - n_ambiguous)
        n_ambiguous = min(ambiguous, n - n_plain)
        split[label] = (n_ambiguous, n_plain)
    return allocation, split


def check_design_weights(
    entries: Sequence[dict[str, object]], strata: Sequence[dict[str, object]], population: int
) -> None:
    """Every candidate can be drawn, so the weights add up to the population."""
    unsampled = [
        f"{stratum['gold_label']}/{stratum['sub_stratum']}"
        for stratum in strata
        if stratum["population"] and not stratum["sampled"]
    ]
    total = math.fsum(entry["design_weight"] for entry in entries)
    if unsampled or not math.isclose(total, population, rel_tol=0, abs_tol=1e-6):
        raise SystemExit(
            f"design weights sum to {total}, not the eligibility population {population}; "
            f"unsampled strata: {unsampled}"
        )


def question(label: str) -> str:
    if label in PERSON_LABELS:
        return "Is the highlighted repeat the same person as the labelled value?"
    if label in PLACE_LABELS:
        return "Is the highlighted repeat the same place as the labelled value?"
    if label in ORGANISATION_LABELS:
        return "Is the highlighted repeat the same organisation as the labelled value?"
    return "Is the highlighted repeat the same value used as the same personal datum?"


def clopper_pearson_upper(failures: int, n: int, confidence: float = 0.95) -> float:
    """One-sided exact binomial upper bound, by bisection on the CDF."""
    def cdf(p: float) -> float:
        return sum(math.comb(n, k) * p**k * (1 - p) ** (n - k) for k in range(failures + 1))

    low, high = 0.0, 1.0
    for _ in range(200):
        middle = (low + high) / 2
        if cdf(middle) > 1 - confidence:
            low = middle
        else:
            high = middle
    return high


def draw_sample(trace_path: Path, wordlist: Path) -> dict[str, object]:
    trace = load_trace(trace_path)
    population = eligible(trace)
    signals, basis = ambiguity_signals(trace, population, wordlist)
    ordered = sorted(population, key=lambda e: (e["document_id"], e["byte_start"], e["byte_end"]))
    # Every rule-class candidate is audited: they are few, and a rule class
    # repeating a value (a ZIP, a phone) is the shape most likely to be a
    # different datum. The rest is stratified by gold label.
    certain = [e for e in ordered if e["predicted_class"] not in LEARNED_CLASSES]
    by_label: dict[str, list[dict[str, object]]] = defaultdict(list)
    for entry in ordered:
        if entry["predicted_class"] in LEARNED_CLASSES:
            by_label[entry["gold_label"]].append(entry)
    sizes = {label: len(entries) for label, entries in by_label.items()}

    def is_ambiguous(entry: dict[str, object]) -> bool:
        return bool(signals[(entry["document_id"], entry["byte_start"], entry["byte_end"])])

    groups = {
        label: (
            [e for e in members if is_ambiguous(e)],
            [e for e in members if not is_ambiguous(e)],
        )
        for label, members in by_label.items()
    }
    allocation, split = allocate(
        SAMPLE_SIZE - len(certain),
        {label: (len(ambiguous), len(plain)) for label, (ambiguous, plain) in groups.items()},
    )
    rng = random.Random(SAMPLE_SEED)
    entries: list[dict[str, object]] = []
    strata = []

    def take(label: str, name: str, group: list[dict[str, object]], count: int) -> None:
        strata.append(
            {"gold_label": label, "sub_stratum": name, "population": len(group), "sampled": count}
        )
        for entry in rng.sample(group, count) if count else []:
            key = (entry["document_id"], entry["byte_start"], entry["byte_end"])
            entries.append(
                {
                    **entry,
                    "sub_stratum": name,
                    "ambiguity_signals": signals[key],
                    "design_weight": len(group) / count,
                    "question": question(entry["gold_label"]),
                    "verdict": None,
                    "second_adjudicator_verdict": None,
                    "note": None,
                }
            )

    take("*", "rule_class_census", certain, len(certain))
    for label in sorted(groups):
        ambiguous, plain = groups[label]
        n_ambiguous, n_plain = split[label]
        take(label, "ambiguous", ambiguous, n_ambiguous)
        take(label, "plain", plain, n_plain)
    check_design_weights(entries, strata, len(population))
    entries.sort(key=lambda e: (e["gold_label"], e["document_id"], e["byte_start"]))
    for index, entry in enumerate(entries, 1):
        entry["id"] = f"gg-{index:03d}"
    if len(entries) != SAMPLE_SIZE:
        raise SystemExit(f"drew {len(entries)} candidates, expected {SAMPLE_SIZE}")
    if clopper_pearson_upper(MAX_FAILURES, SAMPLE_SIZE) > MAX_UPPER_BOUND:
        raise SystemExit("the acceptance count no longer meets the declared bound")
    return {
        "schema_version": 1,
        "contract": "scored-labels-v3",
        "purpose": (
            "Human audit of the contract v3 gold-gap rule. The column stays a "
            "diagnostic beside the v2 headline until this audit passes."
        ),
        "status": "awaiting_verdicts",
        "source": {
            "trace": trace_path.name,
            "trace_sha256": sha256_file(trace_path),
            "contract_file": V3_PATH,
            "contract_sha256": sha256_file(REPO_ROOT / V3_PATH),
            "generator": "scripts/bench/gold_gap_evidence.py sample",
        },
        "no_document_text": (
            "Entries carry document IDs and byte offsets only. Render values and "
            "context locally with `gold_gap_evidence.py sheet`."
        ),
        "eligibility_population": len(population),
        "eligibility_population_by_label": dict(sorted(sizes.items())),
        "selection": {
            "seed": SAMPLE_SEED,
            "size": SAMPLE_SIZE,
            "floors": SAMPLE_FLOORS,
            "rule_class_census": len(certain),
            "allocation": (
                "every rule-class candidate first (census), then one draw per non-empty "
                "sub-stratum and the label floors, then the rest proportional to "
                "learned-class label population (largest remainder)"
            ),
            "allocation_by_label": dict(sorted(allocation.items())),
            "ambiguous_oversample_factor": AMBIGUOUS_OVERSAMPLE,
            "within_stratum": (
                "rule-class census first; then labels sorted, per label the ambiguous "
                "sub-stratum then the plain one; "
                "random.Random(seed).sample over members sorted by (document_id, byte_start)"
            ),
            "ambiguity_basis": basis,
            "strata": strata,
            "design_weight": "sub-stratum population / sub-stratum sample size",
        },
        "answers": ["yes", "no", "uncertain"],
        "acceptance": {
            "rule": (
                f"at most {MAX_FAILURES} of {SAMPLE_SIZE} answered 'no' or 'uncertain' "
                "(uncertain after second adjudication counts as no)"
            ),
            "max_failures": MAX_FAILURES,
            "one_sided_95_upper_bound_at_max_failures": round(
                clopper_pearson_upper(MAX_FAILURES, SAMPLE_SIZE), 4
            ),
            "bound": (
                "Clopper-Pearson one-sided 95 % upper bound on the candidate-level "
                "false-credit rate must be at most 5 %"
            ),
            "one_sided_95_upper_bound_at_one_more_failure": round(
                clopper_pearson_upper(MAX_FAILURES + 1, SAMPLE_SIZE), 4
            ),
            "clustering": (
                "report failures per document as well; if document clustering widens "
                "the bound past 5 %, the audit fails"
            ),
            "design_note": (
                "ambiguous shapes are oversampled, so the unweighted failure count is "
                "conservative for the population rate; the weighted and byte-weighted "
                "rates are reported with the design weights"
            ),
            "declared_before_looking": True,
        },
        "entries": entries,
    }


def sheet(trace_path: Path, sample_path: Path) -> str:
    trace = {item.uid: item.document.text.encode("utf-8") for item in load_trace(trace_path)}
    sample = json.loads(sample_path.read_text(encoding="utf-8"))
    lines = ["# Gold-gap audit sheet (local, never committed)", ""]
    for entry in sample["entries"]:
        text = trace[entry["document_id"]]
        start, end = entry["byte_start"], entry["byte_end"]
        gold_start, gold_end = entry["attributed_gold_span"]

        def cut(a: int, b: int) -> str:
            return text[a:b].decode("utf-8", errors="replace").replace("\n", " ")

        lines += [
            f"## {entry['id']} · {entry['gold_label']} · {entry['document_id']}",
            "",
            f"**Question:** {entry['question']}",
            "",
            f"- Labelled: …{cut(max(0, gold_start - CONTEXT_BYTES), gold_start)}"
            f"**[{cut(gold_start, gold_end)}]**{cut(gold_end, gold_end + CONTEXT_BYTES)}…",
            f"- Repeat:   …{cut(max(0, start - CONTEXT_BYTES), start)}"
            f"**[{cut(start, end)}]**{cut(end, end + CONTEXT_BYTES)}…",
            "",
        ]
    return "\n".join(lines)


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("replay", "sample", "sheet"):
        command = sub.add_parser(name)
        command.add_argument("--trace", type=Path, required=True)
    sub.choices["sample"].add_argument("--wordlist", type=Path, default=Path("/usr/share/dict/words"))
    sub.choices["sample"].add_argument("--out", type=Path, default=REPO_ROOT / SAMPLE_PATH)
    sub.choices["sheet"].add_argument("--sample", type=Path, default=REPO_ROOT / SAMPLE_PATH)
    sub.choices["sheet"].add_argument(
        "--out", type=Path, default=REPO_ROOT / "target/gold-gap-audit/sheet.md"
    )
    args = parser.parse_args(list(argv) if argv is not None else None)
    if args.command == "replay":
        print(json.dumps(replay(args.trace), indent=2, sort_keys=True))
    elif args.command == "sample":
        args.out.write_text(
            json.dumps(draw_sample(args.trace, args.wordlist), indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        print(f"wrote {args.out}")
    else:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(sheet(args.trace, args.sample), encoding="utf-8")
        print(f"wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
