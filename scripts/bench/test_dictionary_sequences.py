"""Independent generator invariants and canonical gate regression guards."""
import copy
import json
import unittest

import agentic_layers as gate
import dictionary_sequences as sequence
from test_agentic_layers import _scorecard


def card(leak=100):
    result = _scorecard({"C": 10, "A": 10, "D": 0, "R": 10})
    result["gaze"] = {"revision": "runtime"}
    extension = {
        "identity": sequence.identity(sequence.generate(), sequence.SEED, "published"),
        "contract": "a", "runtime": result["gaze"],
        "binary_sha256": "b" * 64, "runs": {}, "evidence": {},
    }
    for mode in sequence.SEQUENCES:
        for layer in sequence.LAYERS:
            key = f"{mode}/{layer}"
            run = copy.deepcopy(result["runs"][0])
            run["config"] = "rule-floor-core"
            run["metrics"]["utf8_bytes"] = {"leaked": leak if mode == "switch" and layer == "K" else 0,
                                           "false_positive": 0}
            extension["runs"][key] = run
            extension["evidence"][key] = {"sha256": "c" * 64, "observations": 10}
    result["dictionary_sequence"] = extension
    return result


class GeneratorTests(unittest.TestCase):
    def test_seed_and_partition_are_reproducible_and_independent(self):
        cases = sequence.generate()
        self.assertEqual(cases, sequence.generate())
        other = sequence.generate(partition="calibration")
        self.assertFalse({t.casefold() for c in cases for t in c.terms} &
                         {t.casefold() for c in other for t in c.terms})
        self.assertNotEqual(cases, sequence.generate(seed=sequence.SEED + 1))

    def test_gold_is_placed_in_utf8_and_benign_twins_are_not_terms(self):
        cases = sequence.generate()
        self.assertEqual(sum(c.layer == "K" for c in cases), sum(c.layer == "B" for c in cases))
        for case in cases:
            doc = case.document
            raw = doc.text.encode()
            for span in doc.spans:
                self.assertIn(raw[span.start:span.end].decode().casefold(),
                              {t.casefold() for t in case.terms})
            if case.layer == "B":
                self.assertFalse(doc.spans)
            self.assertTrue(all(raw[s.start:s.end].decode() in case.terms for s in case.literal_spans))
            self.assertEqual(list(json.loads(case.context())["dictionaries"]), ["sequence"])


class ExtensionGateTests(unittest.TestCase):
    def test_transition_improvement_passes_and_neutral_fails(self):
        self.assertEqual(gate.gate(card(), card(0))["verdict"], "pass")
        self.assertEqual(gate.gate(card(), card())["verdict"], "fail")

    def test_missing_or_changed_extension_is_not_comparable(self):
        base = card()
        for reverse in (False, True):
            missing = copy.deepcopy(base)
            del missing["dictionary_sequence"]
            pair = (missing, base) if reverse else (base, missing)
            self.assertEqual(gate.gate(*pair)["verdict"], "not_comparable")
        changed = card(0)
        changed["dictionary_sequence"]["identity"]["seed"] += 1
        self.assertEqual(gate.gate(base, changed)["verdict"], "not_comparable")

    def test_bad_evidence_and_runtime_are_rejected(self):
        for mutation in ("population", "runtime", "run"):
            changed = card()
            if mutation == "population":
                changed["dictionary_sequence"]["evidence"]["switch/K"]["observations"] = 0
            elif mutation == "runtime":
                changed["dictionary_sequence"]["runtime"] = {}
            else:
                del changed["dictionary_sequence"]["runs"]["fresh/K"]
            with self.assertRaises(gate.LayerError):
                gate.gate(card(), changed)

    def test_all_extension_correctness_regressions_fail_despite_leak_gain(self):
        for field in ("leaked", "false_positive", "refusal", "restore", "manifest"):
            changed = card(0)
            run = changed["dictionary_sequence"]["runs"]["switch/B"]
            if field in ("leaked", "false_positive"):
                run["metrics"]["utf8_bytes"][field] = 200
            elif field == "refusal":
                run["pipeline_availability"].update(completed_documents=9, failed_closed_documents=1)
                run["pipeline_contract"].update(documents=9, restore_exact_documents=9,
                                                manifest_valid_documents=9)
            else:
                key = "restore_exact_documents" if field == "restore" else "manifest_valid_documents"
                run["pipeline_contract"][key] -= 1
            self.assertEqual(gate.gate(card(), changed)["verdict"], "fail", field)

    def test_control_changes_cannot_supply_or_hide_gain(self):
        changed = card(0)
        changed["dictionary_sequence"]["runs"]["fresh/K"]["metrics"]["utf8_bytes"]["leaked"] = 1
        self.assertEqual(gate.gate(card(), changed)["verdict"], "not_comparable")


if __name__ == "__main__":
    unittest.main()
