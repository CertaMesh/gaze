#!/usr/bin/env python3
"""Focused fail-closed tests for the pinned Kiji benchmark runner."""

from __future__ import annotations

import importlib.util
import json
import re
import sys
import tempfile
import types
import unittest
from pathlib import Path


EXPECTED_LABELS = (
    "O",
    "B-PER",
    "I-PER",
    "B-ORG",
    "I-ORG",
    "B-LOC",
    "I-LOC",
    "B-MISC",
    "I-MISC",
)


def load_runner() -> types.ModuleType:
    numpy_stub = types.ModuleType("numpy")
    numpy_stub.ndarray = object
    ort_stub = types.ModuleType("onnxruntime")
    ort_stub.InferenceSession = object
    tokenizers_stub = types.ModuleType("tokenizers")
    tokenizers_stub.Tokenizer = object
    sys.modules["numpy"] = numpy_stub
    sys.modules["onnxruntime"] = ort_stub
    sys.modules["tokenizers"] = tokenizers_stub

    runner_path = Path(__file__).with_name("kiji-runner.py")
    spec = importlib.util.spec_from_file_location("kiji_runner_under_test", runner_path)
    if spec is None or spec.loader is None:
        raise RuntimeError("unable to load Kiji runner")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


runner = load_runner()


class KijiLabelRegistryTests(unittest.TestCase):
    def write_labels(self, payload: object) -> Path:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name) / "labels.json"
        path.write_text(json.dumps(payload), encoding="utf-8")
        return path

    @staticmethod
    def pinned_vocabulary_payload(labels: object) -> dict[str, object]:
        return {
            "schema_version": 1,
            "source": "onnx-community/distilbert-NER-ONNX",
            "source_commit": "3a19fe9404a4469d91aa3d551558a97f68872f67",
            "labels": labels,
        }

    def test_exact_pinned_mapping_and_rust_parity(self) -> None:
        expected = dict(enumerate(EXPECTED_LABELS))
        self.assertEqual(runner.PINNED_LABELS, expected)
        self.assertEqual((runner.PINNED_LABELS[3], runner.PINNED_LABELS[4]), ("B-ORG", "I-ORG"))
        self.assertEqual((runner.PINNED_LABELS[5], runner.PINNED_LABELS[6]), ("B-LOC", "I-LOC"))

        rust_path = (
            Path(__file__).resolve().parents[2]
            / "crates/gaze-recognizers/src/safety_net/kiji_distilbert/backend/decode.rs"
        )
        rust_source = rust_path.read_text(encoding="utf-8")
        registry = re.search(
            r"pub\(crate\) const ID2LABEL: \[&str; 9\] = \[(.*?)\];",
            rust_source,
            re.DOTALL,
        )
        self.assertIsNotNone(registry)
        assert registry is not None
        self.assertEqual(tuple(re.findall(r'"([^"]+)"', registry.group(1))), EXPECTED_LABELS)

    def test_accepts_exact_id2label(self) -> None:
        payload = {"id2label": {str(index): label for index, label in enumerate(EXPECTED_LABELS)}}
        self.assertEqual(runner.load_labels(self.write_labels(payload)), dict(enumerate(EXPECTED_LABELS)))

    def test_accepts_exact_pinned_vocabulary(self) -> None:
        payload = self.pinned_vocabulary_payload(
            [
                {"id": "person", "upstream": ["B-PER", "I-PER"]},
                {"id": "location", "upstream": ["B-LOC", "I-LOC"]},
                {"id": "organization", "upstream": ["B-ORG", "I-ORG"]},
                {"id": "miscellaneous", "upstream": ["B-MISC", "I-MISC"]},
            ]
        )
        self.assertEqual(runner.load_labels(self.write_labels(payload)), dict(enumerate(EXPECTED_LABELS)))

    def test_rejects_ambiguous_or_extra_label_schema_members(self) -> None:
        exact = {str(index): label for index, label in enumerate(EXPECTED_LABELS)}
        valid_labels = [
            {"id": "person", "upstream": ["B-PER", "I-PER"]},
            {"id": "organization", "upstream": ["B-ORG", "I-ORG"]},
            {"id": "location", "upstream": ["B-LOC", "I-LOC"]},
            {"id": "miscellaneous", "upstream": ["B-MISC", "I-MISC"]},
        ]
        valid_vocabulary = self.pinned_vocabulary_payload(valid_labels)
        malformed_member = [dict(item) for item in valid_labels]
        malformed_member[0]["extra"] = "disallowed"
        ambiguous = [
            {"id2label": exact, "labels": []},
            {**valid_vocabulary, "id2label": exact},
            {"id2label": exact, "extra": "disallowed"},
            {**valid_vocabulary, "extra": "disallowed"},
            self.pinned_vocabulary_payload(malformed_member),
        ]

        for payload in ambiguous:
            with self.subTest(payload=payload):
                with self.assertRaises(runner.KijiRunnerError):
                    runner.load_labels(self.write_labels(payload))

    def test_rejects_malformed_id2label_mappings(self) -> None:
        exact = {str(index): label for index, label in enumerate(EXPECTED_LABELS)}
        malformed = []
        partial = dict(exact)
        partial.pop("8")
        malformed.append({"id2label": partial})
        malformed.append({"id2label": {**exact, "9": "O"}})
        malformed.append({"id2label": {**exact, "00": "O"}})
        malformed.append({"id2label": {**exact, "3": "B-LOC", "5": "B-ORG"}})
        malformed.append({"id2label": {**exact, "4": "I-UNKNOWN"}})
        malformed.append({"id2label": {**exact, "2": 2}})
        malformed.append({"id2label": list(EXPECTED_LABELS)})

        for payload in malformed:
            with self.subTest(payload=payload):
                with self.assertRaises(runner.KijiRunnerError):
                    runner.load_labels(self.write_labels(payload))

    def test_rejects_malformed_vocabulary(self) -> None:
        valid = [
            {"id": "person", "upstream": ["B-PER", "I-PER"]},
            {"id": "organization", "upstream": ["B-ORG", "I-ORG"]},
            {"id": "location", "upstream": ["B-LOC", "I-LOC"]},
            {"id": "miscellaneous", "upstream": ["B-MISC", "I-MISC"]},
        ]
        malformed = [
            self.pinned_vocabulary_payload(valid[:-1]),
            self.pinned_vocabulary_payload(valid + [{"id": "other", "upstream": ["B-OTHER"]}]),
            self.pinned_vocabulary_payload(
                [*valid[:-1], {"id": "miscellaneous", "upstream": ["B-MISC", "B-MISC"]}]
            ),
            self.pinned_vocabulary_payload(
                [*valid[:-1], {"id": "miscellaneous", "upstream": ["B-MISC", "I-UNKNOWN"]}]
            ),
            self.pinned_vocabulary_payload(
                [*valid[:-1], {"id": 4, "upstream": ["B-MISC", "I-MISC"]}]
            ),
            self.pinned_vocabulary_payload("not-a-list"),
            {**self.pinned_vocabulary_payload(valid), "schema_version": True},
            {**self.pinned_vocabulary_payload(valid), "source": "synthetic/model"},
            {**self.pinned_vocabulary_payload(valid), "source_commit": "0" * 40},
            {"labels": valid},
            {},
        ]
        for payload in malformed:
            with self.subTest(payload=payload):
                with self.assertRaises(runner.KijiRunnerError):
                    runner.load_labels(self.write_labels(payload))

    def test_rejects_duplicate_json_keys(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name) / "labels.json"
        path.write_text('{"id2label": {}, "id2label": {}}', encoding="utf-8")
        with self.assertRaises(runner.KijiRunnerError):
            runner.load_labels(path)


if __name__ == "__main__":
    unittest.main()
