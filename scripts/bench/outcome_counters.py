"""Per-run protection outcome counters for benchmark scorecards.

The counters sit beside the existing metrics and never change them. They
separate four things the byte score folds together:

- refused operations (untransformable values, refused possibly-sensitive
  values, and every other fail-closed document), and refusals of documents
  that carry no scored gold (false-positive refusals);
- protection of confirmed identifiers versus protection of ambiguous values
  (collision-family tokens such as `custom:family:<name>`);
- false positives by document count and by affected-value count;
- how much non-PII content stays usable, counting bytes lost to false
  positives and to refusals separately, so a refusal never reads as a gain.

Prove that a committed release record still rescores to its committed
scorecards with only the counters added:

    python scripts/bench/outcome_counters.py \
      docs/reference/benchmarks/observations-v0.17.0.jsonl.gz \
      --release v0.17.0 --check docs/reference/benchmarks/outcome-counters-v0.17.0.json
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from collections import Counter
from collections.abc import Mapping, Sequence
from pathlib import Path

import gaze_bench_score as score

OUTCOME_COUNTERS_FIELD = "outcome_counters"

UNTRANSFORMABLE_REASONS = score.UNTRANSFORMABLE_REASONS
UNTRANSFORMABLE_CODE_PREFIX = score.UNTRANSFORMABLE_CODE_PREFIX
UNTRANSFORMABLE_CODES = score.UNTRANSFORMABLE_CODES
POSSIBLY_SENSITIVE_REFUSED_CODE = score.POSSIBLY_SENSITIVE_REFUSED_CODE

# A precedence tie or a missing mandatory anchor emits one family-level token:
# the value is protected, but which identifier it is stays unproven.
AMBIGUOUS_CLASS_PREFIX = "custom:family:"

BLOCKED_KINDS = ("untransformable", "possibly_sensitive", "fail_closed")


def is_ambiguous_class(label: str) -> bool:
    return label.startswith(AMBIGUOUS_CLASS_PREFIX)


def refusal_kind(code: str) -> str:
    if code in UNTRANSFORMABLE_CODES:
        return "untransformable"
    if code == POSSIBLY_SENSITIVE_REFUSED_CODE:
        return "possibly_sensitive"
    return "fail_closed"


class _ValueCounts:
    def __init__(self) -> None:
        self.values = 0
        self.gold_values = 0
        self.false_positive_values = 0

    def add(self, overlaps_gold: bool) -> None:
        self.values += 1
        self.gold_values += overlaps_gold
        self.false_positive_values += not overlaps_gold

    def result(self) -> dict[str, int]:
        return {
            "values": self.values,
            "gold_values": self.gold_values,
            "false_positive_values": self.false_positive_values,
        }


class OutcomeAccumulator:
    def __init__(self) -> None:
        self.attempted_documents = 0
        self.blocked: Counter[str] = Counter()
        self.untransformable_reasons: Counter[str] = Counter()
        self.fp_refusal_documents = 0
        self.confirmed = _ValueCounts()
        self.possibly_sensitive = _ValueCounts()
        self.fp_documents = 0
        self.fp_documents_ambiguous_only = 0
        self.non_pii_bytes = 0
        self.false_positive_bytes = 0
        self.refused_non_pii_bytes = 0

    def add_refused(self, document: score.Document, code: str) -> None:
        self.attempted_documents += 1
        kind = refusal_kind(code)
        self.blocked[kind] += 1
        if kind == "untransformable":
            self.untransformable_reasons[code.removeprefix(UNTRANSFORMABLE_CODE_PREFIX)] += 1
        gold, ignored, _ = score.contract_scoring_view(document, ())
        self.fp_refusal_documents += not gold
        non_pii = _non_pii_bytes(document, gold, ignored)
        self.non_pii_bytes += non_pii
        self.refused_non_pii_bytes += non_pii

    def add_scored(self, document: score.Document, predictions: Sequence[score.Span]) -> None:
        self.attempted_documents += 1
        gold, ignored, scored = score.contract_scoring_view(document, predictions)
        false_positive_kinds = set()
        for span in scored:
            overlaps_gold = score.interval_overlaps((span.start, span.end), gold)
            ambiguous = is_ambiguous_class(span.label)
            (self.possibly_sensitive if ambiguous else self.confirmed).add(overlaps_gold)
            if not overlaps_gold:
                false_positive_kinds.add(ambiguous)
        predicted = score.subtract_intervals(
            score.merge_intervals((span.start, span.end) for span in scored), ignored
        )
        false_positive_bytes = score.interval_length(predicted) - score.intersection_length(
            gold, predicted
        )
        # Same definition as metrics.documents_with_false_positives (bytes),
        # so the two document counts always agree.
        self.fp_documents += false_positive_bytes > 0
        self.fp_documents_ambiguous_only += false_positive_bytes > 0 and false_positive_kinds == {True}
        self.non_pii_bytes += _non_pii_bytes(document, gold, ignored)
        self.false_positive_bytes += false_positive_bytes

    def result(self) -> dict[str, object]:
        blocked = sum(self.blocked.values())
        retained = self.non_pii_bytes - self.false_positive_bytes - self.refused_non_pii_bytes
        return {
            "attempted_documents": self.attempted_documents,
            "blocked_operations": {
                "documents": blocked,
                "by_kind": {kind: self.blocked[kind] for kind in BLOCKED_KINDS},
            },
            "untransformable_refused": {
                "documents": self.blocked["untransformable"],
                "by_reason": {
                    reason: self.untransformable_reasons[reason]
                    for reason in UNTRANSFORMABLE_REASONS
                },
            },
            "fp_refusals": {"documents": self.fp_refusal_documents},
            "protection": {
                "confirmed": self.confirmed.result(),
                "possibly_sensitive": {
                    **self.possibly_sensitive.result(),
                    # `pass_audited` leaves the value raw and writes only an
                    # audit row, which the bench response does not carry.
                    "dispositions": {
                        "protect": self.possibly_sensitive.values,
                        "refuse": self.blocked["possibly_sensitive"],
                    },
                },
            },
            "false_positives": {
                "documents": self.fp_documents,
                "documents_from_possibly_sensitive_only": self.fp_documents_ambiguous_only,
                "values": self.confirmed.false_positive_values
                + self.possibly_sensitive.false_positive_values,
                "possibly_sensitive_values": self.possibly_sensitive.false_positive_values,
            },
            "usable_content": {
                "non_pii_bytes": self.non_pii_bytes,
                "retained_non_pii_bytes": retained,
                "lost_to_false_positive_bytes": self.false_positive_bytes,
                "lost_to_refusal_bytes": self.refused_non_pii_bytes,
                "retained_ratio": score.safe_ratio(retained, self.non_pii_bytes),
            },
        }


def _non_pii_bytes(
    document: score.Document,
    gold: Sequence[tuple[int, int]],
    ignored: Sequence[tuple[int, int]],
) -> int:
    return (
        len(document.text.encode("utf-8"))
        - score.interval_length(ignored)
        - score.interval_length(gold)
    )


def has_outcome_counters(card: Mapping[str, object]) -> bool:
    runs = card.get("runs")
    return isinstance(runs, list) and any(
        isinstance(run, dict) and OUTCOME_COUNTERS_FIELD in run for run in runs
    )


def strip_outcome_counters(card: Mapping[str, object]) -> dict[str, object]:
    """Drop the counters from every run, for comparison with older scorecards."""
    def runs(items: object) -> object:
        if not isinstance(items, list):
            return items
        return [
            {key: value for key, value in run.items() if key != OUTCOME_COUNTERS_FIELD}
            if isinstance(run, dict) else run
            for run in items
        ]

    stripped = dict(card)
    if "runs" in stripped:
        stripped["runs"] = runs(stripped["runs"])
    layers = stripped.get("layers")
    if isinstance(layers, dict):
        stripped["layers"] = {
            name: {**layer, "runs": runs(layer["runs"])}
            if isinstance(layer, dict) and "runs" in layer else layer
            for name, layer in layers.items()
        }
    return stripped


# Contracts the proof replays, with the committed scorecard each must return.
PROOF_CONTRACTS = (
    ("v1", None, ""),
    ("v2", "scored-labels-v2.json", "-scored-labels-v2"),
    ("v3", "scored-labels-v3.json", "-scored-labels-v3"),
    ("v4", "scored-labels-v4.json", "-scored-labels-v4"),
)
PROOF_SCHEMA = "gaze-outcome-counters-proof/v1"


def _card_bytes(card: Mapping[str, object]) -> bytes:
    return json.dumps(card, indent=2).encode("utf-8") + b"\n"


def prove(record_path: Path, release: str) -> dict[str, object]:
    """Rescore a committed release record under every contract.

    Every pre-existing scorecard field must come back byte for byte; only the
    outcome counters are new.
    """
    import bench_json
    import scorecard_record as record

    bench = record_path.parent
    contracts: dict[str, object] = {}
    for name, contract_file, suffix in PROOF_CONTRACTS:
        contract = (
            score.SCORED_LABEL_CONTRACT_V1 if contract_file is None
            else score.load_scored_label_contract(
                bench / contract_file,
                display_path=f"docs/reference/benchmarks/{contract_file}",
            )
        )
        card = record.rescore(record_path, contract)
        committed_path = bench / f"scorecard-{release}{suffix}.json.gz"
        committed = bench_json.read_content(committed_path)
        if has_outcome_counters(json.loads(committed)):
            raise ValueError(f"{committed_path.name} already carries outcome counters")
        contracts[name] = {
            "committed_scorecard": committed_path.name,
            "committed_scorecard_sha256": hashlib.sha256(committed).hexdigest(),
            "existing_fields_byte_identical": _card_bytes(strip_outcome_counters(card)) == committed,
            "outcome_counters": {
                run["config"]: run[OUTCOME_COUNTERS_FIELD] for run in card["runs"]
            },
        }
    return {
        "schema": PROOF_SCHEMA,
        "release": release,
        "observation_record": {
            "file": record_path.name, "sha256": score.sha256_file(record_path),
        },
        "contracts": contracts,
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("record", type=Path)
    parser.add_argument("--release", required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", type=Path)
    mode.add_argument("--check", type=Path)
    args = parser.parse_args(argv)
    evidence = prove(args.record, args.release)
    identical = all(
        item["existing_fields_byte_identical"] for item in evidence["contracts"].values()
    )
    rendered = json.dumps(evidence, indent=2) + "\n"
    if args.write is not None:
        if not identical:
            print("ERROR: rescore changed an existing scorecard field", file=sys.stderr)
            return 1
        args.write.write_text(rendered, encoding="utf-8")
        return 0
    if not identical or args.check.read_text(encoding="utf-8") != rendered:
        print(f"ERROR: {args.check} does not match a fresh rescore", file=sys.stderr)
        return 1
    print(f"{args.check}: existing fields byte-identical under "
          f"{', '.join(evidence['contracts'])}; outcome counters match")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
