"""Bounded source mutants must fail assertions at the URL record seams."""
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent

MUTATIONS = [
    ('shift-start', 'span = Gold(offset, offset +', 'span = Gold(offset + 1, offset +', 'RawSpanTests'),
    ('decoded-length', 'offset + len(value.encode("utf-8")), "benign"', 'offset + len(value.replace("\\\\/", "/").encode("utf-8")), "benign"', 'RawSpanTests'),
    ('swallow-quote', 'offset + len(value.encode("utf-8")), "benign"', 'offset + len(value.encode("utf-8")) + 1, "benign"', 'RawSpanTests'),
    ('unicode-char-offset', 'offset = len(prefix.encode("utf-8"))', 'offset = len(prefix)', 'RawSpanTests'),
    ('normalize-raw', '    prefix, suffix = TEMPLATES[surface][partition]', '    values = tuple(v.replace("\\\\/", "/") for v in values)\n    prefix, suffix = TEMPLATES[surface][partition]', 'RawSpanTests'),
    ('deduplicate-repeat', '    prefix, suffix = TEMPLATES[surface][partition]', '    values = tuple(dict.fromkeys(values))\n    prefix, suffix = TEMPLATES[surface][partition]', 'RawSpanTests'),
    ('cross-partitions', '    records = []\n    for cell in URL_CELLS:', '    partition = "dev"\n    records = []\n    for cell in URL_CELLS:', 'test_partition_metadata_matches_requested_partition'),
    ('drop-escaped-form', '"escaped": base.replace("/", "\\\\/")', '"escaped": base', 'test_each_declared_spelling_has_literal_boundary_evidence'),
]


class UrlMutationTests(unittest.TestCase):
    def test_eight_mutants_break_behavior_assertions_without_schema_or_syntax_errors(self):
        original = (BENCH / "url_cells.py").read_text()
        for name, old, new, selected in MUTATIONS:
            with self.subTest(mutant=name):
                self.assertEqual(original.count(old), 1, "mutant must change exactly one source site")
                mutated = original.replace(old, new)
                compile(mutated, name, "exec")
                with tempfile.TemporaryDirectory(prefix="url-cell-mutant-") as directory:
                    tmp = Path(directory)
                    (tmp / "url_cells.py").write_text(mutated)
                    shutil.copy(BENCH / "test_url_cells.py", tmp / "test_url_cells.py")
                    command = [sys.executable, "-m", "unittest", "discover", "-s", str(tmp),
                               "-p", "test_url_cells.py", "-k", selected]
                    env = {**os.environ, "PYTHONPATH": str(BENCH)}
                    result = subprocess.run(command, env=env, text=True, capture_output=True, timeout=10)
                output = result.stdout + result.stderr
                self.assertEqual(result.returncode, 1, output)
                self.assertIn("AssertionError", output)
                self.assertNotIn("FAILED (errors=", output, "an exception is not an assertion kill")
                self.assertNotIn("SyntaxError", output)
