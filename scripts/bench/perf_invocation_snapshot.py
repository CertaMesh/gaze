#!/usr/bin/env python3
"""Generate the model-free CLI hook snapshot and inventory projection (untimed)."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

from perf_hook_deletion import BASE_POLICY, CUSTOM_POLICY
from perf_lifecycle_contract import HOOK_AUDIT, RECOGNIZERS
from perf_attribution import child_environment, PREFIX, validate_trace

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "crates/gaze-cli/tests/fixtures"
MODEL_ONLY = frozenset((
    "nym_attach_ns", "nym_outer_verify_ns", "model_constructor_ns",
    "nym_model_constructor_ns", "bundle_verify_ns", "tokenizer_load_ns", "model_load_ns",
    "ner_tokenize_ns", "ner_encodes", "ner_mutex_wait_ns", "ner_ort_ns", "ner_windows",
    "nym_tokenize_ns", "nym_mutex_wait_ns", "nym_ort_ns", "nym_windows",
    "nym_stable_ns", "nym_neutral_ns", "nym_windows_stable", "nym_windows_neutral",
    "safety_view_build_ns", "safety_model_ns",
))


def inventory():
    hooks = {key for keys, _, _ in HOOK_AUDIT for key in keys}
    if not MODEL_ONLY <= hooks:
        raise ValueError("stale model-only inventory")
    return {"model_free": sorted(hooks - MODEL_ONLY), "model_only": sorted(MODEL_ONLY)}


def fixtures():
    def case(name, daemon, texts, extra="", context=False, error=False):
        requests = [{"session_id": "synthetic-snapshot", "text": text} for text in texts]
        if context:
            requests[0]["context"] = {}
        return {"id": name, "daemon": daemon, "policy": BASE_POLICY + extra,
                "requests": requests, "error": error}
    return [
        case("clean-email", False, ["alice@example.invalid"]),
        case("clean-context", False, ["alice@example.invalid"], context=True),
        case("clean-validator", False, ["Card: 4111 1111 1111 1111"]),
        case("clean-anchored", False, ["SYNTH-123"], CUSTOM_POLICY),
        case("daemon", True, ["", "alice@example.invalid", "alice@example.invalid", "plain prose"]),
        case("daemon-context", True, ["alice@example.invalid"], context=True),
        case("daemon-sweep", True, ["SYNTH-123", "prefix SYNTH-123"], CUSTOM_POLICY),
        case("daemon-empty-error", True, [""], CUSTOM_POLICY.replace("^SYNTH-[0-9]+$", "^$"), error=True),
    ]


def without_ns(row):
    row = validate_trace(row)
    for group in ("metrics", "recognizers"):
        for metric in row[group].values():
            del metric["ns"]
    return row


def capture(binary):
    result = {}
    with tempfile.TemporaryDirectory() as tmp:
        policy = Path(tmp) / "policy.toml"
        for case in fixtures():
            policy.write_text(case["policy"])
            daemon = case["daemon"]
            payload = ("".join(json.dumps(r) + "\n" for r in case["requests"]) if daemon
                       else case["requests"][0]["text"])
            args = [str(binary), "daemon" if daemon else "clean", "--policy", str(policy)]
            if not daemon and "context" in case["requests"][0]:
                context = Path(tmp) / "context.json"
                context.write_text(json.dumps(case["requests"][0]["context"]))
                args.extend(["--context-json", str(context)])
            run = subprocess.run(args,
                                 input=payload, capture_output=True, text=True,
                                 env=child_environment(), timeout=30)
            if daemon:
                responses = [json.loads(line) for line in run.stdout.splitlines()]
                assert run.returncode == 0 and len(responses) == len(case["requests"])
                assert all(bool(r.get("error")) == case["error"] for r in responses)
            else:
                assert (run.returncode != 0) == case["error"]
                if not case["error"]:
                    assert "clean_text" in json.loads(run.stdout)
            rows = [without_ns(json.loads(line[len(PREFIX):])) for line in run.stderr.splitlines()
                    if line.startswith(PREFIX)]
            assert len(rows) == len(case["requests"]) + 1
            result[case["id"]] = rows
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    outputs = {"perf-invocation-cases.json": fixtures(), "perf-hook-inventory.json": inventory()}
    if args.binary:
        outputs["perf-invocations.json"] = capture(args.binary.resolve())
    for name, value in outputs.items():
        path = FIXTURES / name
        if args.check:
            assert json.loads(path.read_text()) == value, f"stale {name}"
        else:
            path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
