"""Behavioral oracles for typed refusals, including mixed gold/benign values."""
import json
from pathlib import Path
import unittest
import tempfile
from unittest import mock

import gaze_bench_score as score
import outcome_counters
import refusal_contract as contract
import refusal_gate
import scorecard_record as record
import agentic_layers as agentic
from test_agentic_layers import _success_response


def document(uid="d", gold=True):
    return score.Document(uid, '{"customer_id":812345,"limit":7}', "en", "US", "synthetic",
                          (score.Span(15, 21, "CUSTOMER_ID"),) if gold else ())


# Stands in for a producer that has landed with its end-to-end completeness test.
TESTED_PRODUCER = "synthetic-complete-evidence-producer"


def pin_tested_producer(test):
    patcher = mock.patch.object(contract, "COMPLETE_EVIDENCE_PRODUCERS", frozenset({TESTED_PRODUCER}))
    patcher.start()
    test.addCleanup(patcher.stop)


def refusal(spans=((15, 21),), code="untransformable_json_number_no_text_only_path"):
    return {"fixture_id": "d", "pipeline_error_stage": "clean", "pipeline_error_code": code,
            "timing": {"total_ms": 1.0}, "evidence_complete": True,
            "evidence_producer": TESTED_PRODUCER, "refusal_evidence": [
                {"raw_start": a, "raw_end": b, "outcome": "confirmed_untransformable",
                 "reason": code.removeprefix("untransformable_")} for a, b in spans]}


class ShippedProducerPinTests(unittest.TestCase):
    def test_shipped_pin_is_empty_and_matches_the_protocol(self):
        # No producer has an end-to-end completeness test yet, so a subject
        # asserting completeness, under any producer identity, is ordinary.
        self.assertEqual(contract.COMPLETE_EVIDENCE_PRODUCERS, frozenset())
        self.assertFalse(contract.eligible(document(), refusal()))
        self.assertEqual(refusal_gate.inventory([document()], {"d": refusal()})["ordinary"], {"d"})
        protocol = json.loads((Path(__file__).parent / "benchmark-protocol.json").read_text())
        self.assertEqual(protocol["refusal_contract"]["complete_evidence_producers"],
                         sorted(contract.COMPLETE_EVIDENCE_PRODUCERS))


class RefusalContractTests(unittest.TestCase):
    def setUp(self):
        pin_tested_producer(self)

    def test_complete_evidence_from_an_unpinned_producer_is_ordinary(self):
        for producer in (None, "clean_for_bench", TESTED_PRODUCER + "-other"):
            response = refusal()
            if producer is None:
                response.pop("evidence_producer")
            else:
                response["evidence_producer"] = producer
            result = refusal_gate.inventory([document()], {"d": response})
            self.assertEqual((result["blocked"], result["ordinary"]), (set(), {"d"}), producer)

    def test_first_error_with_unlisted_benign_sibling_is_ordinary(self):
        # The sibling limit is untransformable too; a first-error path cannot
        # prove that every refused value belongs to confirmed-ID gold.
        for complete in (None, False):
            response = refusal()
            response.pop("evidence_complete")
            if complete is not None:
                response["evidence_complete"] = complete
            result = refusal_gate.inventory([document()], {"d": response})
            self.assertEqual(result["blocked"], set())
            self.assertEqual(result["ordinary"], {"d"})

    def test_encoded_carrier_cannot_borrow_an_outer_node_path(self):
        response = refusal(code="untransformable_encoded_carrier")
        d = document()
        d = score.Document(**{**d.__dict__, "text":
            r'{"customer_id":812345,"payload":"{\"customer_id\":812345}"}'})
        self.assertFalse(contract.eligible(d, response))

    def test_zero_refusals_do_not_open_optional_records(self):
        totals = {layer: {"failed_closed": 0} for layer in ("C", "A", "D", "R")}
        expected = {"verdict": "pass", "historical": True}
        with mock.patch.object(record, "_read", side_effect=AssertionError("optional record opened")):
            result = refusal_gate.compare(({}, {}), (Path("missing"), Path("broken")),
                                          "test", lambda *args: expected, lambda *args: totals)
        self.assertEqual(result, expected)

    def test_gold_labelled_typed_refusal_is_blocked_operation(self):
        result = refusal_gate.inventory([document()], {"d": refusal()})
        self.assertEqual(result["blocked"], {"d"})
        self.assertEqual(result["ordinary"], set())
        self.assertEqual(result["blocked_operation_rate"], {"numerator": 1, "denominator": 1, "rate": 1.0})

    def test_benign_refusal_fails_even_with_valid_typed_reason(self):
        result = refusal_gate.inventory([document(gold=False)], {"d": refusal()})
        self.assertEqual(result["blocked"], set())
        self.assertEqual(result["false_refusals"], {"d"})
        self.assertEqual(result["false_refusal_rate"], {"numerator": 1, "denominator": 1, "rate": 1.0})

    def test_mixed_gold_and_benign_refusal_spans_fail(self):
        result = refusal_gate.inventory([document()], {"d": refusal(((15, 21), (30, 31)))})
        self.assertEqual(result["blocked"], set())
        self.assertEqual(result["false_refusals"], {"d"})

    def test_missing_evidence_and_untyped_errors_never_get_exception(self):
        for response in (refusal(), {"fixture_id": "d", "pipeline_error_stage": "clean",
                                    "pipeline_error_code": "policy"}):
            response.pop("refusal_evidence", None)
            response.pop("evidence_complete", None)
            self.assertFalse(contract.eligible(document(), response))

    def test_contextual_gold_is_not_confirmed_id_gold(self):
        d = document()
        d = score.Document(**{**d.__dict__, "spans": (score.Span(15, 21, "AGE"),)})
        self.assertFalse(contract.eligible(d, refusal()))

    def test_partial_or_overextended_id_never_gets_exception(self):
        for span in ((15, 20), (14, 21), (15, 22)):
            self.assertFalse(contract.eligible(document(), refusal((span,))))

    def test_live_and_replay_accept_same_closed_codes_and_evidence(self):
        for code in contract.UNTRANSFORMABLE_CODES:
            response = refusal(code=code)
            score.validate_response(document(), response)
            compact = record._compact_response(response)
            self.assertEqual(compact["refusal_evidence"], response["refusal_evidence"])
            self.assertIs(compact["evidence_complete"], True)
            self.assertEqual(compact["evidence_producer"], TESTED_PRODUCER)
            record.validate_refusal(compact, document())
        for code in contract.PROTECTION_CODES | {"possibly_sensitive_refused"}:
            response = refusal(code=code)
            response.pop("refusal_evidence")
            response.pop("evidence_complete")
            score.validate_response(document(), response)
            record.validate_refusal(record._compact_response(response), document())

    def test_live_and_replay_reject_malformed_or_unknown_evidence(self):
        for mutate in (
            lambda r: r.update(pipeline_error_code="untransformable_invented"),
            lambda r: r["refusal_evidence"][0].update(reason="encoded_carrier"),
            lambda r: r["refusal_evidence"][0].update(raw_end=10000),
            lambda r: r["refusal_evidence"][0].update(raw_start=True),
            lambda r: r["refusal_evidence"][0].update(outcome="possibly_sensitive"),
            lambda r: r["refusal_evidence"][0].update(value="forbidden"),
            lambda r: r.update(refusal_evidence=[]),
            lambda r: r.update(evidence_complete=1),
            lambda r: r.update(evidence_complete="true"),
            lambda r: r.update(evidence_producer=1),
            lambda r: r.update(evidence_producer=""),
            lambda r: r.pop("refusal_evidence"),
        ):
            response = refusal()
            mutate(response)
            with self.assertRaises((ValueError, score.ResponseValidationError)):
                score.validate_response(document(), response)
            with self.assertRaises(record.RecordError):
                record.validate_refusal(record._compact_response(response), document())

    def test_typed_possible_outcome_round_trips_and_overrides_class_heuristic(self):
        d = document()
        item = {"raw_start": 15, "raw_end": 21, "class": "custom:customer_id", "action": "tokenize",
                "protection_outcome": {"possibly_sensitive": "label_value_shape_unclear"},
                "provenance": {"stage": "primary_pipeline", "decision": "policy", "source_ids": ["customer.id"]}}
        # Validate final evidence directly; the matching manifest is independent of its outcome.
        manifest = [{"raw_start": 15, "raw_end": 21, "clean_start": 15, "clean_end": 21,
                     "class": "custom:customer_id"}]
        score._validate_final_protection_trace(d, [item], manifest)
        response = _success_response(d)
        response["final_protection_trace"] = [item]
        response["manifest_spans"] = manifest
        compact = record._compact_response(response)
        self.assertEqual(compact["final_protection_trace"][0]["protection_outcome"], item["protection_outcome"])
        counts = outcome_counters.group_counters([d], {"d": compact})
        self.assertEqual(counts["protection"]["confirmed"]["values"], 0)
        self.assertEqual(counts["protection"]["possibly_sensitive"]["values"], 1)
        item["protection_outcome"] = "protected"
        item["class"] = manifest[0]["class"] = "custom:family:customer"
        counts = outcome_counters.group_counters([d], {"d": response})
        self.assertEqual(counts["protection"]["confirmed"]["values"], 1)
        self.assertEqual(counts["protection"]["possibly_sensitive"]["values"], 0)

    def test_unknown_possible_reason_is_rejected(self):
        with self.assertRaises(ValueError):
            contract.validate_outcome({"possibly_sensitive": "invented"})

    def test_retained_record_rejects_tampered_refusal_evidence_without_legacy_adapter(self):
        d = document()
        header = {"kind": "header", "schema_version": 1, "corpus_sha256": "0" * 64,
                  "scorecard": {"runs": [{}], "dataset": {"integrity": {"sha256": "0" * 64}}},
                  "documents": [{**record._document_row(d), "layer": "C"}], "layer_identity": None}
        row = {"kind": "observation", "layer": "C", "config": "test", "document_id": "d",
               "response": record._compact_response(refusal())}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "record.jsonl.gz"
            record._write_rows(path, [header, row])
            _, retained = record._read(path)
            self.assertEqual(retained[0]["response"]["refusal_evidence"], row["response"]["refusal_evidence"])
            row["response"]["refusal_evidence"][0]["reason"] = "encoded_carrier"
            record._write_rows(path, [header, row])
            with self.assertRaises(record.RecordError):
                record._read(path)


class PairedGateTests(unittest.TestCase):
    def setUp(self):
        pin_tested_producer(self)

    def compare(self, *, benign=False, independent_gain=True, refuse_base=False, refuse_candidate=True,
                ordinary_base=False, incomplete_candidate=False, change_gold=False):
        """Real replay and gate arithmetic over small independently authored records."""
        cards, headers, observations = [], [], []
        for side in (0, 1):
            card = {"runs": [], "layers": {}, "scoring": {}, "parameters": {"ner_threshold": 0.5},
                    "observation_record": {"sha256": str(side)}}
            header = {"documents": [], "validator": {}, "layer_contract": record._contract_row(score.SCORED_LABEL_CONTRACT_V1)}
            rows = []
            for layer in ("C", "A", "D", "R"):
                first = document(layer + "-block", gold=not benign)
                other = document(layer + "-other")
                docs = [first, other]
                responses = {d.uid: _success_response(d) for d in docs}
                if layer == "A" and ((side and refuse_candidate) or (not side and refuse_base)):
                    responses[first.uid] = {**refusal(), "fixture_id": first.uid}
                    if (not side and ordinary_base) or (side and incomplete_candidate):
                        responses[first.uid].pop("evidence_complete")
                if layer == "C" and side and independent_gain:
                    responses[other.uid]["final_protection_trace"] = [{
                        "raw_start": 15, "raw_end": 21, "class": "custom:customer_id", "action": "tokenize",
                        "provenance": {"stage": "primary_pipeline", "decision": "policy", "source_ids": ["customer.id"]}}]
                for d in docs:
                    descriptor = {**record._document_row(d), "layer": layer}
                    if d == first:
                        descriptor["validator"] = {"subject_verdict": side}
                    if side and change_gold and layer == "A" and d == first:
                        descriptor["gold"] = []
                    header["documents"].append(descriptor)
                    rows.append({"layer": layer, "config": "test", "document_id": d.uid,
                                 "response": record._compact_response(responses[d.uid])})
                run = score.run_config(Path('.'), Path('.'), 'test', docs, Path('.'), None, None, None,
                                       0.5, Path('.'), replay_responses=responses)
                if layer == "C":
                    card["runs"] = [run]
                else:
                    card["layers"][layer] = {"runs": [run]}
            header["scorecard"] = card
            cards.append(card)
            headers.append(header)
            observations.append(rows)

        def totals(card, config):
            result = {}
            for layer in ("C", "A", "D", "R"):
                run = (card["runs"] if layer == "C" else card["layers"][layer]["runs"])[0]
                counts = run["pipeline_contract"]
                result[layer] = {"attempted": run["pipeline_availability"]["attempted_documents"],
                                 "documents": counts["documents"],
                                 "headline_leaked": run["metrics"]["utf8_bytes"]["leaked"],
                                 "leaked": run["metrics"]["utf8_bytes"]["leaked"], "twin_leaked": 0,
                                 "false_positive": run["metrics"]["utf8_bytes"]["false_positive"],
                                 "failed_closed": run["pipeline_availability"]["failed_closed_documents"],
                                 "restore_exact": counts["restore_exact_documents"],
                                 "manifest_valid": counts["manifest_valid_documents"],
                                 "guard_false_positive": {f: 0 for fs in agentic.CREDIT_GUARD_FAMILIES.values() for f in fs}}
            return result

        with (mock.patch.object(score, "sha256_file", side_effect=lambda p: str(p)),
              mock.patch.object(record, "_read", side_effect=lambda p: (headers[int(str(p))], observations[int(str(p))])),
              mock.patch.object(record, "rescore", side_effect=lambda p, *a, **k: cards[int(str(p))])):
            return refusal_gate.compare(cards, (Path('0'), Path('1')), 'test', agentic.decide, totals)

    def test_gold_block_allowed_with_independent_gain_and_common_restore_denominator(self):
        result = self.compare()
        self.assertEqual(result["verdict"], "pass", result)
        self.assertEqual(result["summary"]["leaked_bytes_decrease"], 6)
        row = result["layers"]["A"]
        self.assertEqual(row["documents_base"], 1)
        self.assertEqual(row["restore_exact_base"], row["restore_exact_candidate"])
        self.assertEqual(result["operation_outcomes"]["A"]["candidate"]["blocked_operation_rate"],
                         {"numerator": 1, "denominator": 2, "rate": 0.5})

    def test_unblocking_must_not_hide_a_leak(self):
        result = self.compare(refuse_base=True, refuse_candidate=False)
        self.assertEqual(result["verdict"], "fail")
        self.assertIn("unblocked operations leak", result["reason"])

    def test_refusal_alone_is_never_a_gain(self):
        result = self.compare(independent_gain=False)
        self.assertEqual(result["verdict"], "fail")
        self.assertEqual(result["summary"]["leaked_bytes_decrease"], 0)

    def test_base_ordinary_refusal_cannot_inherit_candidate_block(self):
        with self.assertRaisesRegex(record.RecordError, "ordinary refusal cannot inherit"):
            self.compare(refuse_base=True, ordinary_base=True)

    def test_first_error_evidence_cannot_turn_a_refusal_into_a_gain(self):
        result = self.compare(incomplete_candidate=True)
        self.assertEqual(result["verdict"], "fail")

    def test_complete_evidence_from_an_unpinned_producer_cannot_become_a_gain(self):
        with mock.patch.object(contract, "COMPLETE_EVIDENCE_PRODUCERS", frozenset()):
            result = self.compare()
        self.assertEqual(result["verdict"], "fail")

    def test_gold_identity_changes_remain_rejected(self):
        with self.assertRaisesRegex(record.RecordError, "document inventories differ"):
            self.compare(change_gold=True)

    def test_benign_refusals_fail_even_when_unchanged_and_another_layer_improves(self):
        result = self.compare(benign=True, refuse_base=True)
        self.assertEqual(result["verdict"], "fail")
        self.assertIn("benign/non-gold", result["reason"])


class RefusalMutationTests(unittest.TestCase):
    def test_benign_refusal_guard_mutant_is_killed(self):
        from check_refusal_gate_mutation import check
        self.assertEqual(check(), {"baseline": "pass", "disabled_benign_refusal_guard": "killed",
                                   "disabled_concealed_refusal_guard": "killed"})


if __name__ == "__main__":
    unittest.main()
