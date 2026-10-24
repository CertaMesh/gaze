#!/usr/bin/env python3
"""Remove admission checks in isolated Python copies and require executed failures."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from perf_lifecycle_contract import CLOCK_CHECKS, REQUEST_ASSEMBLY, OWNERSHIP_CHECKS
import perf_attribution as perf

ROOT = Path(__file__).resolve().parent


def prove(out: Path) -> dict:
    source = (ROOT / "perf_attribution.py").read_text()
    mutations = [(check.id, source.replace("for check in CLOCK_CHECKS:",
                  f"for check in (c for c in CLOCK_CHECKS if c.id != {check.id!r}):"),
                  ("test_serial_" + check.id.removesuffix("-serial").replace("-", "_")
                   if check.kind == "serial" else "test_parent_" + check.id.replace("-", "_")))
                 for check in CLOCK_CHECKS]
    ownership_tests = {
        "rule-owner-ns": "test_reviewer_rules_must_equal_rule_spans",
        "rule-owner-invocations": "test_rule_owner_invocations",
        "rule-owner-candidates": "test_rule_owner_candidates",
        "ner-owner-capacity": "test_ner_owner_serial_nonzero_leaves",
        "recognizer-role": "test_recognizer_role",
        "stable-recognizer-role": "test_stable_recognizer_role",
    }
    for check in OWNERSHIP_CHECKS:
        mutations.append((check, source.replace("for check in OWNERSHIP_CHECKS:",
                          f"for check in (c for c in OWNERSHIP_CHECKS if c != {check!r}):"),
                          ownership_tests[check]))
    mutations.extend([
        ("clock-phase", source.replace('record["phase"] not in CLOCK_PHASES[key]', 'False'),
         "test_phase_registry_ns"),
        ("request-assembly-counts", source.replace('if assembly_counts not in ({0}, {1}):', 'if False:'),
         "test_request_assembly_hooks_are_complete_and_once_each"),
        ("startup-work", source.replace('if record["phase"] == "startup" and (any(', 'if False and (any('),
         "test_startup_work_counters_are_refused"),
        ("startup-recognizers", source.replace('if record["phase"] == "startup" and (any(', 'if False and (any('),
         "test_startup_recognizers_are_refused"),
        ("invocation", source.replace('if metric["ns"] and not metric.get("invocations", 0):', 'if False:'),
         "test_nonzero_clock_requires_invocation"),
    ])
    for key in sorted(REQUEST_ASSEMBLY):
        mutations.append(("one-shot-phase-" + key,
                          source.replace('and key in REQUEST_ASSEMBLY',
                                         f'and key in (REQUEST_ASSEMBLY - {{{key!r}}})'),
                          "test_one_shot_phase_" + key))
    results = []
    out.mkdir(parents=True, exist_ok=True)
    for name, mutated, test in mutations:
        if mutated == source:
            raise AssertionError(f"mutation did not change source: {name}")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "perf_attribution.py").write_text(mutated)
            (root / "perf_lifecycle_contract.py").write_bytes((ROOT / "perf_lifecycle_contract.py").read_bytes())
            env = {**os.environ, "PYTHONPATH": str(root) + os.pathsep + str(ROOT),
                   "PYTHONDONTWRITEBYTECODE": "1"}
            selected = f"test_perf_lifecycle_bounds.BoundTests.{test}"
            run = subprocess.run([sys.executable, "-m", "unittest", "-v", selected],
                                 cwd=root, env=env, capture_output=True, text=True)
            log = run.stdout + run.stderr
            (out / f"{name}.log").write_text(log)
            if (run.returncode != 1 or "AssertionError: ValueError not raised" not in log
                    or "Ran 1 test" not in log or "FAILED (failures=1)" not in log):
                raise AssertionError(f"mutation must fail one executed assertion: {name}; see its log")
            (root / "perf_attribution.py").write_text(source)
            restored = subprocess.run([sys.executable, "-m", "unittest", "-v", selected],
                                      cwd=root, env=env, capture_output=True, text=True)
            log = restored.stdout + restored.stderr
            (out / f"{name}-restored.log").write_text(log)
            if restored.returncode != 0 or "Ran 1 test" not in log or "\nOK\n" not in log:
                raise AssertionError(f"restored assertion did not pass: {name}")
            results.append({"check": name, "test": selected, "mutated_assertions_failed": 1,
                            "restored_tests_passed": 1})
    receipt = {"runner_sha256": perf.RUNNER_SHA256,
               "runner_source_sha256": hashlib.sha256(source.encode()).hexdigest(),
               "contract_sha256": hashlib.sha256((ROOT / "perf_lifecycle_contract.py").read_bytes()).hexdigest(),
               "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
               "mutations": results, "mutation_count": len(results),
               "serial_sum_checks": sum(c.kind == "serial" for c in CLOCK_CHECKS),
               "parent_checks": sum(c.kind == "parent" for c in CLOCK_CHECKS),
               "ownership_checks": len(OWNERSHIP_CHECKS),
               "parent_test_isolation": "Each parent check alone; serial sums and ancestor edges otherwise imply it. Full graph also rejects every parent witness.",
               "executed_assertion_failures": len(results), "restored_passes": len(results)}
    (out / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    receipt = prove(args.out)
    print(f"{receipt['mutation_count']} mutations: executed assertion failure and restored pass each")
