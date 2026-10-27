import argparse
import contextlib
import importlib.util
import io
from pathlib import Path
import shutil
import sys
import tempfile
import unittest


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


if __name__ == "__main__":
    unittest.main()
