"""Checks for scorer validity and failure boundaries; synthetic fixtures only."""
import copy
import json
import unittest

import generate
from model import ARMS, authorize, edit_one, simulate
from run import authority_suite, evaluate


class ExperimentTests(unittest.TestCase):
    def test_generated_bytes_and_partition_ownership(self):
        rows = generate.generate()
        self.assertEqual(len(rows), 128)
        self.assertEqual(len({r["id"] for r in rows}), 128)
        names = {s: {e["full"] for r in rows if r["split"] == s for e in r["entities"] if e["kind"] == "Name"} for s in ("dev", "test")}
        self.assertFalse(names["dev"] & names["test"])
        for r in rows:
            for t in r["turns"]:
                for g in t["gold"]:
                    self.assertEqual(t["text"].encode()[g["start"]:g["end"]].decode(), g["value"])

    def test_byte_scorer_prices_partial_coverage_and_separators(self):
        r = generate.generate()[0]
        r = copy.deepcopy(r)
        r["turns"] = [{"text":"é Ab", "seed":False, "gold":[{"start":0,"end":2,"value":"é","entity":"p1"}]}]
        p = {"turns":[{"spans":[{"start":1,"end":3,"token":"x","root":"abstain"}],"restore_ok":False}]}
        c = evaluate(r,p,"exact")
        self.assertEqual((c["covered_gold_bytes"],c["missed_pii_bytes"],c["false_positive_bytes"]),(1,1,1))
        self.assertEqual(c["restoration_errors"],1)

    def test_overlap_is_a_scorer_error(self):
        r = generate.generate()[0]
        p = simulate(r,"exact")
        p["turns"][0]["spans"].append(p["turns"][0]["spans"][0])
        with self.assertRaises(ValueError): evaluate(r,p,"exact")

    def test_authentication_and_destination_matrix(self):
        self.assertTrue(authority_suite()["all_pass"])
        self.assertEqual(authority_suite()["invalid_probe_plaintext_bytes"],0)

    def test_agent_claim_does_not_change_evidence_links(self):
        r = next(r for r in generate.generate() if r["family"] == "surname_collision")
        a = simulate(r,"evidence")
        for t in r["turns"]: t["agent_claim"] = {"entity":"p999", "trusted":True}
        self.assertEqual(a,simulate(r,"evidence"))

    def test_ambiguity_abstains_even_if_values_equal(self):
        r = next(r for r in generate.generate() if r["family"] == "identical_names")
        p = simulate(r,"structured")
        self.assertTrue(all(s["root"] == "abstain" for s in p["turns"][1]["spans"]))

    def test_occurrence_restore_and_canonical_negative_control(self):
        r = next(r for r in generate.generate() if r["family"] == "given")
        for arm in ARMS:
            p = simulate(r,arm)
            if arm == "canonical_restore":
                self.assertFalse(p["turns"][1]["restore_ok"])
            else: self.assertTrue(all(t["restore_ok"] for t in p["turns"]))

    def test_bounded_edit_distance_has_no_transitive_expansion(self):
        self.assertTrue(edit_one("Clara Ellery","Clarx Ellery"))
        self.assertFalse(edit_one("Clara Ellery","Clxxx Ellery"))


if __name__ == "__main__": unittest.main()
