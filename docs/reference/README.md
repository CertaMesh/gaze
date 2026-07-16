# Reference

Information-oriented and complete. Reference pages describe *what is* — exact commands,
schema fields, crate boundaries, metric surfaces, and benchmark evidence — without tutorial
narrative. Use them to look things up once you know what you're doing. For the reasoning
behind a contract, follow through to [explanation](../explanation/README.md).

## Surfaces

- **[CLI](cli.md)** — every `gaze` subcommand, flag, stdin/stdout protocol, and exit code.
- **[Policy schema](policy.md)** — the full `policy.toml` surface: rulepacks, custom
  recognizers, validators, normalizers, and locale gating.
- **[Example policy TOML](policy.example.toml)** — a copyable starter policy file.
- **[Crate map](crates.md)** — the published crates and what each owns; links to every crate README.
- **[Metrics catalog](metrics.md)** — the SSOT for every observable surface: audit-log
  columns, conflict tiers, pipeline counters, SafeBundle fields, MCP context, with stability
  guarantees per metric.
- **[Security review](security-review.md)** — the security invariants, each citing the named
  test that verifies it, plus the unverified bucket and explicit non-guarantees.
- **[Accessibility](accessibility.md)** — the accessibility posture of each Gaze surface.

## Benchmarks

Public benchmark claims must trace to the canonical
**[benchmark index and methodology](benchmarks/README.md)**. It links every
committed benchmark report and scorecard through v0.12, classifies current,
supplemental, contractual, and historical evidence, and inventories the
runners, locked configuration, sources, snapshots, metric directions, and
production goals.
