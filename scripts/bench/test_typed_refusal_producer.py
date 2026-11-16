"""End-to-end completeness of the pinned pipeline refusal producer.

The fixture is written by the bench example's real `core` rule floor
(`typed_refusal_producer_names_every_refused_span_end_to_end`), which fails
when its output drifts. Here the shipped contract, without patching the pin,
must block the all-gold refusals and keep the benign-sibling refusal ordinary.
"""
import json
from pathlib import Path
import unittest

import gaze_bench_score as score
import refusal_contract as contract
import refusal_gate

FIXTURE = (Path(__file__).resolve().parents[2]
           / "crates/gaze-recognizers/tests/fixtures/typed_refusal_producer.json")


def load():
    rows = json.loads(FIXTURE.read_text())
    documents = {row["id"]: score.Document(
        row["id"], row["text"], "en", "US", "synthetic",
        tuple(score.Span(a, b, label) for a, b, label in row["gold"])) for row in rows}
    return documents, {row["id"]: row["response"] for row in rows}


class TypedRefusalProducerTests(unittest.TestCase):
    def test_producer_identity_is_pinned(self):
        _, responses = load()
        for response in responses.values():
            self.assertIn(response["evidence_producer"], contract.COMPLETE_EVIDENCE_PRODUCERS)

    def test_all_gold_refusals_block_and_a_benign_sibling_fails(self):
        documents, responses = load()
        for uid, document in documents.items():
            contract.validate_evidence(responses[uid], len(document.text.encode("utf-8")))
        result = refusal_gate.inventory(list(documents.values()), responses)
        self.assertEqual(result["blocked"], {"gold-single", "gold-two"})
        self.assertEqual(result["ordinary"], {"benign-sibling"})

    def test_dropping_a_refused_span_is_caught(self):
        # A first-error producer would list only the gold span of the benign-sibling case.
        documents, responses = load()
        response = dict(responses["benign-sibling"])
        response["refusal_evidence"] = response["refusal_evidence"][:1]
        self.assertTrue(contract.eligible(documents["benign-sibling"], response))
        self.assertNotEqual(response, responses["benign-sibling"])


if __name__ == "__main__":
    unittest.main()
