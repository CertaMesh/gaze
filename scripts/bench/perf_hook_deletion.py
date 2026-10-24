#!/usr/bin/env python3
"""Untimed inventory-driven deletion sweep over retained successful CLI records."""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import tempfile

import perf_attribution as perf
from perf_lifecycle_contract import HOOK_AUDIT, RECOGNIZERS


FIXTURE = Path(__file__).with_name("fixtures") / "perf-hook-deletions.json"
BASE_POLICY = ('schema_version="0.1.0"\n[session]\nscope="conversation"\n'
               '[policy.rulepacks]\nbundled=["core"]\n'
               '[[rule]]\nkind="default"\naction="tokenize"\n')
INPUTS = ("", "plain lowercase prose", "alice@example.invalid",
          "alice@example.invalid alice@example.invalid", "SYNTH-123", " \n\t",
          '{"email":"alice@example.invalid"}',
          "From: Dr. Schmidt <alice@example.invalid>\nDr. Schmidt",
          "Card: 4111 1111 1111 1111", "123 Example Street, 12")
MODEL_INPUT = "Dr. Schmidt was born on 1985-04-12. Email alice@example.invalid."
SWEEP_INPUTS = ("SYNTH-123", "prefix SYNTH-123")
CUSTOM_POLICY = ('[[policy.custom_recognizers]]\nkind="regex"\nname="synthetic-order"\n'
                 'class="custom:order_id"\npattern="^SYNTH-[0-9]+$"\n')

# Residual admission limits, AFTER policy-aware guarantees are checked.
# Hook-regression detection belongs to the committed CLI invocation snapshot.
ADMISSION_LIMITS = {
    RECOGNIZERS: "RecognizerSpan::drop creates dynamic entries and Rule totals together; schema 3 has no independent mandatory recognizer cardinality (perf.rs:274-313)",
    "input_bytes": "Without NER windows, sweep additions or no-net audit, anonymous candidate counts do not prove nonempty geometry (registry.rs:1264-1265)",
    "validators_ns": "registry.rs:1264-1265; validator_veto.rs:23-48; gaze-types/src/perf.rs:291-300: anonymous candidate totals precede filtering and carry no validator kind; Rule includes unvalidated custom regexes",
    "resolve2_ns": "pipeline.rs:1250-1284,1707-1713: optional street/address recovery or sweep; positive sweep_candidates does require it",
    "sweep_candidates": "When NER or address_blocks is active, resolve2 can instead come from street/address recovery (pipeline.rs:1250-1284)",
    "manifest_ns": "Without no-net audit, errors may end before Ledger::new; scan-only Nym paths can audit without a ledger (pipeline.rs:1034,1079,1326)",
    "audit_ns": "pipeline.rs:3067-3072,3097-3102: sink calls are conditional on a logger; no independent sink-call counter",
}


def deletions(records):
    """Erase just one emitted hook output, preserving every sibling field."""
    for index, record in enumerate(records):
        for hooks, sites, guarantee in HOOK_AUDIT:
            for key in hooks:
                targets = sorted(record["recognizers"]) if key == RECOGNIZERS else (key,)
                for target in targets:
                    mutant = copy.deepcopy(records)
                    changed = mutant[index]
                    if key == RECOGNIZERS:
                        output = changed["recognizers"][target]
                    elif key in perf.METRICS:
                        output = changed["metrics"][target]
                    else:
                        output = None
                    if output is None:
                        active = bool(changed["counters"][key])
                        changed["counters"][key] = 0
                    else:
                        active = any(output[field] for field in ("ns", "invocations", "candidates"))
                        output.update(ns=0, invocations=0, candidates=0)
                    yield key, target, index, active, sites + ": " + guarantee, mutant

        # One real producer: disabling perf_recognizer! removes every dynamic
        # entry AND the coupled rules aggregate, rather than leaving zero entries.
        mutant = copy.deepcopy(records)
        changed = mutant[index]
        active = bool(changed["recognizers"])
        changed["recognizers"] = {}
        changed["metrics"]["rules_ns"].update(ns=0, invocations=0, candidates=0)
        yield RECOGNIZERS, "all-with-rule-totals", index, active, "gaze/src/registry.rs:1263; gaze-types/src/perf.rs:274-313", mutant


def sweep(fixture: dict) -> dict:
    results = []
    with tempfile.TemporaryDirectory() as tmp:
        policy = Path(tmp) / "policy.toml"
        for case in fixture["cases"]:
            policy.write_text(case["policy"])
            rows = case["records"]
            perf.validate_lifecycle(rows, len(rows) - 1, daemon=True, policy=policy, required=True)
            for key, target, index, active, source, mutant in deletions(rows):
                try:
                    perf.validate_lifecycle(mutant, len(rows) - 1, daemon=True, policy=policy, required=True)
                except ValueError as error:
                    if not active:
                        raise AssertionError("unchanged positive refused") from error
                    status, reason = "refused", str(error)
                else:
                    status = "undetectable" if active else "not-fired"
                    reason = ADMISSION_LIMITS.get(key) if active else source
                    if reason is None:
                        raise AssertionError(f"unclassified hook deletion: {case['id']} / {index} / {key}")
                results.append({"case": case["id"], "record": index, "hook": key,
                                "target": target, "status": status, "reason": reason})
    return {"binary_sha256": fixture["binary_sha256"], "runner_sha256": perf.RUNNER_SHA256,
            "timed": False, "cases": len(fixture["cases"]), "deletions": results}


def capture(binary: Path, models: Path) -> dict:
    """Capture synthetic inputs only; keep responses and model paths out of fixtures."""
    expected = json.loads(Path(__file__).with_name("perf-lifecycle-synthetic-proof.json").read_text())
    if perf.file_sha(binary) != expected["timer_binary_sha256"]:
        raise ValueError("capture requires the retained new timer binary")
    model_policy = ('[ner]\nmodel_dir="/synthetic/ner"\n[dob_judge]\nenabled=true\n'
                    'model_dir="/synthetic/dob"\n[safety_net]\nbackend="nym"\n'
                    '[safety_net.nym]\nmodel_dir="/synthetic/nym"\n')
    cases = []
    with tempfile.TemporaryDirectory() as tmp:
        policy = Path(tmp) / "policy.toml"
        for name, texts, settings in (("ordinary", INPUTS, ""),
                                      ("ner-nym-dob", (MODEL_INPUT,), model_policy),
                                      ("repeat-sweep", SWEEP_INPUTS, CUSTOM_POLICY),
                                      ("partial-error", ("",), CUSTOM_POLICY.replace("^SYNTH-[0-9]+$", "^$"))):
            stored_policy = BASE_POLICY + settings
            actual_policy = stored_policy
            for role, directory in (("ner", "davlan-mbert-ner-hrl"),
                                    ("dob", "gliner-multi-pii-dob-int8"), ("nym", "nym-small-int8")):
                actual_policy = actual_policy.replace("/synthetic/" + role, str(models / directory))
            policy.write_text(actual_policy)
            requests = [{"session_id": f"synthetic-{i}", "text": text} for i, text in enumerate(texts)]
            if name == "repeat-sweep":
                for request in requests:
                    request["session_id"] = "synthetic-repeat"
            if name == "ordinary":
                requests.append({"session_id": "synthetic-context", "text": "safe text", "context": {}})
            run = subprocess.run([str(binary), "daemon", "--policy", str(policy)],
                                 input="".join(json.dumps(r) + "\n" for r in requests),
                                 capture_output=True, text=True, timeout=180, env=perf.child_environment())
            responses = [json.loads(line) for line in run.stdout.splitlines()]
            success = name != "partial-error"
            if (run.returncode or len(responses) != len(requests)
                    or not all(("clean_text" in r and not r.get("error")) if success
                               else bool(r.get("error")) for r in responses)):
                raise ValueError("synthetic capture failed; child diagnostics suppressed")
            rows = [json.loads(line[len(perf.PREFIX):]) for line in run.stderr.splitlines()
                    if line.startswith(perf.PREFIX)]
            perf.validate_lifecycle(rows, len(requests), daemon=True, policy=policy, required=True)
            for index, (request, row) in enumerate(zip(requests, rows[1:])):
                row["ordinal"] = 1
                cases.append({"id": f"{name}-{index}", "policy": stored_policy,
                              "input_sha256": perf.sha(request["text"].encode()),
                              "successful_response": success, "records": [rows[0], row]})
    return {"schema": 1, "timed": False, "binary_sha256": perf.file_sha(binary),
            "cases": [case for case in cases if case["successful_response"]],
            "partial_errors": [case for case in cases if not case["successful_response"]]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capture", type=Path, help="retained new timer CLI (no build)")
    parser.add_argument("--models", type=Path, default=Path.home() / ".local/share/gaze/models")
    parser.add_argument("--fixture", type=Path, default=FIXTURE)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if args.capture:
        args.fixture.write_text(json.dumps(capture(args.capture.resolve(), args.models.resolve()), indent=2) + "\n")
    result = sweep(json.loads(args.fixture.read_text()))
    args.out.write_text(json.dumps(result, indent=2) + "\n")
    print({status: sum(r["status"] == status for r in result["deletions"])
           for status in ("refused", "undetectable", "not-fired")})
