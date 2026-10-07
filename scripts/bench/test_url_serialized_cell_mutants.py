"""Source mutants receive credit only for an executed behavior assertion."""
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent
MUTATIONS = (
    ("normalize_unicode_source", "url_serialized_cells.py", "    builder = _Builder()",
     '    values = tuple(v.replace("\\\\u", "u") for v in values)\n    builder = _Builder()', "SerializedRawSpanTests"),
    ("char_coordinates", "url_serialized_cells.py", "    return builder.build()",
     "    from dataclasses import replace\n    text, gold, decoys = builder.build()\n    return text, tuple(replace(g, start=g.start - 1, end=g.end - 1) for g in gold), decoys",
     "SerializedRawSpanTests"),
    ("swallow_neighbor", "url_serialized_cells.py", "    return builder.build()",
     "    from dataclasses import replace\n    text, gold, decoys = builder.build()\n    return text, tuple(replace(g, end=g.end + 1) for g in gold), decoys",
     "test_single_quoted_void_neighbors_are_non_gold"),
    ("deduplicate_raw_repeat", "url_serialized_cells.py", "    builder = _Builder()",
     "    values = tuple(dict.fromkeys(values))\n    builder = _Builder()", "test_literal_unicode_values_and_repeats"),
    ("drop_scheme_serialization", "url_serialized_cells.py",
     'anchor = "https:" + SCHEMES[int(variant.removeprefix("scheme_"))]', 'anchor = "https://"',
     "test_all_scheme_combinations"),
    ("drop_surrogate_spelling", "url_serialized_cells.py", r'r"\uD83D\uDE80"', r'r"\u0061"',
     "test_valid_unicode_source_values"),
    ("cross_partition", "url_serialized_cells.py", "    records = [_record(",
     '    partition = "dev"\n    records = [_record(', "test_extended_partitions"),
    ("drop_frozen_prefix", "url_cells.py", "return generate(partition) + serialized(partition)",
     "return serialized(partition)", "test_frozen_760_row_prefix"),
)


class SerializedMutationTests(unittest.TestCase):
    def test_eight_source_mutants_fail_assertions_with_valid_imports_and_schema(self):
        for name, filename, old, new, selected in MUTATIONS:
            with self.subTest(mutant=name):
                original = (BENCH / filename).read_text()
                self.assertEqual(original.count(old), 1)
                mutated = original.replace(old, new)
                compile(mutated, name, "exec")
                with tempfile.TemporaryDirectory(prefix="url-serialized-mutant-") as directory:
                    path = Path(directory)
                    for file in ("url_cells.py", "url_serialized_cells.py", "test_url_serialized_cells.py"):
                        shutil.copy(BENCH / file, path / file)
                    (path / filename).write_text(mutated)
                    result = subprocess.run(
                        [sys.executable, "-m", "unittest", "discover", "-s", str(path),
                         "-p", "test_url_serialized_cells.py", "-k", selected, "-v"],
                        env={**os.environ, "PYTHONPATH": str(BENCH)},
                        text=True, capture_output=True, timeout=10,
                    )
                output = result.stdout + result.stderr
                print(f"{name}: direct exit {result.returncode}\n{output}")
                self.assertEqual(result.returncode, 1, output)
                self.assertIn("AssertionError", output)
                self.assertIn("Ran ", output)
                self.assertNotIn("Ran 0 tests", output)
                self.assertNotIn("errors=", output)
                for failure in ("SyntaxError", "ImportError", "ModuleNotFoundError", "LayerError:"):
                    self.assertNotIn(failure, output)
