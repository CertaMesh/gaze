# MCP host context mutation evidence

All six probes passed on 2026-10-25 at source commit
`c3fa04f79bf959ec3d444e5b3408363b06fa3e0d` using Rust 1.96.0,
two build jobs, and nice level 10.
Each baseline executed its named test and passed. Each mutant compiled and
executed the same test, which failed. Compile failures and zero-test runs did
not count. The runner restored the source after each probe and left a clean tree.

To reproduce from a clean signed checkout, run:

```sh
anvil run -- python3 scripts/gate/mcp-host-context-mutation-probe.py --run
```

The session-forgery test records the host's received audit ID, returns the tool
reply, and checks the ID after client cancellation and server completion.
It has no clock-based assertion.

The following lines come from the completed run. Every mutant also reached
Cargo's `Finished test profile` stage before its named test failed.

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
```

The runner writes complete logs and its result record under
`target/gate/mcp-host-context-mutations/`. This summary records mutation
coverage only. Workspace gates, detection benchmark certification, and resident
model latency and memory proof are separate requirements.
