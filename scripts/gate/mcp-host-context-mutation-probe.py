#!/usr/bin/env python3
"""Probe committed host-context contracts. No stash or zero-test mutation passes.

List plans by default. Run only after acquiring the shared Cargo slot. Each mutant
must compile, execute its named regression, and fail that regression. Source bytes
are restored in finally. The input tree must be clean and every commit signed.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
CORE = "crates/gaze-mcp-core/src/dispatch.rs"
PRIVATE = "crates/gaze-mcp-core/src/host_context.rs"
DICTIONARY = "crates/gaze-recognizers/src/dictionary.rs"
GUESS = "crates/gaze-recognizers/src/record_guess.rs"
FRONTEND = "crates/gaze-mcp-rmcp/src/frontend.rs"
RAW = "crates/gaze/src/context.rs"
SESSION = "crates/gaze/src/session.rs"
CONTRACT = ["cargo", "test", "-p", "gaze-mcp-core", "--test", "strict_contract"]


def replace(source, old, new):
    if source.count(old) != 1:
        raise RuntimeError(f"mutation anchor count {source.count(old)}")
    return source.replace(old, new)


def without_exclusions(source):
    return replace(source, "            exclude_argument_guesses(context, &argument_text)?;", "")


def without_joined_guesses(source):
    start = source.index("    for scalars in &material.scalars {")
    end = source.index("    let excluded = ", start)
    return source[:start] + source[end:]


def replace_opaque_name(source, body):
    start = source.index("fn opaque_name(")
    end = source.index("\n}\n", start) + 3
    signature = source[start:source.index("{", start) + 1]
    return source[:start] + signature + "\n" + body + "\n}\n" + source[end:]


def reveal_names(source):
    return replace_opaque_name(source, "let _ = (session, namespace); Ok(name.to_owned())")


def public_hash_names(source):
    return replace_opaque_name(source,
        'let _ = (session, namespace); use sha2::{Digest, Sha256}; '
        'Ok(format!("host_{}", &format!("{:x}", Sha256::digest(name.as_bytes()))[..32]))')


def public_record_names(source):
    start = source.index('        let hidden = if name.starts_with(gaze::RECORD_DICTIONARY_PREFIX) {')
    body = source.index('{', start) + 1
    end = source.index('        } else {', body)
    return source[:body] + '\n            name.clone()\n' + source[end:]


def trust_key_presence(source):
    start = source.index('pub(crate) fn validate_session(')
    body = source.index('{', start) + 1
    return source[:body] + '\n    if session.has_opaque_identifier_key() { return Ok(()); }\n' + source[body:]


def ignore_http(source):
    start = source.index('if let Some(parts) = extensions.get::<axum::http::request::Parts>()')
    tail = source[start:]
    return source[:start] + replace(tail, "return host.clone();", "return HostCallContext::default();")


PROBES = [
    ("rebuild-context-pipeline", PRIVATE,
     lambda source: replace(source, "        if !matches {", "        if true { let _ = matches;"),
     ["cargo", "test", "-p", "gaze-mcp-core", "--lib", "dispatch::tests::host_context_reuses_one_construction_across_calls_with_fresh_exclusions", "--", "--exact"],
     "dispatch::tests::host_context_reuses_one_construction_across_calls_with_fresh_exclusions"),
    ("duplicate-views", CORE,
     lambda source: replace(source,
                            "if material.scalars.first() != Some(&scalars) {",
                            "if true {"),
     ["cargo", "test", "-p", "gaze-mcp-core", "--lib", "dispatch::tests::host_argument_identical_views_scan_pairs_once", "--", "--exact"],
     "dispatch::tests::host_argument_identical_views_scan_pairs_once"),
    ("normalization-drift", GUESS,
     lambda source: replace(source,
                            "Self::from_normalized(crate::normalize::normalize(value).text)",
                            "Self::from_normalized(value.to_owned())"),
     CONTRACT + ["host_context_normalized_echo_has_no_membership_oracle_or_promotion"],
     "host_context::host_context_normalized_echo_has_no_membership_oracle_or_promotion"),
    ("split-argument-echo", CORE, without_joined_guesses,
     CONTRACT + ["host_context_joined_echo_has_no_membership_oracle_or_promotion"],
     "host_context::host_context_joined_echo_has_no_membership_oracle_or_promotion"),
    ("argument-budget", CORE,
     lambda source: replace(source,
                            "argument_guess_material(args, self.host_context_policy)?",
                            "argument_guess_material(args, crate::HostContextPolicy { max_argument_bytes: usize::MAX, max_argument_scalars: usize::MAX })?"),
     CONTRACT + ["host_context_enumeration_rejected_before_record_processing"],
     "host_context::host_context_enumeration_rejected_before_record_processing"),
    ("argument-scalars", CORE,
     lambda source: replace(source,
                            "if scalars.len() >= policy.max_argument_scalars {",
                            "if scalars.len() >= usize::MAX {"),
     CONTRACT + ["host_context_argument_budget_counts_bytes_scalars_and_decoded_views"],
     "host_context::host_context_argument_budget_counts_bytes_scalars_and_decoded_views"),
    ("argument-entries", CORE,
     lambda source: replace(source,
                            "entries = entries\n                .checked_add(1)\n                .filter(|size| *size <= policy.max_argument_bytes)",
                            "entries = entries\n                .checked_add(1)\n                .filter(|size| *size <= usize::MAX)"),
     CONTRACT + ["host_context_zero_byte_entries_rejected_before_record_processing"],
     "host_context::host_context_zero_byte_entries_rejected_before_record_processing"),
    ("guess-preparation", GUESS,
     lambda source: replace(source,
                            "        self.scan(&[left, separator, right], true);",
                            "        let _ = RecordGuessText::new(&left.normalized);\n        self.scan(&[left, separator, right], true);"),
     ["cargo", "test", "-p", "gaze-recognizers", "--lib", "record_guess::tests::record_guess_preparation_is_independent_of_terms_and_pairs", "--", "--exact"],
     "record_guess::tests::record_guess_preparation_is_independent_of_terms_and_pairs"),
    ("guess-case-mode", GUESS,
     lambda source: replace(source,
                            "compiled_match && allowed_kinds.contains(&kind)", "true"),
     CONTRACT + ["host_context_exact_guess_does_not_disclose_case_or_whitespace"],
     "host_context::host_context_exact_guess_does_not_disclose_case_or_whitespace"),
    ("guess-allowed-kinds", GUESS,
     lambda source: replace(source,
                            "compiled_match && allowed_kinds.contains(&kind)", "compiled_match"),
     ["cargo", "test", "-p", "gaze-recognizers", "--lib", "record_guess::tests::record_guess_allowed_kinds_and_compilation_modes_are_independent", "--", "--exact"],
     "record_guess::tests::record_guess_allowed_kinds_and_compilation_modes_are_independent"),
    ("guess-boundaries", GUESS,
     lambda source: replace(source,
                            "token_boundary(parts, start, end) && self.accepts(pattern, parts, start, end)",
                            "true && self.accepts(pattern, parts, start, end)"),
     CONTRACT + ["host_context_substring_guess_keeps_unguessed_record_protected"],
     "host_context::host_context_substring_guess_keeps_unguessed_record_protected"),
    ("folded-boundary-view", GUESS,
     lambda source: replace(source,
                            "part.normalized.as_str()", "part.folded.as_str()"),
     CONTRACT + ["host_context_substring_guess_keeps_unguessed_record_protected"],
     "host_context::host_context_substring_guess_keeps_unguessed_record_protected"),
    ("guess-whitespace", GUESS,
     lambda source: replace(source,
                            "if whitespace_count == 1 {",
                            "if whitespace_count == 1 || whitespace_count == 33 {"),
     CONTRACT + ["host_context_collapsed_whitespace_guess_has_no_membership_oracle"],
     "host_context::host_context_collapsed_whitespace_guess_has_no_membership_oracle"),
    ("drop-present", CORE,
     lambda source: replace(source, "let mut host_context = crate::host_context::parse(&host_context_selection)?;",
                            "let mut host_context = { drop(host_context); None::<gaze::Context> };"),
     CONTRACT + ["host_context_response_persists_only_matched_rule_sources_and_restores_exactly"],
     "host_context::host_context_response_persists_only_matched_rule_sources_and_restores_exactly"),
    ("echo-oracle", CORE, without_exclusions,
     CONTRACT + ["host_context_echo_has_no_membership_oracle_or_argument_promotion"],
     "host_context::host_context_echo_has_no_membership_oracle_or_argument_promotion"),
    ("reveal-identifiers", PRIVATE, reveal_names,
     CONTRACT + ["host_context_private_identifiers_never_reach_output_tools_or_audit"],
     "host_context::host_context_private_identifiers_never_reach_output_tools_or_audit"),
    ("raw-debug", RAW,
     lambda source: replace(source, 'f.write_str("RawContext([redacted])")',
                            'f.debug_struct("RawContext").field("fields", &self.fields).finish()'),
     ["cargo", "test", "-p", "gaze-pii", "--lib", "context::tests::raw_context_debug_is_value_free", "--", "--exact"],
     "context::tests::raw_context_debug_is_value_free"),
    ("model-session-forgery", FRONTEND,
     lambda source: replace(source, "let host = host_call_context(&context.extensions);",
                            "let mut host = host_call_context(&context.extensions);\n"
                            '        host.audit_session_id = request.arguments.as_ref().and_then(|args| args.get("_session_id")).and_then(serde_json::Value::as_str).map(str::to_owned);'),
     ["cargo", "test", "-p", "gaze-mcp-rmcp", "--test", "stdio_smoke", "stdio_dispatch_lists_and_calls_tools"],
     "stdio_dispatch_lists_and_calls_tools"),
    ("ignore-http-context", FRONTEND, ignore_http,
     ["cargo", "test", "-p", "gaze-mcp-rmcp", "--all-features", "--lib",
      "frontend::tests::embedded_http_reads_host_middleware_request_extensions", "--", "--exact"],
     "frontend::tests::embedded_http_reads_host_middleware_request_extensions"),
    ("public-hash-identifiers", PRIVATE, public_hash_names,
     CONTRACT + ["host_context_private_labels_are_session_scoped_and_defeat_public_hash_guesses"],
     "host_context::host_context_private_labels_are_session_scoped_and_defeat_public_hash_guesses"),
    ("public-record-identifiers", PRIVATE, public_record_names,
     CONTRACT + ["host_context_record_dictionary_identifiers_defeat_public_hash_guesses"],
     "host_context::host_context_record_dictionary_identifiers_defeat_public_hash_guesses"),
    ("omit-snapshot-opaque-key", SESSION,
     lambda source: replace(source, "            opaque_identifier_key,\n            scope:",
                            "            opaque_identifier_key: { drop(opaque_identifier_key); None },\n            scope:"),
     CONTRACT + ["host_context_private_labels_survive_trusted_snapshot_import"],
     "host_context::host_context_private_labels_survive_trusted_snapshot_import"),
    ("ignore-legacy-opaque-key", CORE,
     lambda source: replace(source, "crate::host_context::validate_session(self.session)?;",
                            "let _ = crate::host_context::validate_session(self.session);"),
     CONTRACT + ["host_context_legacy_opaque_snapshot_restores_but_dispatch_fails_closed"],
     "host_context::host_context_legacy_opaque_snapshot_restores_but_dispatch_fails_closed"),
    ("trust-key-presence", PRIVATE, trust_key_presence,
     CONTRACT + ["host_context_legacy_opaque_snapshot_restores_but_dispatch_fails_closed"],
     "host_context::host_context_legacy_opaque_snapshot_restores_but_dispatch_fails_closed"),
]


def resources():
    if shutil.disk_usage(ROOT).free < 100 * 1024**3:
        raise RuntimeError("free disk below 100 GiB")
    target = ROOT / "target"
    if target.exists():
        usage = subprocess.run(["du", "-sk", str(target)], text=True, capture_output=True)
        # Cargo removes temporary files during a live scan. Other scan failures
        # still reject the measurement; a vanished file has no remaining bytes.
        vanished = usage.stderr.splitlines()
        if usage.returncode and not (vanished and all(
            line.startswith(f"du: {target}/") and line.endswith(": No such file or directory")
            for line in vanished
        )):
            raise RuntimeError(f"target size scan failed: {usage.stderr.strip()}")
        kib = int(usage.stdout.split()[0])
        if kib > 15 * 1024**2:
            raise RuntimeError("target exceeds 15 GiB")
    pressure = subprocess.check_output(["memory_pressure"], text=True, timeout=10)
    match = re.search(r"System-wide memory free percentage:\s*(\d+)%", pressure)
    if not match:
        raise RuntimeError("could not read free-memory percentage")
    return os.getloadavg()[0] <= 25 and int(match[1]) >= 30


def run(command, log):
    while not resources():
        print("resource hold before start", flush=True)
        time.sleep(15)
    env = dict(os.environ, CARGO_BUILD_JOBS="2")
    with log.open("w") as output:
        child = subprocess.Popen(["nice", "-n", "10", *command], cwd=ROOT,
                                 env=env, stdout=output, stderr=subprocess.STDOUT,
                                 start_new_session=True)
        paused = False
        try:
            while child.poll() is None:
                admitted = resources()
                if admitted == paused:
                    os.killpg(child.pid, signal.SIGCONT if admitted else signal.SIGSTOP)
                    paused = not admitted
                    print("resource resume" if admitted else "resource pause", flush=True)
                time.sleep(5)
            return child.returncode
        finally:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGCONT)
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--probe", action="append", choices=[item[0] for item in PROBES])
    args = parser.parse_args()
    probes = [item for item in PROBES if not args.probe or item[0] in args.probe]
    if not args.run:
        print(json.dumps([{"probe": name, "source": path, "command": command, "test": test}
                          for name, path, _, command, test in probes], indent=2))
        return
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip():
        raise RuntimeError("commit all changes before running mutations")
    commit = subprocess.check_output(["git", "cat-file", "commit", "HEAD"], cwd=ROOT, text=True)
    if "\ngpgsig " not in commit:
        raise RuntimeError("HEAD must be signed")
    output = ROOT / "target/gate/mcp-host-context-mutations"
    output.mkdir(parents=True, exist_ok=True)
    results = []
    for name, path, mutate, command, test in probes:
        source = ROOT / path
        original = source.read_bytes()
        baseline_log = output / f"{name}-baseline.log"
        status = run(command, baseline_log)
        baseline = baseline_log.read_text()
        if status or not re.search(rf"test {re.escape(test)} \.\.\. ok", baseline):
            raise RuntimeError(f"{name}: baseline did not execute and pass its named test")
        try:
            source.write_text(mutate(original.decode()))
            mutant_log = output / f"{name}-mutant.log"
            status = run(command, mutant_log)
            mutant = mutant_log.read_text()
            killed = status != 0 and bool(re.search(rf"test {re.escape(test)} \.\.\. FAILED", mutant))
            results.append({"probe": name, "compiled_executed_and_killed": killed,
                            "baseline_log": str(baseline_log.relative_to(ROOT)),
                            "mutant_log": str(mutant_log.relative_to(ROOT))})
            (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
            if not killed:
                raise RuntimeError(f"{name}: mutant did not compile, ran zero tests, or survived")
        finally:
            source.write_bytes(original)
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
