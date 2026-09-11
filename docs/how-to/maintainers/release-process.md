# Release Process

`CertaMesh/gaze` is a public repository. Two release channels are live for adopters; Homebrew remains repo-local pending a public tap.

## Public release channels (live)

### GitHub Releases

Source: [`.github/workflows/release.yml`](../../../.github/workflows/release.yml).

- Triggered on `v*` tag pushes.
- Builds and uploads platform binary artifacts plus a source tarball to the GitHub Releases page.
- The GitHub Release body uses GitHub-generated release notes from the tag history.
- `CHANGELOG.md` remains the curated human source for release highlights and is scrubbed before publication; committed `dist/release-notes/` files are intentionally not maintained.
- Browse releases at <https://github.com/CertaMesh/gaze/releases>.

### crates.io

Source: [`.github/workflows/publish-crates.yml`](../../../.github/workflows/publish-crates.yml).

- Triggered on `v*` tag pushes (with `workflow_dispatch` dry-run available).
- Authenticates to crates.io via OIDC trusted-publisher (`rust-lang/crates-io-auth-action`); no long-lived `CARGO_REGISTRY_TOKEN` secret.
- Derives the publish set and topological order from `cargo metadata` with `cargo run -p xtask -- publish-plan`. Every workspace member with `publish != false` is included automatically, including new crates. The core crate is published as `gaze-pii` while its library target remains `gaze`.
- Runs a manifest pre-flight before any real publish: `cargo package --no-verify --workspace --exclude xtask` for the workspace. Workspace packaging resolves coordinated, not-yet-published dependency versions together. Per-crate packaging would resolve those versions against crates.io before they exist. This catches unpublishable manifests before OIDC auth or partial publishing.
- Checks crates.io for every planned crate before publishing. If any crate is absent, the workflow fails up front because OIDC trusted publishing cannot first-publish a new crate.
- Skips crates already at the published version (idempotent re-runs) and retries on index-propagation lag.
- New crates require a one-time manual seed publish with a crates.io token, followed by trusted-publisher linking, before a tag publish can proceed:

```bash
cargo publish -p <crate>
```

After the seed publish, add the crate's Trusted Publisher on crates.io for `CertaMesh/gaze` and `.github/workflows/publish-crates.yml`, then re-run the publish workflow. `workflow_dispatch` has a `check_new_crates` input for exceptional dry-run diagnostics, but tag releases keep the guard on.
- Browse crates at <https://crates.io/crates/gaze-pii> (and sibling crate pages).

Cutting a release: tag the merge commit on `main` with `vX.Y.Z` and push the tag. Both workflows fire from the same tag push; no manual crates.io step is needed for crates already in the OIDC publish loop.

## Pre-tag model-setup ownership gate

Before the first `gaze-model-setup` publication, run `release.yml` with
`workflow_dispatch` on the reviewed preparation branch. This path scrubs the
release text and runs `scripts/gate/model-setup-ownership.sh` on hosted Linux;
it does not build release assets, create a release, or publish crates.

The ownership gate uses the shipped installer to fetch and strictly verify
the source-pinned real Kiji FP32 bundle. It checks identical artifact hashes
before testing a copy owned by a distinct user, uses a foreign-owned working
directory, and explicitly runs the ignored cross-directory effective-user test.
It also verifies loose-mode repair with an independent bundle check and
exact effective-user ownership, 0700 directory modes, and 0600 file modes.
A separately hashed, readable foreign-owned copy proves setup rejects the
owner mismatch specifically; the verifier keeps its separate private copy.
Post-repair hashes and owner/mode inventory are retained in the receipts. Exact test names must report a passing test;
a zero-test cargo result cannot pass the gate.

After review, dispatch with `gh workflow run release.yml --ref <preparation-branch>
-f version=0.14.0 -f pr_number=<release-pr>`. Require a successful
`model-setup-ownership-preflight` job for that exact preparation head before
tagging. Its `model-setup-ownership-<commit>` artifact
records the commit, commands, toolchain, model hashes, owner/mode inventory,
and test results. It contains receipts only; model files are temporary and
are removed when the gate exits. The script requires an unprivileged Linux
user with passwordless sudo so real foreign ownership can be constructed.
The publish plan orders `gaze-recognizers` before `gaze-model-setup` at the
coordinated release version.

## Homebrew Tap Location

Decision for v0.4.6 S6 (#184), reaffirmed post repo-public flip: keep Homebrew repo-local until the organization creates an explicit public tap and release publication target.

Current state:

- The formula source lives in this repository at `dist/homebrew/gaze.rb`.
- No public `CertaMesh/tap` or `CertaMesh/homebrew-tap` repository exists yet.
- Repo-public status alone does not enable `brew install` — adopters still need a tap that serves the formula. Until that tap exists, `cargo install gaze-cli` (from crates.io) is the supported install path for the CLI.
- `.github/workflows/release.yml` intentionally remains artifact-only for Homebrew: it builds and uploads GitHub release assets, but does not push formula updates to an external tap.
- The release workflow uploads generated GitHub release notes and binary/checksum artifacts; it does not read a committed release-notes file.
- Modern Homebrew rejects direct install/info commands for formula files outside a tap, so local smoke means staging the formula into a scratch tap rather than installing `./dist/homebrew/gaze.rb` directly.

Axis-5 rationale: documenting the repo-local formula is more ergonomic than advertising a tap that adopters cannot use. It gives collaborators a concrete smoke path while keeping public install instructions honest and reversible when a public tap is created.

Local smoke for maintainers:

```bash
brew tap-new CertaMesh/gaze-smoke
cp dist/homebrew/gaze.rb "$(brew --repo CertaMesh/gaze-smoke)/Formula/gaze.rb"
brew info CertaMesh/gaze-smoke/gaze
brew untap CertaMesh/gaze-smoke
```

Future public tap work is org-level operations outside this repo PR. When a public tap exists, update this document, the README install section, and `.github/workflows/release.yml` together so the formula location, adopter instructions, and release automation agree.
