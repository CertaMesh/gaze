"""Closed, aggregate-only proof for the optional OPF comparison being unavailable."""

from __future__ import annotations

import hashlib
import json
import re
from typing import Any, Mapping

REASON = "two_unchanged_600_second_socket_timeouts"


def digest(value: Any) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def unavailable_tools(report: Mapping[str, Any]) -> frozenset[str]:
    unavailable = report.get("unavailable", {})
    if not isinstance(unavailable, dict) or set(unavailable) - {"opf"}:
        raise ValueError("only optional OPF can have an unavailable receipt")
    if not unavailable:
        return frozenset()
    receipt = unavailable["opf"]
    keys = {"schema_version", "tool", "status", "reason", "measurement_revision",
            "comparison_adapter_sha256", "corpus_sha256", "contracts_sha256",
            "checkpoint_sha256", "runtime_source_revision", "attempts"}
    if not isinstance(receipt, dict) or set(receipt) != keys:
        raise ValueError("unavailable receipt schema must contain only aggregate receipts")
    if (receipt["schema_version"] != 1 or receipt["tool"] != "opf"
            or receipt["status"] != "unavailable" or receipt["reason"] != REASON):
        raise ValueError("unsupported unavailable receipt")
    if "opf" in report["tools"] or "opf" in report.get("skipped", {}):
        raise ValueError("unavailable OPF cannot also contain measured or skipped results")
    expected = {
        "measurement_revision": report.get("rescore_source_revision", report["harness_revision"]),
        "comparison_adapter_sha256": report.get("measurement_compare_sha256", report["compare_sha256"]),
        "corpus_sha256": digest(report["corpus"]),
        "contracts_sha256": digest(report["contracts"]),
    }
    if any(receipt[key] != value for key, value in expected.items()):
        raise ValueError("unavailable receipt differs from measured comparison inputs")
    for key in ("comparison_adapter_sha256", "corpus_sha256", "contracts_sha256", "checkpoint_sha256"):
        if not isinstance(receipt[key], str) or not re.fullmatch(r"[0-9a-f]{64}", receipt[key]):
            raise ValueError("unavailable receipt lacks SHA-256 pin")
    for key in ("measurement_revision", "runtime_source_revision"):
        if not isinstance(receipt[key], str) or not re.fullmatch(r"[0-9a-f]{40}", receipt[key]):
            raise ValueError("unavailable receipt lacks source revision")
    attempts = receipt["attempts"]
    if not isinstance(attempts, list) or len(attempts) != 2:
        raise ValueError("unavailable OPF requires exactly two unchanged attempts")
    for number, attempt in enumerate(attempts, 1):
        if (not isinstance(attempt, dict)
                or set(attempt) != {"attempt", "timeout_seconds", "outcome", "log_sha256", "configuration_sha256"}
                or type(attempt["attempt"]) is not int or attempt["attempt"] != number
                or type(attempt["timeout_seconds"]) is not int or attempt["timeout_seconds"] != 600
                or attempt["outcome"] != "socket_timeout"
                or any(not isinstance(attempt[key], str) or not re.fullmatch(r"[0-9a-f]{64}", attempt[key])
                       for key in ("log_sha256", "configuration_sha256"))):
            raise ValueError("invalid unavailable attempt receipt")
    configuration = digest({key: value for key, value in receipt.items() if key != "attempts"})
    if any(attempt["configuration_sha256"] != configuration for attempt in attempts):
        raise ValueError("unavailable OPF attempts did not use unchanged pinned configuration")
    if attempts[0]["log_sha256"] == attempts[1]["log_sha256"]:
        raise ValueError("unavailable OPF requires two distinct attempt receipts")
    return frozenset({"opf"})
