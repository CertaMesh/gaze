import gzip
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import bench_json


BENCH = Path(__file__).resolve().parents[2] / "docs/reference/benchmarks"


def scorecard_pins() -> list[tuple[str, str]]:
    """Every (scorecard file, sha256) the committed release evidence records."""
    history = json.loads((BENCH / "release-history.json").read_text(encoding="utf-8"))
    pins = []
    for release in history["releases"]:
        pins.append((release["scorecard"], release["scorecard_sha256"]))
        pins += [(result["scorecard"], result["scorecard_sha256"])
                 for result in release.get("contract_results", ())]

    def walk(node: object) -> None:
        if isinstance(node, dict):
            name = node.get("file")
            if isinstance(name, str) and name.startswith("scorecard-"):
                pins.append((name, node["sha256"]))
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    for receipt in sorted(BENCH.glob("release-measurement-receipts-*.json")):
        walk(json.loads(receipt.read_text(encoding="utf-8")))
    return pins


class BenchJsonTests(unittest.TestCase):
    def test_compression_is_byte_reproducible_and_content_addressed(self):
        content = json.dumps({"a": [1, 2, 3]}, indent=2).encode("utf-8") + b"\n"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "card.json"
            source.write_bytes(content)
            first, second = root / "first.json.gz", root / "second.json.gz"
            digest = bench_json.compress(source, first)
            bench_json.compress(source, second)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            self.assertEqual(gzip.decompress(first.read_bytes()), content)
            self.assertEqual(digest, hashlib.sha256(content).hexdigest())
            self.assertEqual(bench_json.content_sha256(first), bench_json.content_sha256(source))
            self.assertEqual(bench_json.load(first), {"a": [1, 2, 3]})

    def test_compress_refuses_invalid_json_and_wrong_suffixes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "card.json"
            source.write_text("{not json", encoding="utf-8")
            with self.assertRaises(ValueError):
                bench_json.compress(source, root / "card.json.gz")
            self.assertFalse((root / "card.json.gz").exists())
            source.write_text("{}", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "must end in .gz"):
                bench_json.compress(source, root / "card.json")

    def test_committed_scorecards_are_gzip_only_and_match_every_pin(self):
        self.assertEqual(sorted(BENCH.glob("scorecard-*.json")), [])
        committed = {path.name for path in BENCH.glob("scorecard-*.json.gz")}
        self.assertTrue(committed)
        pins = scorecard_pins()
        self.assertTrue(pins)
        for name, pinned in pins:
            with self.subTest(name=name):
                self.assertIn(name, committed)
                self.assertEqual(bench_json.content_sha256(BENCH / name), pinned)


if __name__ == "__main__":
    unittest.main()
