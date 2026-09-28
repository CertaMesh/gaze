"""Model-free tests for the competitors'-own-benchmark tooling."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import loaders  # noqa: E402

MAPS = {
    "extension_labels": ["IPADDRESS", "DATE"],
    "gold": {"bench": {"PERSON": ["FIRSTNAME", "SURNAME"], "IP": ["IPADDRESS"],
                       "DATE_TIME": ["DATE", "DATEOFBIRTH"], "ADDR": ["STREET", "ZIP"]}},
    "tool_extension": {"tool": {"ip": ["IPADDRESS"]}},
}


class ComposeMappingTest(unittest.TestCase):
    def setUp(self) -> None:
        self._main = loaders._main_corpus_labels
        loaders._main_corpus_labels = lambda: {"FIRSTNAME", "SURNAME", "DATEOFBIRTH", "STREET", "ZIP"}

    def tearDown(self) -> None:
        loaders._main_corpus_labels = self._main

    def test_overlap_and_extension(self) -> None:
        tool = {"name": ["FIRSTNAME"], "ip": [], "dob": ["DATEOFBIRTH"], "zip": ["ZIP"], "secret": []}
        composed = loaders.compose_mapping("tool", tool, "bench", MAPS)
        self.assertEqual(composed, {"name": ("PERSON",), "ip": ("IP",), "dob": ("DATE_TIME",),
                                    "zip": ("ADDR",), "secret": ()})

    def test_without_extension_ip_is_uncovered(self) -> None:
        maps = {**MAPS, "tool_extension": {}}
        self.assertEqual(loaders.compose_mapping("tool", {"ip": []}, "bench", maps), {"ip": ()})

    def test_extension_for_unknown_tool_label_fails(self) -> None:
        with self.assertRaisesRegex(ValueError, "never emits"):
            loaders.compose_mapping("tool", {"name": ["FIRSTNAME"]}, "bench", MAPS)

    def test_unknown_gold_target_fails(self) -> None:
        maps = json.loads(json.dumps(MAPS))
        maps["gold"]["bench"]["X"] = ["NOT_A_LABEL"]
        with self.assertRaisesRegex(ValueError, "unknown canonical"):
            loaders.compose_mapping("tool", {"ip": []}, "bench", maps)

    def test_committed_maps_compose_for_every_tool(self) -> None:
        maps = loaders.load_label_maps()
        raw = json.loads((loaders.BENCH / "compare/label-map.json").read_text(encoding="utf-8"))
        loaders._main_corpus_labels = self._main
        for benchmark in maps["gold"]:
            for tool, table in raw.items():
                composed = loaders.compose_mapping(tool, table, benchmark, maps)
                self.assertEqual(set(composed), set(table))


class BioSpansTest(unittest.TestCase):
    def test_chunks(self) -> None:
        spans = loaders.bio_spans(["Hi", "John", "Smith", "in", "New", "York"],
                                  ["O", "B-PERSON", "I-PERSON", "O", "B-LOC", "I-LOC"])
        self.assertEqual(spans, [(3, 13, "PERSON"), (17, 25, "LOC")])

    def test_orphan_and_type_switch_start_new_chunks(self) -> None:
        self.assertEqual(loaders.bio_spans(["a", "b", "c"], ["I-X", "I-Y", "I-Y"]),
                         [(0, 1, "X"), (2, 5, "Y")])

    def test_adjacent_b_tags_split(self) -> None:
        self.assertEqual(loaders.bio_spans(["a", "b"], ["B-X", "B-X"]), [(0, 1, "X"), (2, 3, "X")])

    def test_rejects_other_schemes(self) -> None:
        with self.assertRaises(ValueError):
            loaders.bio_spans(["a"], ["S-X"])


class PiibenchLoaderTest(unittest.TestCase):
    def test_hash_pin_and_byte_offsets(self) -> None:
        with tempfile.TemporaryDirectory() as root:
            data = Path(root)
            (data / "data").mkdir()
            test = data / "data/test_5k.jsonl"
            test.write_text(json.dumps({"tokens": ["Grüße", "Anna"], "labels": ["O", "B-PERSON"],
                                        "source": "few_nerd"}) + "\n", encoding="utf-8")
            manifest = {"piibench": {}, "sources": [], "excluded": {}, "published_full_mix": {},
                        "files": {"test_5k.jsonl": loaders.sha256(test)}}
            (data / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            splits, _ = loaders.load_piibench_commercial(data)
            document = splits["test"][0]
            span = document.spans[0]
            self.assertEqual(document.text.encode()[span.start:span.end], b"Anna")
            test.write_text(test.read_text(encoding="utf-8") + "\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "differs"):
                loaders.load_piibench_commercial(data)


if __name__ == "__main__":
    unittest.main()
