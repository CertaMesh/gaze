"""Case-count gates for the committed CLI/benchmark equivalence sample."""

import argparse
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import check_policy_equivalence as equivalence


class EquivalenceCaseCountTests(unittest.TestCase):
    def test_self_check_rejects_empty_and_short_fixtures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture_dir = root / "fixtures"
            fixture_dir.mkdir()
            for name in ("gaze", "bench", "policy.toml"):
                (root / name).touch()
            args = argparse.Namespace(
                self_check=True,
                gaze=root / "gaze",
                bench=root / "bench",
                policy=root / "policy.toml",
                seed=20260710,
                documents=220,
            )
            fixture = fixture_dir / "policy_equivalence.jsonl"
            for rows in ([], [
                {"id": "synthetic-1", "language": "en", "text": "alice@example.invalid"},
                {"id": "synthetic-2", "language": "de", "text": "Dr. Schmidt"},
            ]):
                with self.subTest(case_count=len(rows)):
                    fixture.write_text("".join(json.dumps(row) + "\n" for row in rows))
                    with mock.patch.object(equivalence, "FIXTURES", fixture_dir), \
                            mock.patch.object(equivalence, "BenchSubprocess") as spawn:
                        with self.assertRaisesRegex(ValueError, "expected at least 6"):
                            equivalence.compare(args)
                        spawn.assert_not_called()


if __name__ == "__main__":
    unittest.main()
