- Changelog entries now live in per-PR fragments under `changelog.d/`, so
  parallel PRs no longer conflict on `CHANGELOG.md`. The release PR runs
  `scripts/release/changelog.py assemble` to move them into the version
  section, and tag jobs refuse to publish while a fragment is left.
  `bundle-tokenization-drift --verify-ack` accepts its acknowledgement line in
  a `changed` fragment.
