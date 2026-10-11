"""Independent generator invariants and canonical gate regression guards."""
import copy
import json
import gzip
import shutil
import tempfile
import unittest
from pathlib import Path

import agentic_layers as gate
import dictionary_sequences as sequence
import gaze_bench_score as score
from test_agentic_layers import _scorecard


def card(leak=100):
    result = _scorecard({"C": 10, "A": 10, "D": 0, "R": 10})
    result["gaze"] = {"revision": "runtime"}
    extension = {
        "identity": sequence.identity(sequence.generate(), sequence.SEED, "published"),
        "contract": "a", "runtime": result["gaze"],
        "binary_sha256": "b" * 64, "runs": {}, "evidence": {},
        "producer_binding": {"fresh_locked_build": True, "toolchain": "rustc 1.96.0 test",
                             "profile": "debug", "example_source_sha256": "d" * 64,
                             "cargo_lock_sha256": "e" * 64},
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
    def test_every_required_contract_covers_the_generated_labels(self):
        docs = sequence.documents(sequence.generate(), "switch")
        for version in (1, 2, 4):
            contract = (score.SCORED_LABEL_CONTRACT_V1 if version == 1 else
                        score.load_scored_label_contract(sequence.ROOT /
                            f"docs/reference/benchmarks/scored-labels-v{version}.json"))
            selected = score.apply_scored_label_contract(docs, contract)
            self.assertEqual(sum(len(d.spans) for d in selected), sum(len(d.spans) for d in docs))

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


class ReplayBindingTests(unittest.TestCase):
    capture = sequence.ROOT / "docs/reference/benchmarks/dictionary-sequences/baseline-a6449e2f"

    def test_valid_committed_capture_replays_unchanged(self):
        replay = sequence.replay(self.capture, score.SCORED_LABEL_CONTRACT_V1)
        self.assertEqual(replay["runs"]["switch/K"]["metrics"]["utf8_bytes"]["leaked"], 35856)
        self.assertEqual(replay["runs"]["fresh/K"]["metrics"]["utf8_bytes"]["leaked"], 0)
        for run in replay["runs"].values():
            self.assertEqual(run["pipeline_availability"]["attempted_documents"], 324)
            self.assertEqual(run["pipeline_contract"]["restore_exact_documents"], 324)
            self.assertEqual(run["pipeline_contract"]["manifest_valid_documents"], 324)

    def assert_substitution_rejected(self, replacement):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "capture"
            shutil.copytree(self.capture, directory)
            path = directory / "capture.json"
            manifest = json.loads(path.read_text())
            manifest["records"]["switch/K"] = copy.deepcopy(manifest["records"][replacement])
            path.write_text(json.dumps(manifest))
            with self.assertRaises(ValueError) as error:
                sequence.replay(directory, score.SCORED_LABEL_CONTRACT_V1)
            self.assertEqual(type(error.exception).__name__, "ReplayBindingError")
            self.assertEqual(str(error.exception), "dictionary record does not match regenerated inputs")

    def test_digest_valid_record_must_match_regenerated_inventory(self):
        for mutation in ("sequence", "corpus", "id", "gold", "population", "observation"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary) / "capture"
                shutil.copytree(self.capture, directory)
                path = directory / "capture.json"
                manifest = json.loads(path.read_text())
                evidence = manifest["records"]["switch/K"]
                record = directory / evidence["file"]
                with gzip.open(record, "rt") as stream:
                    rows = [json.loads(line) for line in stream]
                header = rows[0]
                if mutation == "sequence":
                    header["scorecard"]["parameters"]["dictionary_sequence"] = "fresh"
                elif mutation == "corpus":
                    header["corpus_sha256"] = "f" * 64
                    header["scorecard"]["dataset"]["integrity"]["sha256"] = "f" * 64
                    evidence["corpus_sha256"] = "f" * 64
                elif mutation == "id":
                    header["documents"][0]["id"] = "wrong-input"
                elif mutation == "gold":
                    header["documents"][0]["gold"] = []
                elif mutation == "population":
                    header["scorecard"]["dataset"]["evaluated_population"]["documents"] -= 1
                else:
                    rows[1]["document_id"] = rows[2]["document_id"]
                with gzip.open(record, "wt") as stream:
                    for row in rows:
                        stream.write(json.dumps(row) + "\n")
                evidence["sha256"] = score.sha256_file(record)
                path.write_text(json.dumps(manifest))
                with self.assertRaises(sequence.ReplayBindingError) as error:
                    sequence.replay(directory, score.SCORED_LABEL_CONTRACT_V1)
                self.assertEqual(str(error.exception), "dictionary record does not match regenerated inputs")

    def test_manifest_cannot_claim_a_different_generated_corpus(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "capture"
            shutil.copytree(self.capture, directory)
            path = directory / "capture.json"
            manifest = json.loads(path.read_text())
            manifest["identity"]["corpus_sha256"] = "f" * 64
            path.write_text(json.dumps(manifest))
            with self.assertRaises(sequence.ReplayBindingError):
                sequence.replay(directory, score.SCORED_LABEL_CONTRACT_V1)

    def test_fresh_control_cannot_replace_switch_record(self):
        self.assert_substitution_rejected("fresh/K")

    def test_benign_layer_cannot_replace_positive_record(self):
        self.assert_substitution_rejected("switch/B")


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
        for mutation in ("population", "runtime", "run", "binding"):
            changed = card()
            if mutation == "population":
                changed["dictionary_sequence"]["evidence"]["switch/K"]["observations"] = 0
            elif mutation == "runtime":
                changed["dictionary_sequence"]["runtime"] = {}
            elif mutation == "run":
                del changed["dictionary_sequence"]["runs"]["fresh/K"]
            else:
                changed["dictionary_sequence"]["producer_binding"]["fresh_locked_build"] = False
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
