"""Refusal contract v2: compare the same completed population on both sides.

Blocked gold operations are reported separately. Removing their base counterpart
from byte/restore scoring prevents refusal from manufacturing a leak gain.
"""
import copy
from collections import Counter

import gaze_bench_score as score
import refusal_contract as refusal
import scorecard_record as records


def inventory(documents, responses):
    blocked, false_refusals, ordinary = set(), set(), set()
    benign = sum(not d.spans for d in documents)
    blocked_reasons = Counter()
    ordinary_reasons = Counter()
    for document in documents:
        response = responses[document.uid]
        if "pipeline_error_code" not in response:
            continue
        records.validate_refusal(response, document)
        if refusal.eligible(document, response):
            blocked.add(document.uid)
            blocked_reasons[response["pipeline_error_code"].removeprefix("untransformable_")] += 1
        else:
            ordinary.add(document.uid)
            ordinary_reasons[response["pipeline_error_code"]] += 1
            evidence = response.get("refusal_evidence") or ()
            gold = {(s.start, s.end) for s in document.spans}
            if not gold or any((s["raw_start"], s["raw_end"]) not in gold for s in evidence):
                false_refusals.add(document.uid)
    return {"blocked": blocked, "false_refusals": false_refusals, "ordinary": ordinary,
            **refusal.rates(len(documents), len(blocked), benign,
                            len(false_refusals & {d.uid for d in documents if not d.spans})),
            "false_refusal_documents": len(false_refusals),
            "ordinary_refusal_documents": len(ordinary),
            "blocked_operations_by_reason": dict(sorted(blocked_reasons.items())),
            "ordinary_refusals_by_reason": dict(sorted(ordinary_reasons.items()))}


def compare(cards, paths, config, decide, layer_totals, *, gold_verdict_reference=None):
    loaded = []
    for card, path in zip(cards, paths, strict=True):
        pointer = card.get("observation_record")
        if not pointer or pointer.get("sha256") != score.sha256_file(path):
            raise records.RecordError("refusal gate requires scorecard-bound records")
        header, rows = records._read(path)
        records.validate_replay_adapter(header, rows)
        if gold_verdict_reference is not None:
            reference, _ = records._read(gold_verdict_reference)
            records.freeze_gold_header(header, reference)
        selected_contract = _corpus_contract(card)
        replay = records.rescore(path, selected_contract, max_workers=1,
                                  gold_verdict_reference=gold_verdict_reference)
        if layer_totals(replay, config) != layer_totals(card, config):
            raise records.RecordError("refusal gate scorecard differs from record replay")
        loaded.append((header, rows))
    views = [copy.deepcopy(c) for c in cards]
    reports = {}
    for layer in ("C", "A", "D", "R"):
        sides = []
        for card, (header, rows) in zip(cards, loaded, strict=True):
            selected = [r for r in rows if r["layer"] == layer and r["config"] == config]
            descriptors = {d["id"]: d for d in header["documents"] if d["layer"] == layer}
            documents = [records._document_from_row(descriptors[r["document_id"]]) for r in selected]
            # Reuse the scored contract carried by the admitted card, including exclusions.
            contract = records._contract_from_row(header.get("layer_contract")) if layer != "C" else _corpus_contract(card)
            documents = score.apply_scored_label_contract(documents, contract)
            responses = {r["document_id"]: r["response"] for r in selected}
            sides.append((inventory(documents, responses), selected, descriptors, contract))
        if {r["document_id"] for r in sides[0][1]} != {r["document_id"] for r in sides[1][1]}:
            raise records.RecordError("refusal gate attempted populations differ")
        # Gold ownership and input identities must agree independently of the subject.
        if sides[0][2] != sides[1][2]:
            raise records.RecordError("refusal gate document inventories differ")
        excluded = sides[0][0]["blocked"] | sides[1][0]["blocked"]
        # An ordinary refusal at an operation blocked on the other side must
        # remain a failure, rather than disappearing with that operation.
        concealed = (sides[0][0]["ordinary"] | sides[1][0]["ordinary"]) & excluded
        if concealed:
            raise records.RecordError("ordinary refusal cannot inherit blocked-operation exclusion")
        reports[layer] = {
            side: {k: v for k, v in data[0].items() if k not in {"blocked", "false_refusals", "ordinary"}}
            for side, data in zip(("base", "candidate"), sides, strict=True)
        }
        reports[layer]["excluded_from_both_byte_and_restore_denominators"] = len(excluded)
        # Completing a formerly blocked operation must not hide a fresh leak
        # or a broken restore/manifest behind the common-population exclusion.
        candidate_responses = {r["document_id"]: r["response"] for r in sides[1][1]}
        unblocked_failures = 0
        for uid in sides[0][0]["blocked"] - sides[1][0]["blocked"]:
            document = score.apply_scored_label_contract(
                [records._document_from_row(sides[1][2][uid])], sides[1][3])[0]
            response = candidate_responses[uid]
            metrics = score.MetricAccumulator()
            metrics.add(document, score.final_trace_predictions(document, response))
            integrity = score.ContractAccumulator()
            integrity.add(response)
            valid = integrity.result()
            unblocked_failures += bool(metrics.result()["utf8_bytes"]["leaked"]
                                       or valid["restore_exact_documents"] != 1
                                       or valid["manifest_valid_documents"] != 1)
        reports[layer]["unblocked_candidate_failures"] = unblocked_failures
        for view, (header, _), (counts, rows, descriptors, contract) in zip(views, loaded, sides, strict=True):
            if not excluded:
                continue
            rows = [r for r in rows if r["document_id"] not in excluded]
            if not rows:
                raise records.RecordError("refusal gate has no completed comparison population")
            run_list = view["runs"] if layer == "C" else view["layers"][layer]["runs"]
            index = next(i for i, run in enumerate(run_list) if run["config"] == config)
            old = run_list[index]
            for key in ("scored_population", "failed_closed_population"):
                old[key]["document_ids"] = [uid for uid in old[key]["document_ids"] if uid not in excluded]
            _, run_list[index] = records._replay_group((header, view, descriptors, rows, layer, index, config, contract))
    result = decide(*(layer_totals(v, config) for v in views))
    # False refusals fail outright, including a benign sibling in a mixed document.
    bad = [layer for layer, r in reports.items() if r["candidate"]["false_refusal_documents"]]
    if bad:
        result.update(verdict="fail", reason=f"benign/non-gold refusals in {bad}")
    unsafe_unblocks = [layer for layer, r in reports.items() if r["unblocked_candidate_failures"]]
    if unsafe_unblocks:
        result.update(verdict="fail", reason=f"unblocked operations leak or fail integrity in {unsafe_unblocks}")
    return {**result, "refusal_contract_version": refusal.VERSION, "operation_outcomes": reports}


def _corpus_contract(card):
    value = card.get("scoring", {}).get("scored_label_contract")
    if value is None or value.get("version") == 1:
        return score.SCORED_LABEL_CONTRACT_V1
    # Only committed contracts are admitted. Never trust a card-supplied path.
    from pathlib import Path
    path = Path(__file__).resolve().parents[2] / "docs/reference/benchmarks" / f"scored-labels-v{value['version']}.json"
    contract = score.load_scored_label_contract(path)
    if contract.contract_id != value["id"]:
        raise records.RecordError("refusal gate scored contract mismatch")
    return contract
