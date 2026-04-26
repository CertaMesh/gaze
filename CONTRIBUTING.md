# Contributing

## Tenant class names in tests

Test fixtures and benchmark labels MUST use neutral class names (e.g. `class_alpha`, `tenant_class_a`, `dict_alpha`), never tenant-specific patterns like `order_id`, `Order_42`, `Song_42`, `User_7`. Rationale: drawer `eac549ae` — gaze core has no built-in tenant knowledge.

The `cargo run -p xtask -- no-tenant-knowledge` gate scans production Rust code in `crates/{gaze,gaze-recognizers,gaze-assembly,gaze-cli}/src/**/*.rs` and fails on those tenant-specific patterns. It intentionally does not scan `tests/`, `benches/`, docs, `CONTRIBUTING.md`, `crates/xtask/`, or `debug-proxy/`.

Use `// allow(tenant-fixture)` only in tests, benches, or docs when a tenant-like fixture is necessary to exercise behavior. That marker is a production-bypass attempt in `crates/*/src/` and hard-fails the gate with `AllowMarkerInProductionScope`.

The `order_id` denylist is intentionally broad — it catches `Order_42`, `order_ids`, etc. If a legitimate production identifier (e.g. `order_history_index_id` for an unrelated subsystem) collides with the denylist post-v0.4.3, coordinate with maintainers to add to allowlist with rationale comment. Do NOT silently bypass via `// allow(tenant-fixture)` in production code — that marker hard-fails the gate (drawer `eac549ae`).

Round-trip, three-surfaces, and recognizer-composition cross-cutting rows are N/A for this structural gate: it emits no tokens, adds no runtime knobs, and does not compose recognizers. The no-tenant-knowledge row is enforced by CI so production code must pass post-merge.

## Phone-number fixtures

Test and benchmark fixtures that contain phone numbers MUST use synthetic, non-reachable values from documented reservation ranges:

- US/NA fixtures: NANPA "555" exchanges (`+1-555-01xx` etc.), reserved for fictional use under [NANPA reservation 555-01xx](https://nationalnanpa.com/).
- UK fixtures: Ofcom drama-reserved ranges (`+44-7700-900xxx`), per [Ofcom drama numbers guidance](https://www.ofcom.org.uk/phones-and-broadband/phone-numbers/numbers-for-drama).
- Other locales: synthesize a non-reachable shape (e.g. exchange code `0` or out-of-band country code) and add a fixture comment noting the synthetic origin.

Rationale: drawer `gaze_decisions_e1ab6dc0`. Real reachable numbers in test
fixtures risk inadvertent leakage into adopter telemetry, public CI logs, and
crate metadata. The `phonenumber` parser-backed `E164Phone` validator
(v0.4.4 S3a) accepts the NANPA 555 reservation and Ofcom drama ranges as valid
E.164, so positive-path tests continue to exercise the validator without using
real numbers.
