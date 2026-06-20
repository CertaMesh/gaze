# Accessibility

Gaze treats accessibility as a baseline expectation, not a feature to be added later. The project consists of three surfaces today, each with its own accessibility posture.

## CLI (`gaze clean` / `gaze restore`)

The `gaze` binary is the primary user-facing surface. It operates over stdin/stdout with plain-text input and output, which makes it screen-reader friendly and compatible with assistive tooling that pipes terminal sessions. We deliberately:

- Never encode information through color alone. ANSI styling, when used in diagnostics, always carries the same information in the text itself (e.g. `[error]` prefixes, not just red text).
- Keep error messages self-describing — the reader does not need visual context (such as a TUI panel position) to understand what went wrong.
- Respect `NO_COLOR` and non-TTY environments by suppressing ANSI escapes. Human-facing CLI styling is behind one std `IsTerminal` gate: non-empty `NO_COLOR` disables ANSI, non-empty `CLICOLOR_FORCE` enables ANSI only when `NO_COLOR` is absent, and otherwise styling is emitted only for terminal streams. The `gaze mcp doctor` human table uses this gate for decorative state-column color while still printing the literal `pass`, `warn`, and `fail` words; `--json` output is never colored. Regression tests cover forced color, `NO_COLOR`, non-TTY stdout, text-only state words, and JSON output.

## Documentation

The docs in this repository are plain Markdown and follow conventions that work for screen readers and text-only renderers:

- Semantic heading levels (no skipped levels, one H1 per file).
- Alt text on any embedded image (architecture diagrams, screenshots).
- Code blocks always carry an explicit language tag (` ```rust `, ` ```toml `, ` ```bash `) so syntax highlighters and assistive tools can parse them correctly.
- Tables are kept simple (no merged cells, header row always present).

## Future UI surfaces

Gaze is a CLI-and-library project; there is no end-user UI in the core repository. The companion marketing site (`gaze-website`) and any future dashboard or audit-viewer UI built on top of `gaze-audit` will target **WCAG 2.1 AA conformance** as their accessibility baseline. This includes keyboard-only navigation, sufficient color contrast, focus indicators, and ARIA labels on interactive controls.

Accessibility regressions are treated like security regressions: they should not ship.
