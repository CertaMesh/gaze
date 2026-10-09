import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import benchmark_cache as cache


class ObservationCacheTests(unittest.TestCase):
    def setUp(self):
        self.key = {
            "crates_tree_sha256": "1" * 64,
            "policy_sha256": "2" * 64,
            "corpus_sha256": "3" * 64,
            "seed": 20260710,
            "scored_labels_sha256": "4" * 64,
            "model_bundle_sha256": {"davlan": "5" * 64, "nym": "6" * 64},
            "profile": "full",
        }

    def test_exact_key_hits_and_mutated_key_misses_with_reason(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            record = root / "observations-v1.jsonl.gz"
            record.write_bytes(b"value-free-observations")
            cache.store(root / "cache", self.key, record)

            hit, reason = cache.lookup(root / "cache", self.key)
            self.assertEqual(hit.read_bytes(), record.read_bytes())
            self.assertIn("cache hit", reason)

            mutated = {**self.key, "seed": self.key["seed"] + 1}
            miss, reason = cache.lookup(root / "cache", mutated)
            self.assertIsNone(miss)
            self.assertIn("seed", reason)

    def test_corrupted_record_is_a_cache_miss(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            record = root / "observations-v1.jsonl.gz"
            record.write_bytes(b"original")
            cached = cache.store(root / "cache", self.key, record)
            cached.write_bytes(b"corrupt")

            hit, reason = cache.lookup(root / "cache", self.key)
            self.assertIsNone(hit)
            self.assertIn("record sha256", reason)

    def test_malformed_metadata_is_a_cache_miss(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            record = root / "observations-v1.jsonl.gz"
            record.write_bytes(b"original")
            cached = cache.store(root / "cache", self.key, record)
            cached.with_name(cache.METADATA).write_text("[]", encoding="utf-8")

            hit, reason = cache.lookup(root / "cache", self.key)
            self.assertIsNone(hit)
            self.assertIn("invalid metadata", reason)

    def test_key_digest_is_independent_of_mapping_order(self):
        forward = cache.key_digest(self.key)
        reverse = cache.key_digest(dict(reversed(tuple(self.key.items()))))
        expected = hashlib.sha256(
            json.dumps(self.key, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
        self.assertEqual(forward, reverse)
        self.assertEqual(forward, expected)
