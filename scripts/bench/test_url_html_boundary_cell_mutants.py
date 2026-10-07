"""Only executed behavior assertion failures count as source mutation kills."""
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent
MUTATIONS = (
    ("wrong_quote", 'builder.text(f"<{tag} {key}=\'")',
     'builder.text(f\'<{tag} {key}="\')', "test_literal_img_link_unicode"),
    ("swallow_slash", "    return builder.build()",
     "    from dataclasses import replace\n    text, gold, decoys = builder.build()\n    return text, tuple(replace(g, end=g.end + 2) for g in gold), decoys",
     "test_literal_img_link_unicode"),
    ("drop_repeat", "    builder = _Builder()",
     "    values = tuple(dict.fromkeys(values))\n    builder = _Builder()", "test_literal_img_link_unicode"),
    ("omit_link", 'return generate(partition) + serialized(partition)',
     'return [r for r in generate(partition) + serialized(partition) if r.surface != "url_serialized_html_link"]',
     "test_both_selfclosing_tags"),
)


class HtmlBoundaryMutationTests(unittest.TestCase):
    def test_four_mutants_fail_html_behavior_assertions(self):
        for name, old, new, selected in MUTATIONS:
            with self.subTest(mutant=name):
                filename = "url_cells.py" if name == "omit_link" else "url_serialized_cells.py"
                original = (BENCH / filename).read_text()
                self.assertEqual(original.count(old), 1)
                mutated = original.replace(old, new)
                compile(mutated, name, "exec")
                with tempfile.TemporaryDirectory(prefix="url-html-mutant-") as directory:
                    path = Path(directory)
                    for file in ("url_cells.py", "url_serialized_cells.py", "test_url_html_boundary_cells.py"):
                        shutil.copy(BENCH / file, path / file)
                    (path / filename).write_text(mutated)
                    child = subprocess.run(
                        [sys.executable, "-m", "unittest", "discover", "-s", str(path),
                         "-p", "test_url_html_boundary_cells.py", "-k", selected, "-v"],
                        env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1", "PYTHONPATH": str(BENCH)},
                        text=True, capture_output=True, timeout=10,
                    )
                output = child.stdout + child.stderr
                print(f"{name}: direct exit {child.returncode}\n{output}")
                self.assertEqual(child.returncode, 1, output)
                self.assertIn("AssertionError", output)
                self.assertIn("Ran 1 test", output)
                for failure in ("errors=", "SyntaxError", "ImportError", "ModuleNotFoundError", "LayerError:"):
                    self.assertNotIn(failure, output)


if __name__ == "__main__":
    unittest.main()
