import argparse
import contextlib
import importlib.util
import io
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "verify_clean_equality.py"
SPEC = importlib.util.spec_from_file_location("verify_clean_equality", SCRIPT)
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


class BinaryIdentity(unittest.TestCase):
    def test_copied_binary_is_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp) / "base"
            candidate = Path(temp) / "candidate"
            shutil.copyfile(sys.executable, base)
            shutil.copyfile(base, candidate)
            error = io.StringIO()
            with contextlib.redirect_stderr(error), self.assertRaises(SystemExit) as refused:
                gate.binary_hashes(argparse.ArgumentParser(), (base, candidate))
            self.assertEqual(refused.exception.code, 2)
            self.assertIn("identical binary contents", error.getvalue())

    def test_distinct_binary_contents_keep_both_hashes(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp) / "base"
            candidate = Path(temp) / "candidate"
            base.write_bytes(b"base executable")
            candidate.write_bytes(b"candidate executable")
            self.assertEqual(
                gate.binary_hashes(argparse.ArgumentParser(), (base, candidate)),
                [gate.sha256(base.read_bytes()), gate.sha256(candidate.read_bytes())],
            )


class ComparisonContract(unittest.TestCase):
    def test_one_byte_mutant_refuses_placeholder_only_output(self):
        base = (0, {"clean_text": "<SESSION:EMAIL_1> <SESSION:NAME_2>"})
        self.assertIsNone(gate.one_byte_mutant(base, base))

    def test_one_byte_mutant_changes_prose_after_placeholders(self):
        base = (0, {"clean_text": "<SESSION:EMAIL_1> ü Public prose"})
        observations = []
        compare = gate.differs

        def record(left, right):
            observations.append(right)
            return compare(left, right)

        with patch.object(gate, "differs", side_effect=record):
            proof = gate.one_byte_mutant(base, base)
        self.assertEqual(proof, {"changed_output_bytes": 1, "detected": True})
        self.assertEqual(observations[-1][1]["clean_text"],
                         "<SESSION:EMAIL_1> ü Aublic prose")
        self.assertEqual(base[1]["clean_text"], "<SESSION:EMAIL_1> ü Public prose")

    def test_only_complete_emitted_tokens_lose_the_session_prefix(self):
        response = {
            "clean_text": "<abcdef12:EMAIL_1> <abcdef12:PUBLIC> <abcdef12:",
            "entries": [{"token": "<abcdef12:EMAIL_1>"}],
            "stats": {},
            "leak_report": {},
        }
        normalized = gate.normalize(response)
        self.assertEqual(
            normalized["clean_text"],
            "<SESSION:EMAIL_1> <abcdef12:PUBLIC> <abcdef12:",
        )
        self.assertEqual(normalized["entries"], [{"token": "<SESSION:EMAIL_1>"}])

    def test_one_byte_output_change_is_rejected(self):
        base = (0, {"clean_text": "Public prose"})
        self.assertEqual(
            gate.one_byte_mutant(base, base),
            {"changed_output_bytes": 1, "detected": True},
        )
        changed = (0, {"clean_text": "Aublic prose"})
        self.assertTrue(gate.differs(base, changed))
        with self.assertRaisesRegex(RuntimeError, "one-byte output mutant survived"):
            gate.one_byte_mutant(base, changed)


if __name__ == "__main__":
    unittest.main()
