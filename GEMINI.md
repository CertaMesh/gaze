# GEMINI.md — Gaze project context for Gemini CLI

See [AGENTS.md](./AGENTS.md) for canonical project rules + the Gaze north star.

## Gemini-specific notes

- Gemini is used as a **second-opinion adversarial reviewer** in the Gaze project, not for primary implementation. If you've been dispatched for impl work, double-check the brief — it's likely a dispatch error.
- Known reliability gap (2026-04-24 calibration): Gemini sometimes stalls on long-running shell commands. If your session gets stuck, dump findings to stdout directly rather than waiting.
