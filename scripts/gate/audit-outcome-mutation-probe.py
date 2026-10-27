#!/usr/bin/env python3
"""Prove audit migration and CLI filter tests reject broken implementations.

Run from a clean, committed candidate with `anvil run -- python3
scripts/gate/audit-outcome-mutation-probe.py`. Each mutation is restored even
when its check fails. Logs stay in target/audit-outcome-mutations.
"""
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def run_probe(name, source, old, new, command):
    path = ROOT / source
    original = path.read_bytes()
    text = original.decode()
    if old not in text:
        raise RuntimeError(f"mutation anchor missing: {name}")
    env = os.environ.copy()
    env.update(CARGO_BUILD_JOBS="2", GAZE_DISABLE_OS_KEYSTORE="1")
    logs = ROOT / "target/audit-outcome-mutations"
    logs.mkdir(parents=True, exist_ok=True)
    try:
        path.write_text(text.replace(old, new, 1))
        with (logs / f"{name}.log").open("w") as log:
            result = subprocess.run(["nice", "-n", "10", "cargo", "test", *command],
                                    cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        output = (logs / f"{name}.log").read_text()
        if result.returncode == 0 or "test result: FAILED" not in output:
            raise RuntimeError(f"mutation was not killed by an assertion: {name}")
        print(f"KILLED {name}", flush=True)
    finally:
        path.write_bytes(original)


def main():
    subprocess.run(["git", "diff", "--quiet", "HEAD"], cwd=ROOT, check=True)
    run_probe("skip-outcome-migration", "crates/gaze-audit/src/sqlite.rs",
              'name: "outcome", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true',
              'name: "outcome", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: false',
              ["-p", "gaze-audit", "--test", "protection_outcomes",
               "outcome_migration_preserves_legacy_rows_and_is_idempotent", "--", "--exact"])
    run_probe("ignore-outcome-filter", "crates/gaze-audit/src/query.rs",
              'if let Some(value) = &filter.outcome {',
              'if let Some(value) = &None::<String> {',
              ["-p", "gaze-cli", "--test", "audit_outcomes",
               "audit_query_and_export_filter_outcome_and_reason_without_disclosing_keys", "--", "--exact"])
    subprocess.run(["git", "diff", "--quiet", "HEAD"], cwd=ROOT, check=True)
    print("MUTATIONS DONE: migration and filter mutations killed")


if __name__ == "__main__":
    main()
