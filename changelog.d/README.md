# Changelog fragments

Each PR describes its user-visible changes in its own fragment file here instead
of editing `CHANGELOG.md`. Parallel PRs then never conflict on the changelog.

Name a fragment `<name>.<section>.md`:

- `<name>` is lowercase letters, digits and hyphens, unique to the PR. Use the
  branch name or topic, for example `mcp-conversation-sessions`.
- `<section>` is one of `added`, `changed`, `deprecated`, `removed`, `fixed`,
  `security` or `benchmark`.

A fragment holds one or more Markdown list items, written as they should appear
in the changelog, without headings. Use several fragments for several sections:

```markdown
- `gaze mcp doctor` reports the declared protection profile and the session
  scope.
```

A bundled-snapshot change acknowledged by `bundle-tokenization-drift
--verify-ack` needs a `changed` fragment with a `[bundle-tokenization-drift]`
line that names each affected bundle.

`python3 scripts/release/changelog.py check` validates fragment names and
contents; CI runs it on every PR. At release time,
`python3 scripts/release/changelog.py assemble X.Y.Z --date YYYY-MM-DD` moves
every fragment into a new `## [X.Y.Z]` section of `CHANGELOG.md`, in Keep a
Changelog section order and file name order within a section, and deletes the
fragments.
