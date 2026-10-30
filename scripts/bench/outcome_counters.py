"""Per-run protection outcome counters, reported beside benchmark scorecards.

The counters are a sidecar derived from an observation record, so no existing
scorecard number changes. They separate four things the byte score folds
together:

- refused operations (untransformable values, refused possibly-sensitive
  values, and every other fail-closed document), and refusals of documents
  that carry no scored gold (false-positive refusals);
- protection of confirmed identifiers versus protection of ambiguous values
  (collision-family tokens such as `custom:family:<name>`);
- false positives by document count and by affected-value count;
- how much non-PII content stays usable, counting bytes lost to false
  positives and to refusals separately, so here a refusal never reads as a gain.

    python scripts/bench/outcome_counters.py report RECORD \\
      --scored-labels docs/reference/benchmarks/scored-labels-v4.json --output counters-v4.json
    python scripts/bench/outcome_counters.py prove \\
      docs/reference/benchmarks/observations-v0.17.0.jsonl.gz \\
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

from refusal_contract import UNTRANSFORMABLE_REASONS, UNTRANSFORMABLE_CODES
UNTRANSFORMABLE_CODE_PREFIX = "untransformable_"
POSSIBLY_SENSITIVE_REFUSED_CODE = "possibly_sensitive_refused"

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

    def add(self, overlaps_gold: bool, false_positive: bool) -> None:
        # An over-extended value is both: it covers gold and protects extra bytes.
        self.values += 1
        self.gold_values += overlaps_gold
        self.false_positive_values += false_positive

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

    def add_scored(self, document: score.Document, predictions: Sequence[score.Span], outcomes=None) -> None:
        self.attempted_documents += 1
        gold, ignored, scored = score.contract_scoring_view(document, predictions)
        false_positive_kinds = set()
        for span in scored:
            overlaps_gold = score.interval_overlaps((span.start, span.end), gold)
            # Each span is judged by its own non-gold bytes, so a value that
            # runs past its gold counts as a false positive too.
            false_positive = _false_positive_bytes([(span.start, span.end)], gold, ignored) > 0
            typed = (outcomes or {}).get((span.start, span.end, span.label))
            ambiguous = (isinstance(typed, dict) and "possibly_sensitive" in typed
                         if typed is not None else is_ambiguous_class(span.label))
            (self.possibly_sensitive if ambiguous else self.confirmed).add(
                overlaps_gold, false_positive
            )
            if false_positive:
                false_positive_kinds.add(ambiguous)
        false_positive_bytes = _false_positive_bytes(
            [(span.start, span.end) for span in scored], gold, ignored
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
                        "protect_values": self.possibly_sensitive.values,
                        "refuse_documents": self.blocked["possibly_sensitive"],
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


def _false_positive_bytes(
    intervals: Sequence[tuple[int, int]],
    gold: Sequence[tuple[int, int]],
    ignored: Sequence[tuple[int, int]],
) -> int:
    predicted = score.subtract_intervals(score.merge_intervals(intervals), ignored)
    return score.interval_length(predicted) - score.intersection_length(gold, predicted)


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


def group_counters(
    documents: Sequence[score.Document], responses: Mapping[str, Mapping[str, object]]
) -> dict[str, object]:
    """Counters for one run from contract-applied documents and their responses."""
    outcomes = OutcomeAccumulator()
    for document in documents:
        response = responses[document.uid]
        if "pipeline_error_code" in response:
            outcomes.add_refused(document, str(response["pipeline_error_code"]))
        else:
            outcomes.add_scored(document, score.final_trace_predictions(document, response), {
                (item["raw_start"], item["raw_end"], item["class"]): item["protection_outcome"]
                for item in response["final_protection_trace"] if "protection_outcome" in item
            })
    return outcomes.result()


REPORT_SCHEMA = "gaze-outcome-counters/v1"


def report(
    record_path: Path,
    contract: score.ScoredLabelContract,
    layer_contract: score.ScoredLabelContract | None = None,
) -> dict[str, object]:
    """Counters for every run in an observation record, under one contract.

    A sidecar to the contract scorecard from the same record. Scorecards stay
    untouched: certification binds a capture byte-for-byte to its replay, and
    `gaze_bench_score.py` is digest-pinned by the published competitor rows.
    """
    import scorecard_record as record

    header, observations = record._read(record_path)
    card = header["scorecard"]
    documents = {row["id"]: record._document_from_row(row) for row in header["documents"]}
    groups: dict[tuple[str, str], list[Mapping[str, object]]] = {}
    for row in observations:
        groups.setdefault((row["layer"], row["config"]), []).append(row)

    def counters(layer: str, config: str, selected: score.ScoredLabelContract) -> dict[str, object]:
        group = groups[(layer, config)]
        scored = score.apply_scored_label_contract(
            [documents[row["document_id"]] for row in group], selected
        )
        return {"config": config, **group_counters(
            scored, {row["document_id"]: row["response"] for row in group}
        )}

    result: dict[str, object] = {
        "schema": REPORT_SCHEMA,
        "observation_record": {"file": record_path.name, "sha256": score.sha256_file(record_path)},
        "scored_label_contract": {"id": contract.contract_id, "version": contract.version},
        "runs": [counters("C", run["config"], contract) for run in card.get("runs", [])],
    }
    if "layers" in card:
        layer_contract = layer_contract or record._contract_from_row(header.get("layer_contract"))
        result["layers"] = {
            layer: [counters(layer, run["config"], layer_contract)
                    for run in card["layers"][layer]["runs"]]
            for layer in ("A", "D", "R")
        }
    return result


# Contracts the proof replays, with the committed scorecard each must return.
PROOF_CONTRACTS = (
    ("v1", None, ""),
    ("v2", "scored-labels-v2.json", "-scored-labels-v2"),
    ("v3", "scored-labels-v3.json", "-scored-labels-v3"),
    ("v4", "scored-labels-v4.json", "-scored-labels-v4"),
)
PROOF_SCHEMA = "gaze-outcome-counters-proof/v1"


def _load_contract(bench: Path, contract_file: str | None) -> score.ScoredLabelContract:
    if contract_file is None:
        return score.SCORED_LABEL_CONTRACT_V1
    return score.load_scored_label_contract(
        bench / contract_file, display_path=f"docs/reference/benchmarks/{contract_file}"
    )


def prove(record_path: Path, release: str) -> dict[str, object]:
    """Rescore a committed release record and count its outcomes per contract.

    Every committed scorecard must still come back byte for byte, so adding the
    counters changed no existing number.
    """
    import bench_json
    import scorecard_record as record

    bench = record_path.parent
    contracts: dict[str, object] = {}
    for name, contract_file, suffix in PROOF_CONTRACTS:
        contract = _load_contract(bench, contract_file)
        card = record.rescore(record_path, contract)
        committed_path = bench / f"scorecard-{release}{suffix}.json.gz"
        committed = bench_json.read_content(committed_path)
        replayed = json.dumps(card, indent=2).encode("utf-8") + b"\n"
        contracts[name] = {
            "committed_scorecard": committed_path.name,
            "committed_scorecard_sha256": hashlib.sha256(committed).hexdigest(),
            "scorecard_byte_identical": replayed == committed,
            "outcome_counters": report(record_path, contract)["runs"],
        }
    evidence: dict[str, object] = {
        "schema": PROOF_SCHEMA,
        "release": release,
        "observation_record": {
            "file": record_path.name, "sha256": score.sha256_file(record_path),
        },
        "contracts": contracts,
    }
    # Generated layers score under the record's own layer contract, whichever
    # contract the C runs use, so they are pinned once.
    counted = report(record_path, score.SCORED_LABEL_CONTRACT_V1)
    if "layers" in counted:
        evidence["layers"] = counted["layers"]
    return evidence


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    counted = commands.add_parser("report", help="write the counters for one contract")
    counted.add_argument("record", type=Path)
    counted.add_argument("--scored-labels", type=Path)
    counted.add_argument("--output", type=Path, required=True)
    proved = commands.add_parser("prove", help="pin or check the committed-record proof")
    proved.add_argument("record", type=Path)
    proved.add_argument("--release", required=True)
    mode = proved.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", type=Path)
    mode.add_argument("--check", type=Path)
    args = parser.parse_args(argv)

    if args.command == "report":
        contract = (
            score.load_scored_label_contract(args.scored_labels)
            if args.scored_labels else score.SCORED_LABEL_CONTRACT_V1
        )
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report(args.record, contract), indent=2) + "\n",
                               encoding="utf-8")
        return 0

    evidence = prove(args.record, args.release)
    identical = all(item["scorecard_byte_identical"] for item in evidence["contracts"].values())
    rendered = json.dumps(evidence, indent=2) + "\n"
    if args.write is not None:
        if not identical:
            print("ERROR: rescore no longer returns a committed scorecard", file=sys.stderr)
            return 1
        args.write.write_text(rendered, encoding="utf-8")
        return 0
    if not identical or args.check.read_text(encoding="utf-8") != rendered:
        print(f"ERROR: {args.check} does not match a fresh rescore", file=sys.stderr)
        return 1
    print(f"{args.check}: committed scorecards byte-identical under "
          f"{', '.join(evidence['contracts'])}; outcome counters match")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
