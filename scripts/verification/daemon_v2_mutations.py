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
from datetime import datetime

ROOT = Path(__file__).resolve().parents[2]
CLI = "crates/gaze-cli/src/commands/daemon/v2.rs"
CORE = "crates/gaze/src/session.rs"
DAEMON = "crates/gaze-cli/src/commands/daemon.rs"
PREFIX = "commands::daemon::v2::tests::"

# Each substitution removes one admission, invalidation, or recovery guard.
PROBES = [
    ('audit-reservation', DAEMON, 'requested > self.limit', 'false', 'deferred_logger_refuses_before_collecting_out_of_budget_entries'),
    ('audit-metadata-budget', DAEMON, 'buffer.reserve_entry(entry)?;', '();', 'deferred_audit_metadata_cannot_exceed_the_transaction_state_budget'),
    ('audit-report-budget', CLI, 'report_allocation_bytes(report)?', '0', 'deferred_safety_reports_cannot_exceed_the_transaction_state_budget'),
    ('audit-stable-pass', CLI, 'audit.reset_pass().map_err(|_| Failure::Pipeline)?;', '();', 'audit_counts_only_the_committed_stable_pass_and_no_refused_attempts'),
    ('audit-defer', DAEMON, 'buffer.entries.push(entry.clone());', 'self.log_committed(entry)?;', 'audit_counts_only_the_committed_stable_pass_and_no_refused_attempts'),
    ('audit-flush', CLI, 'audit.flush().map_err(|_| Failure::Pipeline)?;', 'drop(audit);', 'audit_counts_only_the_committed_stable_pass_and_no_refused_attempts'),
    ('audit-refused-validation', DAEMON, 'buffer.entries.push(entry.clone());', 'self.log_committed(entry)?;', 'validation_refusal_discards_audit_rows_and_staged_state'),
    ('audit-safety-report', CLI, 'reports.push(report);', 'self.logger.log_safety_net_report(&report, &live.session, DocumentKind::Text).map_err(|_| Failure::Pipeline)?;\n                reports.push(report);', 'safety_net_audit_counts_only_final_pass_and_discards_refused_reports'),
    ('open-admission', CLI, '        if self\n            .v2\n            .bytes_except(&session_id)\n            .0\n            .saturating_add(live.bytes())\n            .saturating_add(response.get().len())\n            > self.v2.limits.global_v2_state_bytes\n        {\n            return Err(Failure::CacheCapacity);\n        }\n        self.make_session_room();\n', '        self.make_session_room();\n        if self\n            .v2\n            .bytes_except(&session_id)\n            .0\n            .saturating_add(live.bytes())\n            .saturating_add(response.get().len())\n            > self.v2.limits.global_v2_state_bytes\n        {\n            return Err(Failure::CacheCapacity);\n        }\n', 'refused_open_keeps_existing_namespaces_and_restore_mappings'),
    ('idle-eviction-audit', DAEMON, 'self.log_eviction(&session, "idle_timeout");', 'let _ = session;', 'v2_idle_and_lru_evictions_publish_the_v1_audit_metadata'),
    ('lru-eviction-audit', CLI, 'if let Some(entry) = self.v2.sessions.remove(&id) {\n                        self.log_eviction(&entry.session, "lru");\n                    }', 'self.v2.sessions.remove(&id);', 'v2_idle_and_lru_evictions_publish_the_v1_audit_metadata'),
    ('mixed-eviction-audit', CLI, 'if let Some(entry) = self.v2.sessions.remove(&other) {\n                        self.log_eviction(&entry.session, "lru");\n                    }', 'self.v2.sessions.remove(&other);', 'v2_idle_and_lru_evictions_publish_the_v1_audit_metadata'),
    ('eviction-warning', DAEMON, 'tracing::warn!(audit_session_id = %audit_session_id, reason = %reason, "gaze daemon evicted session");', '', 'v2_idle_and_lru_evictions_publish_the_v1_audit_metadata'),
    ('independent-oracle', CLI, 'before == transaction.semantic_revision()', 'true', 'committed_v1_oracle_recleans_late_learned_blocks'),
    ("tombstone-digest", CLI, '&& t.receipt.digest == digest', '', "strict_restore_deletion_compaction_and_close_tombstone"),
    ("cross-protocol-lru", CLI, 'other_time < time', 'false', "shared_cap_selects_the_oldest_namespace_in_both_protocol_orders"),
    ("profile", CLI, 'r.string("framing_profile")? != PROFILE', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("projection", CLI, 'projection_id != live.projection_id', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("pipeline", CLI, 'r.string("pipeline_id")? != self.v2.pipeline_id', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("epoch", CLI, '!self.v2.epochs_match(r, live)', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("sequence", CLI, 'seq != live.next_seq', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("receipt", CLI, 'request_id != receipt.request_id || digest != receipt.digest', "false", "opening_receipt_sequences_and_exact_frame_retries"),
    ("revision", CLI, 'r.counter("expected_conversation_revision")? != live.revision', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("generation", CLI, 'r.counter("expected_manifest_generation")? != live.generation', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("hash", CLI, 'hash(&text) != sha256', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("duplicate-block", CLI, '!ids.insert(id.clone())', "ids.insert(id.clone()) && false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("resync-body", CLI, 'if resync {', 'if false {', "resync_requires_every_body_and_forbids_revision_preconditions"),
    ("block-count", CLI, 'requested.len() > limits.active_blocks', "false", "capacity_failures_are_atomic_and_do_not_consume_sequences"),
    ("block-bytes", CLI, 'text.len() > limits.block_raw_bytes', "false", "capacity_failures_are_atomic_and_do_not_consume_sequences"),
    ("active-raw", CLI, 'total > limits.active_raw_bytes', "false", "capacity_failures_are_atomic_and_do_not_consume_sequences"),
    ("request-frame", CLI, 'line.len() > self.v2.limits.v2_request_frame_bytes', "false", "bounded_request_and_response_edges_and_nonterminal_real_v2_frame_ceiling"),
    ("response-frame", CLI, 'bytes.len() > self.limit.saturating_sub(self.bytes.len())', "false", "bounded_request_and_response_edges_and_nonterminal_real_v2_frame_ceiling"),
    ("stabilization", CLI, 'before == transaction.semantic_revision()', "true", "late_last_block_revisits_earlier_text_and_oversize_inventory_never_partially_commits"),
    ("restore-generation", CLI, 'r.counter("minimum_manifest_generation")? > live.generation', "false", "restore_limits_generation_and_unknown_tokens_fail_closed"),
    ("manifest-reservation", CORE, 'if projected > limit {', 'if false {', "session::repeat_query_purity_tests::staged_manifest_budget_refuses_before_mapping_growth_and_commit"),
    ("v1-isolation", DAEMON, 'self.v2.contains(&request.session_id)', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("counter-bound", CLI, 'value > MAX_COUNTER', "false", "invalid_protocol_objects_never_fall_back_to_v1"),
    ("payload-cache", CLI, 'raw_bytes(&blocks) > limits.session_cache_bytes', "false", "individual_state_budgets_refuse_before_commit"),
    ("session-cache", CLI, 'cache > limits.session_cache_bytes', "false", "individual_state_budgets_refuse_before_commit"),
    ("session-manifest", CLI, 'manifest > limits.session_manifest_bytes', "false", "individual_state_budgets_refuse_before_commit"),
    ("global-state", CLI, 'state > limits.global_v2_state_bytes', "false", "individual_state_budgets_refuse_before_commit"),
    ("pass-count", CLI, '0..limits.stabilization_passes', "0..8", "capacity_failures_are_atomic_and_do_not_consume_sequences"),
    ("deterministic-source", CORE, 'if state.deterministic_sweep {', 'if false {', "session::repeat_query_purity_tests::deterministic_transaction_sweep_has_stable_equal_span_family_order"),

    ("global-manifest", CLI, '> limits.global_manifest_bytes', "> usize::MAX", "individual_state_budgets_refuse_before_commit"),
    ("context", CLI, '.get("context")', '.get("absent_context")', "context_is_validated_on_every_request_and_does_not_carry_unmatched_values"),
    ("duplicate-key", CLI, 'fields.insert(key, value).is_some()', 'fields.insert(key, value).is_some() && false', "invalid_protocol_objects_never_fall_back_to_v1"),

]


def command(args, logfile):
    pressure = subprocess.run(["memory_pressure"], capture_output=True, text=True, check=True).stdout
    free = re.search(r"System-wide memory free percentage:\s*(\d+)%", pressure)
    if not free or int(free[1]) < 50:
        raise RuntimeError("memory headroom below the required 50 percent")
    if os.getloadavg()[0] > 25:
        raise RuntimeError("load1 exceeds the required ceiling of 25")
    now = datetime.now().time()
    if datetime.strptime("15:25", "%H:%M").time() <= now < datetime.strptime("15:45", "%H:%M").time():
        raise RuntimeError("Cargo is paused during the 15:25-15:45 quiet window")
    env = os.environ.copy()
    env["CARGO_BUILD_JOBS"] = "2"
    with logfile.open("w") as log:
        return subprocess.run(["nice", "-n", "10", "cargo", *args], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT).returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", action="append", help="probe name; repeat to select several")
    parser.add_argument("--output-dir", default="target/daemon-v2-mutations", help="evidence directory")
    options = parser.parse_args()
    subprocess.run(["git", "diff", "--exit-code", "HEAD"], cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
    evidence = ROOT / options.output_dir
    evidence.mkdir(parents=True, exist_ok=True)
    probes = [p for p in PROBES if options.only is None or p[0] in options.only]
    if not probes:
        parser.error("unknown probe")
    (evidence / "provenance.json").write_text(json.dumps({
        "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "probes": [p[0] for p in probes],
        "rustc": subprocess.check_output(["rustc", "-Vv"], text=True).strip(),
        "cargo": subprocess.check_output(["cargo", "-V"], text=True).strip(),
        "profile_dev_debug": os.environ.get("CARGO_PROFILE_DEV_DEBUG", "default"),
        "profile_test_debug": os.environ.get("CARGO_PROFILE_TEST_DEBUG", "default"),
        "incremental": os.environ.get("CARGO_INCREMENTAL", "default"),
        "started_at": datetime.now().isoformat(),
    }, indent=2) + "\n")
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
    subprocess.run(["git", "diff", "--exit-code", "HEAD"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
