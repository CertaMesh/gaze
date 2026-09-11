# Reference

Information-oriented and complete. Reference pages describe *what is* — exact commands,
schema fields, crate boundaries, metric surfaces, and benchmark evidence — without tutorial
narrative. Use them to look things up once you know what you're doing. For the reasoning
behind a contract, follow through to [explanation](../explanation/README.md).

## Surfaces

- **[CLI](cli.md)** — every `gaze` subcommand, flag, stdin/stdout protocol, and exit code.
- **[Policy schema](policy.md)** — the full `policy.toml` surface: rulepacks, custom
  recognizers, validators, normalizers, and locale gating.
- **[Redaction classes and recognizers](redaction-classes.md)** — the canonical,
  drift-gated inventory of emitted classes, embedded recognizers, validators,
  normalizers, collision precedence, conflict ordering, deterministic gaps, and
  shipped default activation.
- **[Example policy TOML](policy.example.toml)** — a copyable starter policy file.
- **[Crate map](crates.md)** — the published crates and what each owns; links to every crate README.
- **[Metrics catalog](metrics.md)** — the SSOT for every observable surface: audit-log
  columns, conflict tiers, pipeline counters, SafeBundle fields, MCP context, with stability
  guarantees per metric.
- **[Security review](security-review.md)** — the security invariants, each citing the named
  test that verifies it, plus the unverified bucket and explicit non-guarantees.
- **[Accessibility](accessibility.md)** — the accessibility posture of each Gaze surface.
- **[Dashboard browser security](dashboard/browser-security.md)** — the opt-in dashboard
  child's HTTP gate, headers, auth, and no-store guarantees.
- **[Dashboard accessibility & visual verification](dashboard/accessibility-and-visual-verification.md)** —
  the WCAG 2.2 protocol, 44-state visual matrix, and committed evidence.

## Benchmarks

Public benchmark claims must trace to the canonical
**[Gaze Benchmarks](benchmarks/README.md)** document. It carries the current
release's measured numbers as a table and as charts, the corpora and scoring
contract behind them, the safety-net and NER leaderboard surfaces, the commands
to reproduce every figure, and a per-release history. Numbers are regenerated
from each release's committed scorecard by
[`scripts/bench/render_benchmark_doc.py`](../../scripts/bench/render_benchmark_doc.py),
and CI fails if the document drifts from that evidence.
