"""Public supplemental corpus and record-oracle seams, never native proof."""

import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import jwt_payload_oracle as oracle


class CorpusTests(unittest.TestCase):
    def test_partitions_have_independent_inserted_gold_and_matched_counterweights(self):
        rows = oracle.generate()
        for partition in ("dev", "test"):
            selected = [r for r in rows if r["partition"] == partition]
            positives = [r for r in selected if r["kind"] == "positive"]
            benign = [r for r in selected if r["kind"] == "benign"]
            self.assertEqual(len(positives), 152)
            self.assertEqual(len(benign), 152)
            self.assertEqual({r["pair"] for r in positives}, {r["pair"] for r in benign})
            for row in selected:
                oracle.validate_fixture(row)
            first = positives[0]
            self.assertEqual(first["text"][:first["value_span"][0]], partition + " securitytoken: ")
            self.assertEqual(first["payload"], "eyJzdWIiOiJhbGljZUBleGFtcGxlLmludmFsaWQifQ")
        self.assertEqual(len({r["id"] for r in rows}), len(rows))
        self.assertEqual(len({r["text"] for r in rows}), len(rows))

    def test_committed_inputs_are_pinned_and_gold_tampering_is_rejected(self):
        self.assertEqual(oracle.load_corpus(), oracle.generate())
        row = copy.deepcopy(oracle.generate()[0])
        row["payload_span"][0] += 1
        with self.assertRaises(ValueError):
            oracle.validate_fixture(row)
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            for name in ("dev.jsonl", "test.jsonl", "pins.json"):
                (directory / name).write_bytes((oracle.FIXTURES / name).read_bytes())
            (directory / "dev.jsonl").write_text("{}\n")
            with self.assertRaisesRegex(ValueError, "corpus bytes"):
                oracle.load_corpus(directory)


def observation(row, span=None):
    """Worked native-protocol fixture, not a fake executable or native success."""
    start, end = span or row["value_span"]
    raw = row["text"].encode()
    token = b"<abcd1234:Custom:security_token_1>"
    clean = raw[:start] + token + raw[end:]
    return {
        "id": row["id"], "input_sha256": row["input_sha256"],
        "policy_sha256": "a" * 64, "refused": False, "error": None,
        "clean_text": clean.decode(), "restored": row["text"],
        "imported_restored": row["text"],
        "manifest": [{"raw_span": [start, end], "clean_span": [start, start + len(token)],
                      "class": oracle.CLASS, "token_restore": raw[start:end].decode()}],
        "trace": [{"raw_span": [start, end], "class": oracle.CLASS,
                   "sources": [oracle.SOURCE]}],
    }


class RecordTests(unittest.TestCase):
    def test_partial_header_protection_still_leaks_entire_personal_payload(self):
        row = oracle.generate()[0]
        start = row["value_span"][0]
        result = oracle.score_record(row, observation(row, [start, start + 20]), "a" * 64)
        self.assertEqual(result["leaked_payload_bytes"], 42)
        self.assertEqual(result["leaked_value_bytes"], len(row["value"]) - 20)
        self.assertEqual(result["whole_values"], 0)
        self.assertEqual(result["exact_restores"], 1)
        full = oracle.score_record(row, observation(row), "a" * 64)
        self.assertEqual(full["leaked_payload_bytes"], 0)
        self.assertEqual(full["leaked_value_bytes"], 0)
        self.assertEqual(full["whole_values"], 1)

    def test_credential_controls_are_neutral_but_punctuation_is_a_precision_failure(self):
        rows = oracle.generate()
        control = next(r for r in rows if r["kind"] == "control" and r["protect"])
        self.assertEqual(oracle.score_record(control, observation(control), "a" * 64)["benign_fp_bytes"], 0)
        positive = rows[0]
        start, end = positive["value_span"]
        score = oracle.score_record(positive, observation(positive, [start, end + 1]), "a" * 64)
        self.assertEqual(score["punctuation_fp_bytes"], 1)
        self.assertEqual(score["benign_fp_bytes"], 1)
        benign = next(r for r in rows if r["kind"] == "benign")
        self.assertEqual(oracle.score_record(benign, observation(benign), "a" * 64)["benign_fp_bytes"], len(benign["value"]))

    def test_protocol_and_byte_mapping_mutants_fail_closed(self):
        row = oracle.generate()[0]
        good = observation(row)
        mutants = []
        for key, value in (("policy_sha256", "b" * 64), ("input_sha256", "b" * 64),
                           ("id", "wrong"), ("refused", 0), ("trace", [])):
            mutant = copy.deepcopy(good)
            mutant[key] = value
            mutants.append(mutant)
        mutant = copy.deepcopy(good)
        mutant["manifest"][0]["token_restore"] = "wrong"
        mutants.append(mutant)
        mutant = copy.deepcopy(good)
        mutant["clean_text"] += " extra"
        mutants.append(mutant)
        mutant = copy.deepcopy(good)
        mutant["trace"][0]["sources"] = ["invented.detector"]
        mutants.append(mutant)
        mutant = copy.deepcopy(good)
        mutant["manifest"][0]["raw_span"][0] = -1
        mutants.append(mutant)
        for mutant in mutants:
            with self.subTest(mutant=mutant), self.assertRaises(ValueError):
                oracle.score_record(row, mutant, "a" * 64)

    def test_restore_failures_and_refusals_cannot_hide_leaks(self):
        row = oracle.generate()[0]
        record = observation(row)
        record["imported_restored"] = None
        score = oracle.score_record(row, record, "a" * 64)
        self.assertEqual(score["restore_failures"], 1)
        self.assertEqual(score["exact_restores"], 0)
        record.update(refused=True, error="clean-stage refusal", clean_text=None,
                      restored=None, imported_restored=None, manifest=[], trace=[])
        score = oracle.score_record(row, record, "a" * 64)
        self.assertEqual(score["refusals"], 1)
        self.assertEqual(score["leaked_payload_bytes"], 42)
        self.assertEqual(score["leaked_value_bytes"], 100)

    def test_empty_duplicate_missing_extra_or_unpinned_native_results_fail(self):
        rows = [oracle.generate()[0]]
        handshake = {"protocol": oracle.VERSION, "build_commit": "c" * 40,
                     "policy_sha256": "a" * 64, "model_free": True}
        good = json.dumps(handshake) + "\n" + json.dumps(observation(rows[0])) + "\n"
        scored = oracle.score_stream(rows, good, "a" * 64, "c" * 40)
        self.assertEqual(scored["executed"], 1)
        for data in ("", json.dumps(handshake), good + json.dumps(observation(rows[0])),
                     good.replace('"build_commit": "' + "c" * 40, '"build_commit": "' + "d" * 40),
                     good.replace('"refused": false', '"refused": false, "refused": false')):
            with self.subTest(data=data), self.assertRaises(ValueError):
                oracle.score_stream(rows, data, "a" * 64, "c" * 40)
        with self.assertRaises(ValueError):
            oracle.score_stream([], good, "a" * 64, "c" * 40)

    def test_native_invocation_rejects_process_errors_and_zero_records(self):
        # Python subprocesses exercise failure handling only. No native success is simulated.
        binary = Path(sys.executable)
        binary_hash = oracle.digest(binary.read_bytes())
        with tempfile.TemporaryDirectory() as temp:
            policy = Path(temp) / "failure_only.py"
            for source, message in (("raise SystemExit(7)\n", "native error exit=7"),
                                    ("pass\n", "executed native records")):
                policy.write_text(source)
                with self.assertRaisesRegex(ValueError, message):
                    oracle.run_native(binary, binary_hash, "c" * 40, [oracle.generate()[0]],
                                      policy, oracle.digest(policy.read_bytes()))

    def test_measure_never_reuses_an_existing_report(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "old-result.json"
            output.write_text("old evidence\n")
            run = subprocess.run([sys.executable, str(Path(oracle.__file__).resolve()), "measure",
                                  "--base-bin", "absent-base", "--candidate-bin", "absent-head",
                                  "--base-sha256", "a" * 64, "--candidate-sha256", "b" * 64,
                                  "--base-commit", "c" * 40, "--candidate-commit", "d" * 40,
                                  "--output", str(output)], capture_output=True, text=True, timeout=30)
            self.assertNotEqual(run.returncode, 0)
            self.assertIn("existing report", run.stderr)
            self.assertEqual(output.read_text(), "old evidence\n")

    def test_assertion_mutants_are_killed_by_behavioral_tests(self):
        source = Path(oracle.__file__).read_text()
        mutations = (
            ('result["leaked_payload_bytes"] = len(payload - protected)',
             'result["leaked_payload_bytes"] = 0',
             "test_partial_header_protection_still_leaks_entire_personal_payload"),
            ('result["benign_fp_bytes"] = len(fp)', 'result["benign_fp_bytes"] = 0',
             "test_credential_controls_are_neutral_but_punctuation_is_a_precision_failure"),
            ('restore_failures=int(not restored)', 'restore_failures=0',
             "test_restore_failures_and_refusals_cannot_hide_leaks"),
        )
        for old, new, test in mutations:
            self.assertEqual(source.count(old), 1)
            with tempfile.TemporaryDirectory() as temp:
                directory = Path(temp)
                (directory / "jwt_payload_oracle.py").write_text(source.replace(old, new))
                (directory / "test_jwt_payload_oracle.py").write_text(Path(__file__).read_text())
                run = subprocess.run([sys.executable, "-m", "unittest",
                                      "test_jwt_payload_oracle.RecordTests." + test],
                                     cwd=directory, text=True, capture_output=True, timeout=30)
                self.assertNotEqual(run.returncode, 0)
                self.assertIn("FAILED (failures=1)", run.stderr)
                self.assertNotIn("ERROR:", run.stderr)


if __name__ == "__main__":
    unittest.main()
