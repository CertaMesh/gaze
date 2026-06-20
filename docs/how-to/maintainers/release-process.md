# Release Process

`EmpireTwo/gaze` is a public repository. Two release channels are live for adopters; Homebrew remains repo-local pending a public tap.

## Public release channels (live)

### GitHub Releases

Source: [`.github/workflows/release.yml`](../../../.github/workflows/release.yml).

- Triggered on `v*` tag pushes.
- Builds and uploads platform binary artifacts plus a source tarball to the GitHub Releases page.
- The GitHub Release body uses GitHub-generated release notes from the tag history.
- `CHANGELOG.md` remains the curated human source for release highlights and is scrubbed before publication; committed `dist/release-notes/` files are intentionally not maintained.
- Browse releases at <https://github.com/EmpireTwo/gaze/releases>.

### crates.io

Source: [`.github/workflows/publish-crates.yml`](../../../.github/workflows/publish-crates.yml).

- Triggered on `v*` tag pushes (with `workflow_dispatch` dry-run available).
- Authenticates to crates.io via OIDC trusted-publisher (`rust-lang/crates-io-auth-action`); no long-lived `CARGO_REGISTRY_TOKEN` secret.
- Publishes the trusted-publisher-linked workspace crates in topological order: `gaze-types` → `gaze-audit` → `gaze-recognizers` → `gaze-pii` → `gaze-assembly` → `gaze-mcp-core` → `gaze-mcp-rmcp` → `gaze-document` → `gaze-proxy` → `gaze-cli`. The core crate is published as `gaze-pii` while its library target remains `gaze`.
- Skips crates already at the published version (idempotent re-runs) and retries on index-propagation lag.
- New crates require a one-time manual `cargo publish` with a crates.io token, followed by trusted-publisher linking, before they join the OIDC publish loop.
- Browse crates at <https://crates.io/crates/gaze-pii> (and sibling crate pages).

Cutting a release: tag the merge commit on `main` with `vX.Y.Z` and push the tag. Both workflows fire from the same tag push; no manual crates.io step is needed for crates already in the OIDC publish loop.

## Homebrew Tap Location

Decision for v0.4.6 S6 (#184), reaffirmed post repo-public flip: keep Homebrew repo-local until the organization creates an explicit public tap and release publication target.

Current state:

- The formula source lives in this repository at `dist/homebrew/gaze.rb`.
- No public `EmpireTwo/tap` or `EmpireTwo/homebrew-tap` repository exists yet.
- Repo-public status alone does not enable `brew install` — adopters still need a tap that serves the formula. Until that tap exists, `cargo install gaze-cli` (from crates.io) is the supported install path for the CLI.
- `.github/workflows/release.yml` intentionally remains artifact-only for Homebrew: it builds and uploads GitHub release assets, but does not push formula updates to an external tap.
- The release workflow uploads generated GitHub release notes and binary/checksum artifacts; it does not read a committed release-notes file.
- Modern Homebrew rejects direct install/info commands for formula files outside a tap, so local smoke means staging the formula into a scratch tap rather than installing `./dist/homebrew/gaze.rb` directly.

Axis-5 rationale: documenting the repo-local formula is more ergonomic than advertising a tap that adopters cannot use. It gives collaborators a concrete smoke path while keeping public install instructions honest and reversible when a public tap is created.

Local smoke for maintainers:

```bash
brew tap-new EmpireTwo/gaze-smoke
cp dist/homebrew/gaze.rb "$(brew --repo EmpireTwo/gaze-smoke)/Formula/gaze.rb"
brew info EmpireTwo/gaze-smoke/gaze
brew untap EmpireTwo/gaze-smoke
```

Future public tap work is org-level operations outside this repo PR. When a public tap exists, update this document, the README install section, and `.github/workflows/release.yml` together so the formula location, adopter instructions, and release automation agree.
