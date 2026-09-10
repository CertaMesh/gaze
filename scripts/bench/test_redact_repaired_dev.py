"""Synthetic request-order checks. No model or corpus access."""
import json
from pathlib import Path
import tempfile
import unittest

import redact_repaired_dev as dev


class AdmissionJoinTests(unittest.TestCase):
    def joined(self, records):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "audit.jsonl"
            path.write_text("".join(json.dumps(record) + "\n" for record in records))
            return dev.joined_audit(path, ["synthetic-a", "synthetic-b", "synthetic-c"])

    def test_refusal_does_not_shift_next_request_and_missing_stays_unknown(self):
        batch = {"status": "complete", "batch": 1,
                 "coordinates": "detector_input_utf8", "spans": [
                     {"start": 2, "end": 15, "label": "credit_card",
                      "disposition": "semantic_invalid"}]}
        result = self.joined([
            {"request": 1, "status": "request_begin"}, batch,
            {"request": 1, "status": "request_refusal"},
            {"request": 2, "status": "request_begin"},
            {"request": 2, "status": "request_success"},
        ])["rows"]
        self.assertEqual(result[0]["records"][1], batch)
        self.assertEqual(result[0]["terminal"], "request_refusal")
        self.assertEqual(result[1]["document_id"], "synthetic-b")
        self.assertEqual(result[2]["terminal"], "unknown")

    def test_missing_terminal_remains_unknown_and_misordered_request_refuses(self):
        result = self.joined([{"request": 1, "status": "request_begin"}])
        self.assertEqual(result["rows"][0]["terminal"], "unknown")
        with self.assertRaises(AssertionError):
            self.joined([{"request": 2, "status": "request_begin"}])


if __name__ == "__main__":
    unittest.main()
