# MCP host context mutation evidence

All eleven probes passed on 2026-10-25 at source commit
`011153f3c2a9fa28b6202e64dc2c8b05a5c1cb54` using Rust 1.96.0,
two build jobs, nice level 10, no debug information and no incremental compilation.
Every baseline executed its named test and passed. Every mutant compiled and
executed the same test, which failed. Compile failures and zero-test runs did
not count. The runner restored the source and left a clean tree.

The public-hash regression first failed on the old implementation because a
public SHA-256 guess recovered a private identifier. A second RED test showed
that allocating an unrelated new key could bypass a key-presence-only legacy
guard. Session-keyed, authenticated labels close both paths. Trusted snapshot v8
preserves their namespace; keyless legacy tokens still restore and fail closed
for dispatch. The stdio test uses transport completion events, with no clock assertion.

To reproduce from a clean signed checkout, run:

```sh
anvil run -- env CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
	CARGO_INCREMENTAL=0 python3 scripts/gate/mcp-host-context-mutation-probe.py --run
```

These are the completed run's named test lines. Every mutant also reached
Cargo's `Finished test profile` stage before its test failed.

```text
drop-present
  baseline: test host_context::host_context_response_persists_only_matched_rule_sources_and_restores_exactly ... ok
  mutant:   test host_context::host_context_response_persists_only_matched_rule_sources_and_restores_exactly ... FAILED

echo-oracle
  baseline: test host_context::host_context_echo_has_no_membership_oracle_or_argument_promotion ... ok
  mutant:   test host_context::host_context_echo_has_no_membership_oracle_or_argument_promotion ... FAILED

reveal-identifiers
  baseline: test host_context::host_context_private_identifiers_never_reach_output_tools_or_audit ... ok
  mutant:   test host_context::host_context_private_identifiers_never_reach_output_tools_or_audit ... FAILED

raw-debug
  baseline: test context::tests::raw_context_debug_is_value_free ... ok
  mutant:   test context::tests::raw_context_debug_is_value_free ... FAILED

model-session-forgery
  baseline: test stdio_dispatch_lists_and_calls_tools ... ok
  mutant:   test stdio_dispatch_lists_and_calls_tools ... FAILED

ignore-http-context
  baseline: test frontend::tests::embedded_http_reads_host_middleware_request_extensions ... ok
  mutant:   test frontend::tests::embedded_http_reads_host_middleware_request_extensions ... FAILED

public-hash-identifiers
  baseline: test host_context::host_context_private_labels_are_session_scoped_and_defeat_public_hash_guesses ... ok
  mutant:   test host_context::host_context_private_labels_are_session_scoped_and_defeat_public_hash_guesses ... FAILED

public-record-identifiers
  baseline: test host_context::host_context_record_dictionary_identifiers_defeat_public_hash_guesses ... ok
  mutant:   test host_context::host_context_record_dictionary_identifiers_defeat_public_hash_guesses ... FAILED

omit-snapshot-opaque-key
  baseline: test host_context::host_context_private_labels_survive_trusted_snapshot_import ... ok
  mutant:   test host_context::host_context_private_labels_survive_trusted_snapshot_import ... FAILED

ignore-legacy-opaque-key
  baseline: test host_context::host_context_legacy_opaque_snapshot_restores_but_dispatch_fails_closed ... ok
  mutant:   test host_context::host_context_legacy_opaque_snapshot_restores_but_dispatch_fails_closed ... FAILED

trust-key-presence
  baseline: test host_context::host_context_legacy_opaque_snapshot_restores_but_dispatch_fails_closed ... ok
  mutant:   test host_context::host_context_legacy_opaque_snapshot_restores_but_dispatch_fails_closed ... FAILED
```

Complete logs and the result record are under
`target/gate/mcp-host-context-mutations/`. This summary records mutation
coverage. Full workspace gates, detection benchmark certification, and resident
model latency and memory proof are separate requirements.

## Bounded argument exclusion regressions

Source commit `de3a4bbeaf5090587c43827667849e507bafa08d` adds record-independent
argument admission and shares the response matcher's normalization, fold offsets,
and boundary function. The original response matcher retains its whitespace-run
limit. Exclusion collapses longer runs within the admitted argument byte budget.

The enumeration, substring, pair-order, expanding-case-fold, and long-whitespace
regressions failed before their fixes and pass afterward. U+0345 echo coverage also
passes. The expanding-case-fold failure uses U+0130, whose fold inserts a combining
mark and previously made a substring appear to start at a word boundary. All 23
host-context tests and three focused recognizer tests pass for this implementation.
The latter include a check that the response matcher's 32-character whitespace
limit stays unchanged while guess exclusion accepts longer runs.

The named Anvil check runs seven baseline/mutant pairs:

| Probe | Regression contract |
| --- | --- |
| `normalization-drift` | Fullwidth and normalized echoes cannot reveal record membership. |
| `split-argument-echo` | Scalar joins, including decoded JSON leaves, cannot reveal membership. |
| `argument-budget` | A de Bruijn enumeration is rejected before any record-dependent step. |
| `argument-scalars` | The scalar count bounds original and decoded argument views. |
| `guess-boundaries` | A superstring does not exclude an unguessed record term. |
| `folded-boundary-view` | Boundary checks use the response matcher's normalized text view. |
| `guess-whitespace` | Tool-collapsed whitespace cannot reopen echo membership detection. |

Run from a clean signed checkout, after acquiring the shared build slot:

```sh
anvil check mcp-host-echo --allow-expensive --lock-wait 30m --json
```

Each baseline must execute and pass its named test. Each mutant must compile,
execute that test, and fail it. The runner restores source bytes in `finally`.
The Anvil result records the tested commit and input provenance. Per-probe logs and
`results.json` are written to `target/gate/mcp-host-context-mutations/`. The PR body
records the completed run's candidate and result. Benchmark certification remains
a separate requirement before this draft can merge.
