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
every fragment into a new `## [X.Y.Z]` section of `CHANGELOG.md` and deletes the
fragments. Entries join the first matching section; a new section is placed in
Keep a Changelog order. Within a section, fragments follow file name order.
Release tags refuse to publish while a fragment is left.

For a prerelease such as `1.0.0-rc.1`, `assemble` copies the fragments and any
text still under `[Unreleased]` into the prerelease section and keeps both, so
the final `1.0.0` section still lists every change. Each later prerelease
repeats the earlier ones' entries. Prerelease tags do not require the fragments
to be gone.
