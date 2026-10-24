#!/usr/bin/env python3
"""Execute daemon v2 guard probes, restoring the committed source after each.

Run inside a prepared Anvil worktree with the machine's build slot reserved:
  anvil run -- env CARGO_BUILD_JOBS=2 python3 scripts/verification/daemon_v2_mutations.py
The script refuses dirty source, checks memory before each Cargo invocation,
requires an actual failing test (compile errors never count), and writes evidence
under target/daemon-v2-mutations. No benchmarks or model downloads run.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
CLI = "crates/gaze-cli/src/commands/daemon/v2.rs"
CORE = "crates/gaze/src/session.rs"
DAEMON = "crates/gaze-cli/src/commands/daemon.rs"
PREFIX = "commands::daemon::v2::tests::"

# Each substitution removes one admission, invalidation, or recovery guard.
PROBES = [
    ("profile", CLI, 'r.string("framing_profile")? != PROFILE', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("projection", CLI, 'projection_id != live.projection_id', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("pipeline", CLI, 'r.string("pipeline_id")? != self.v2.pipeline_id', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("epoch", CLI, '!self.v2.epochs_match(r, live)', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("sequence", CLI, 'seq != live.next_seq', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("receipt", CLI, 'request_id != receipt.request_id || digest != receipt.digest', "false", "opening_receipt_sequences_and_exact_frame_retries"),
    ("revision", CLI, 'r.counter("expected_conversation_revision")? != live.revision', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("generation", CLI, 'r.counter("expected_manifest_generation")? != live.generation', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("hash", CLI, 'hash(&text) != sha256', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("duplicate-block", CLI, '!ids.insert(id.clone())', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("resync-body", CLI, 'if resync {', 'if false {', "resync_requires_every_body_and_forbids_revision_preconditions"),
    ("block-count", CLI, 'requested.len() > limits.active_blocks', "false", "capacity_failures_are_atomic_and_do_not_consume_sequences"),
    ("block-bytes", CLI, 'text.len() > limits.block_raw_bytes', "false", "capacity_failures_are_atomic_and_do_not_consume_sequences"),
    ("active-raw", CLI, 'total > limits.active_raw_bytes', "false", "capacity_failures_are_atomic_and_do_not_consume_sequences"),
    ("request-frame", CLI, 'line.len() > self.v2.limits.v2_request_frame_bytes', "false", "bounded_request_and_response_edges_and_nonterminal_real_v2_frame_ceiling"),
    ("response-frame", CLI, 'bytes.len() > self.limit.saturating_sub(self.bytes.len())', "false", "bounded_request_and_response_edges_and_nonterminal_real_v2_frame_ceiling"),
    ("stabilization", CLI, 'before == transaction.semantic_revision()', "true", "late_last_block_revisits_earlier_text_and_oversize_inventory_never_partially_commits"),
    ("restore-generation", CLI, 'r.counter("minimum_manifest_generation")? > live.generation', "false", "restore_limits_generation_and_unknown_tokens_fail_closed"),
    ("manifest-reservation", CORE, 'if projected > limit {', 'if false {', "session::tests::staged_manifest_budget_refuses_before_mapping_growth_and_commit"),
    ("v1-isolation", DAEMON, 'self.v2.contains(&request.session_id)', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("counter-bound", CLI, 'value > MAX_COUNTER', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("payload-cache", CLI, 'raw_bytes(&blocks) > limits.session_cache_bytes', "false", "individual_state_budgets_refuse_before_commit"),
    ("session-cache", CLI, 'cache > limits.session_cache_bytes', "false", "individual_state_budgets_refuse_before_commit"),
    ("session-manifest", CLI, 'manifest > limits.session_manifest_bytes', "false", "individual_state_budgets_refuse_before_commit"),
    ("global-state", CLI, 'state > limits.global_v2_state_bytes', "false", "individual_state_budgets_refuse_before_commit"),
    ("pass-count", CLI, '0..limits.stabilization_passes', "0..8", "capacity_failures_are_atomic_and_do_not_consume_sequences"),
    ("deterministic-source", CORE, 'if state.deterministic_sweep {', 'if false {', "session::tests::deterministic_transaction_sweep_has_stable_equal_span_family_order"),

]


def command(args, logfile):
    pressure = subprocess.run(["memory_pressure"], capture_output=True, text=True, check=True).stdout
    free = re.search(r"System-wide memory free percentage:\s*(\d+)%", pressure)
    if not free or int(free[1]) < 30:
        raise RuntimeError("memory headroom below the required 30 percent")
    env = os.environ.copy()
    env["CARGO_BUILD_JOBS"] = "2"
    with logfile.open("w") as log:
        return subprocess.run(["nice", "-n", "10", "cargo", *args], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT).returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", help="one probe name")
    options = parser.parse_args()
    subprocess.run(["git", "diff", "--exit-code", "HEAD", "--", CLI, CORE, DAEMON], cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
    evidence = ROOT / "target/daemon-v2-mutations"
    evidence.mkdir(parents=True, exist_ok=True)
    probes = [p for p in PROBES if options.only is None or p[0] == options.only]
    if not probes:
        parser.error("unknown probe")
    results = []
    try:
        for name, file, before, after, test in probes:
            path = ROOT / file
            original = path.read_text()
            if original.count(before) != 1:
                raise RuntimeError(f"{name}: expected one mutation anchor")
            package = "gaze-pii" if file == CORE else "gaze-cli"
            full_test = test if file == CORE else PREFIX + test
            args = ["test", "-p", package, "--lib" if file == CORE else "--bin", *([] if file == CORE else ["gaze"]), full_test, "--", "--exact", "--test-threads=1"]
            baseline_log = evidence / f"{name}-baseline.log"
            if command(args, baseline_log):
                raise RuntimeError(f"{name}: baseline failed, see {baseline_log}")
            baseline = baseline_log.read_text()
            if "1 passed" not in baseline:
                raise RuntimeError(f"{name}: baseline executed no target test")
            try:
                path.write_text(original.replace(before, after, 1))
                logfile = evidence / f"{name}-mutant.log"
                status = command(args, logfile)
                output = logfile.read_text()
                caught = status != 0 and "test result: FAILED" in output and "1 failed" in output and "error[E" not in output
                results.append({"probe": name, "test": full_test, "caught": caught, "exit_code": status})
                print(f"{name}: {'CAUGHT' if caught else 'NOT CAUGHT'}", flush=True)
                if not caught:
                    raise RuntimeError(f"{name}: no executed failing test, see {logfile}")
            finally:
                path.write_text(original)
    finally:
        (evidence / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    subprocess.run(["git", "diff", "--exit-code", "HEAD", "--", CLI, CORE, DAEMON], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
