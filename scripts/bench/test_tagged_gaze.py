"""The tagged-only rule for public benchmark blocks (tagged_gaze.py).

The rule is one function, `check_public`, that every public renderer routes its
output through. These tests prove the function refuses each untagged shape, that
the committed public surfaces pass it, and that the renderers themselves keep the
untagged numbers out when the data still holds them.
"""

from __future__ import annotations

import copy
import json
import re
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import mechanism_arms  # noqa: E402
import render_benchmark_doc as render  # noqa: E402
from tagged_gaze import UntaggedGazeError, check_public, require_tag  # noqa: E402

BENCH = render.BENCH_DIR
PUBLIC_BLOCKS = {
    render.DEFAULT_README: ("readme-chart",),
    render.DEFAULT_DOC: (
        "current-release", "panels", "charts", "history", "mechanism-arms", "leak-ledger",
        "latency", "their-benchmarks",
    ),
}


def block(text: str, name: str) -> str:
    begin, end = render.begin_marker(name), render.end_marker(name)
    return text[text.index(begin) + len(begin): text.index(end)]


class GuardShapesTest(unittest.TestCase):
    def test_each_untagged_shape_is_refused(self):
        for text in (
            "| gaze-full | 10 | 20 |",
            "Gaze main a2f6fefd, unreleased (7.5%)",
            "| GLiNER date-of-birth judge | `35c4fc79377d` (unreleased) | v3 |",
            "| v3 | Kiji EN/DE holdout | gaze | 9,256 | 15,560 |",
            "Gaze measured at `a2f6fefd33cebd040a4fc6e5a384f318807717af` (release `v0.15.1`)",
            "Gaze ran on crates tree `97e45cfe`.",
            "Main `1809f6a3c4b2` (record measured on ...)",
        ):
            with self.subTest(text=text), self.assertRaises(UntaggedGazeError):
                check_public(text, "block")

    def test_tagged_text_passes(self):
        for text in (
            "| Gaze 0.15 | 89.2% |", "Gaze v0.15.1 on this set: not yet measured",
            "| gaze-v0.15.1 | 12 |", "`gaze setup` writes the policy", "| Gaze 0.14 | 82.1% |",
        ):
            self.assertEqual(check_public(text, "block"), text)

    def test_require_tag(self):
        self.assertEqual(require_tag("v0.15.1", "x"), "v0.15.1")
        for bad in ("main", "v0.15", "v0.15.1-rc.1", "a2f6fefd", "unreleased"):
            with self.assertRaises(UntaggedGazeError):
                require_tag(bad, "x")


class CommittedSurfacesTest(unittest.TestCase):
    def test_every_generated_public_block_passes_the_guard(self):
        for path, names in PUBLIC_BLOCKS.items():
            text = path.read_text(encoding="utf-8")
            for name in names:
                with self.subTest(path=path.name, block=name):
                    check_public(block(text, name), name)
        check_public((BENCH / "competitors.md").read_text(encoding="utf-8"), "competitors.md")

    def test_every_gaze_bar_and_column_resolves_to_a_released_tag(self):
        history = render.load_history(render.DEFAULT_HISTORY)
        versions = {entry["version"] for entry in history["releases"]}
        minors = {v[1:].rsplit(".", 1)[0] for v in versions}
        panels_text = block(render.DEFAULT_DOC.read_text(encoding="utf-8"), "panels")
        names = set(re.findall(r"\bGaze (\d+\.\d+)\b", panels_text))
        self.assertTrue(names)
        self.assertLessEqual(names, minors)
        for svg in (render.DEFAULT_README.parent / render.CHART_ASSETS).glob("*.svg"):
            found = set(re.findall(r"Gaze</text>\s*<text[^>]*>(\d+\.\d+)<", svg.read_text(encoding="utf-8")))
            self.assertLessEqual(found, minors, svg.name)


class RendererFiltersTest(unittest.TestCase):
    def test_mechanism_arms_hides_unreleased_measurements_and_shows_tagged(self):
        ledger = mechanism_arms.load_ledger()
        releases = mechanism_arms._released_versions(mechanism_arms.RELEASE_HISTORY)
        hidden = mechanism_arms.render(ledger, releases)
        self.assertIn("measurements of unreleased builds", hidden)
        self.assertNotIn("Leaked bytes, without", hidden)
        shown = copy.deepcopy(ledger)
        shown["mechanisms"][0]["measurements"][0]["release"] = "v0.15.1"
        text = mechanism_arms.render(shown, releases)
        self.assertIn("| v0.15.1 |", text)
        untagged = copy.deepcopy(ledger)
        untagged["mechanisms"][0]["measurements"][0]["release"] = "main"
        self.assertEqual(mechanism_arms.render(untagged, releases), hidden)

    def test_the_leak_ledger_page_carries_no_numbers(self):
        import leak_ledger

        index = json.loads(leak_ledger.INDEX.read_text(encoding="utf-8"))
        body = leak_ledger.public_body(index)
        self.assertFalse(re.search(r"\d{1,3},\d{3}", body))
        self.assertNotIn("Main `", body)


if __name__ == "__main__":
    unittest.main()
