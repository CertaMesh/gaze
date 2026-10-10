# Reference

Look up commands, configuration, contracts and evidence. For design reasoning,
see [Explanation](../explanation/README.md).

## Commands and configuration

- [CLI](cli.md): commands, flags, protocols and exit codes.
- [Policy schema](policy.md): TOML fields, recognizers, validators and locale rules.
- [Example policy](policy.example.toml): starter TOML.
- [Redaction classes and recognizers](redaction-classes.md): drift-checked inventory,
  activation, conflict order and detection gaps.

## Observability

[Metrics catalog](metrics.md): audit columns, conflict tiers, counters,
SafeBundle fields and MCP context, with stability rules.

## Crates

[Crate map](crates.md): ownership, dependencies and entry points.

## Security and accessibility

- [Security review](security-review.md): tested invariants, unverified claims and limits.
- [Accessibility](accessibility.md): CLI, docs and UI requirements.
- [Dashboard browser security](dashboard/browser-security.md): HTTP checks, auth and leases.
- [Dashboard verification](dashboard/accessibility-and-visual-verification.md):
  accessibility checks, 44-state matrix and evidence limits.

## Benchmarks

[Gaze Benchmarks](benchmarks/README.md) holds the measured release results,
scoring contracts, model comparisons and reproduction commands. Public claims
must trace to its committed scorecards. CI checks the page with
[`render_benchmark_doc.py`](../../scripts/bench/render_benchmark_doc.py).
