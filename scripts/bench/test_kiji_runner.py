#!/usr/bin/env python3
"""Focused fail-closed tests for the pinned Kiji benchmark runner."""

from __future__ import annotations

import importlib.util
import io
import json
import re
import sys
import tempfile
import types
import unittest
from pathlib import Path
from contextlib import redirect_stderr, redirect_stdout
from unittest import mock


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


class FakeArray:
    def __init__(self, values: object) -> None:
        self.values = values
        self.shape = self._shape(values)
        self.ndim = len(self.shape)

    @classmethod
    def _shape(cls, values: object) -> tuple[int, ...]:
        if not isinstance(values, (list, tuple)):
            return ()
        if not values:
            return (0,)
        child_shape = cls._shape(values[0])
        if any(cls._shape(value) != child_shape for value in values):
            return (len(values),)
        return (len(values), *child_shape)

    def __getitem__(self, index: int) -> object:
        value = self.values[index]  # type: ignore[index]
        return FakeArray(value) if isinstance(value, (list, tuple)) else value

    def __iter__(self):
        for value in self.values:  # type: ignore[union-attr]
            yield FakeArray(value) if isinstance(value, (list, tuple)) else value


def load_runner() -> types.ModuleType:
    numpy_stub = types.ModuleType("numpy")
    numpy_stub.ndarray = object
    numpy_stub.int64 = int
    numpy_stub.asarray = lambda values, dtype=None: (
        values if isinstance(values, FakeArray) else FakeArray(values)
    )
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


class KijiRequestValidationTests(unittest.TestCase):
    @staticmethod
    def row(label_id: int = 0) -> list[float]:
        values = [0.0] * 9
        values[label_id] = 1.0
        return values

    def configure_inference(
        self,
        offsets: list[object],
        output: object,
        *,
        text: str = "Dr. Schmidt",
    ) -> tuple[Path, str]:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        model_dir = Path(directory.name)
        (model_dir / "model.onnx").touch()
        (model_dir / "tokenizer.json").touch()
        labels = {
            "schema_version": 1,
            "source": "onnx-community/distilbert-NER-ONNX",
            "source_commit": "3a19fe9404a4469d91aa3d551558a97f68872f67",
            "labels": [
                {"id": "person", "upstream": ["B-PER", "I-PER"]},
                {"id": "organization", "upstream": ["B-ORG", "I-ORG"]},
                {"id": "location", "upstream": ["B-LOC", "I-LOC"]},
                {"id": "miscellaneous", "upstream": ["B-MISC", "I-MISC"]},
            ]
        }
        (model_dir / "labels.json").write_text(json.dumps(labels), encoding="utf-8")

        encoding = types.SimpleNamespace(
            ids=list(range(len(offsets))),
            attention_mask=[1] * len(offsets),
            type_ids=[0] * len(offsets),
            offsets=offsets,
        )

        class FakeTokenizer:
            @classmethod
            def from_file(cls, _path: str):
                return cls()

            def encode(self, _text: str):
                return encoding

        class FakeSession:
            def __init__(self, *_args, **_kwargs) -> None:
                pass

            def get_inputs(self):
                return [types.SimpleNamespace(name="input_ids")]

            def run(self, *_args, **_kwargs):
                return [output]

        tokenizer_patch = mock.patch.object(runner, "Tokenizer", FakeTokenizer)
        session_patch = mock.patch.object(runner.ort, "InferenceSession", FakeSession)
        tokenizer_patch.start()
        session_patch.start()
        self.addCleanup(tokenizer_patch.stop)
        self.addCleanup(session_patch.stop)
        return model_dir, text

    def test_label_for_rejects_bad_width_non_finite_and_unknown_id(self) -> None:
        with self.assertRaises(runner.KijiRunnerError):
            runner.label_for(FakeArray([0.0] * 8), dict(enumerate(EXPECTED_LABELS)))
        non_finite = self.row(3)
        non_finite[4] = float("nan")
        with self.assertRaises(runner.KijiRunnerError):
            runner.label_for(FakeArray(non_finite), dict(enumerate(EXPECTED_LABELS)))
        missing = dict(enumerate(EXPECTED_LABELS))
        missing.pop(3)
        with self.assertRaises(runner.KijiRunnerError):
            runner.label_for(FakeArray(self.row(3)), missing)

    def test_label_for_rejects_non_finite_softmax(self) -> None:
        with mock.patch.object(runner.math, "exp", return_value=float("inf")):
            with self.assertRaises(runner.KijiRunnerError):
                runner.label_for(FakeArray(self.row(1)), dict(enumerate(EXPECTED_LABELS)))

    def test_rejects_rank_row_count_column_count_and_batch_mismatch(self) -> None:
        cases = [
            FakeArray([self.row()]),
            FakeArray([[self.row()[:-1]]]),
            FakeArray([[self.row(), self.row()]]),
            FakeArray([[self.row()], [self.row()]]),
        ]
        for output in cases:
            with self.subTest(shape=output.shape):
                model_dir, text = self.configure_inference([(0, 0)], output)
                with self.assertRaises(runner.KijiRunnerError):
                    runner.run(model_dir, text)

    def test_rejects_invalid_offsets_for_whole_request(self) -> None:
        invalid_offsets = [(-1, 0), (1, 0), (0, 3), (1, 2), ("0", 0), (0,)]
        for offset in invalid_offsets:
            with self.subTest(offset=offset):
                model_dir, text = self.configure_inference(
                    [offset], FakeArray([[self.row()]]), text="ä"
                )
                with self.assertRaises(runner.KijiRunnerError):
                    runner.run(model_dir, text)

    def test_injected_decode_exception_fails_whole_request_safely(self) -> None:
        model_dir, text = self.configure_inference([(0, 0)], FakeArray([[self.row()]]))
        failures = [
            RuntimeError("alice@example.invalid must not escape"),
            runner.KijiRunnerError("alice@example.invalid must not escape"),
        ]
        for failure in failures:
            with self.subTest(exception=type(failure).__name__):
                with mock.patch.object(runner, "label_for", side_effect=failure):
                    with self.assertRaisesRegex(
                        runner.KijiRunnerError, r"^token 0: token decode failed$"
                    ):
                        runner.run(model_dir, text)

    def test_accepts_zero_length_special_token_offset(self) -> None:
        model_dir, text = self.configure_inference([(0, 0)], FakeArray([[self.row(0)]]))
        self.assertEqual(runner.run(model_dir, text), [])

    def test_main_failure_is_nonzero_empty_stdout_and_sanitized_stderr(self) -> None:
        stdout = io.StringIO()
        stderr = io.StringIO()
        args = types.SimpleNamespace(model_dir="synthetic", precision="fp32")
        with (
            mock.patch.object(runner, "parse_args", return_value=args),
            mock.patch.object(runner, "read_stdin", return_value="alice@example.invalid"),
            mock.patch.object(
                runner,
                "run",
                side_effect=RuntimeError("alice@example.invalid must not escape"),
            ),
            redirect_stdout(stdout),
            redirect_stderr(stderr),
        ):
            exit_code = runner.main([])
        self.assertNotEqual(exit_code, 0)
        self.assertEqual(stdout.getvalue(), "")
        self.assertEqual(stderr.getvalue(), "kiji-runner: request failed\n")
        self.assertNotIn("alice@example.invalid", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
