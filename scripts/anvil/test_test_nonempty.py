"""Exercise the command boundary, including false passes and child failures."""

from pathlib import Path
import subprocess
import sys
import unittest


RUNNER = Path(__file__).with_name("test_nonempty.py")


class NonemptyTestCommand(unittest.TestCase):
    def run_child(self, source):
        return subprocess.run(
            [sys.executable, str(RUNNER), sys.executable, "-c", source],
            capture_output=True,
            text=True,
            check=False,
        )

    def test_empty_and_ignored_only_suites_fail(self):
        for output in ("", "test result: ok. 0 passed; 0 failed; 3 ignored;"):
            with self.subTest(output=output):
                result = self.run_child(f"print({output!r})")
                self.assertEqual(result.returncode, 1)
                self.assertIn("zero tests", result.stderr)

    def test_nonempty_suites_stream_output_and_pass(self):
        result = self.run_child(
            "print('test result: ok. 2 passed; 0 failed; 0 ignored;');"
            "print('test result: ok. 3 passed; 0 failed; 0 ignored;')"
        )
        self.assertEqual(result.returncode, 0)
        self.assertIn("Verified 5 passing Rust tests", result.stdout)

    def test_child_failure_is_preserved_after_passing_suite(self):
        result = self.run_child(
            "print('test result: ok. 2 passed; 0 failed; 0 ignored;');"
            "raise SystemExit(17)"
        )
        self.assertEqual(result.returncode, 17)


if __name__ == "__main__":
    unittest.main()
