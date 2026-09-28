"""The published report must identify its declared chart rows and timing limits."""

from __future__ import annotations

import unittest

from compare import TOOLS
from finalize_report import finalize


def row() -> dict:
    return {"latency": {"p50_ms": 1.0, "p95_ms": 2.0}}


def report() -> dict:
    return {
        "schema_version": 2,
        "tools": {name: {"contracts": {"v3": {"C": row()}}} for name in TOOLS},
        "skipped": {},
        "gaze": {"v3": {"layers": {"C": row()}}},
        "gaze_ablations": {"rules-only": {"v3": {"C": row()}}},
        "contention_samples": {
            name: {"contended": name == "opf", "valid": True, "foreign_cpu_samples": []}
            for name in (*TOOLS, "gaze-full", "gaze-rules-only", "gaze-rules-ner")
        },
    }


class FinalizeReportTest(unittest.TestCase):
    def test_complete_report_attaches_auditable_limits(self) -> None:
        result = finalize(report())
        self.assertEqual(len(result["chart_config_sha256"]), 64)
        self.assertEqual(len(result["finalizer_sha256"]), 64)
        self.assertFalse(result["latency_validity"]["publishable"])
        self.assertEqual(result["tools"]["opf"]["contracts"]["v3"]["C"]["latency"]["status"], "withheld")
        self.assertTrue(result["tools"]["opf"]["contracts"]["v3"]["C"]["latency"]["contended"])
        self.assertFalse(result["gaze"]["v3"]["layers"]["C"]["latency"]["contended"])

    def test_missing_configuration_cannot_be_finalized(self) -> None:
        incomplete = report()
        del incomplete["tools"]["opf"]
        with self.assertRaisesRegex(ValueError, "complete"):
            finalize(incomplete)


if __name__ == "__main__":
    unittest.main()
