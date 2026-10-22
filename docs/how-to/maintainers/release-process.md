# Release Process

Tag pushes publish GitHub Releases and crates.io packages. Homebrew is repo-local.

## Public release channels (live)

### GitHub Releases

Source: [`.github/workflows/release.yml`](../../../.github/workflows/release.yml).

- Triggered on `v*` tag pushes.
- Builds and uploads `gaze-aarch64-apple-darwin`, `gaze-x86_64-linux-gnu`,
  and `gaze-aarch64-linux-gnu`, each with a `.sha256` file. GitHub also provides
  source archives. Linux binaries target Ubuntu 24.04 and require glibc 2.39+.
- Linux arm64 builds run natively on `ubuntu-24.04-arm`. Release builds and the
  arm64 PR dry-run share [release-build.yml](../../../.github/workflows/release-build.yml),
  including features, packaging, checksum verification, and artifact smoke tests.
  The arm64 smoke also installs the pinned NER bundle, runs `gaze clean` with
  NER and no bundled rules, and checks exact restore.
- Before merging changes to either release workflow, require `arm64-dry-run`
  to pass on the PR head. This job runs only on PRs that change those workflows.
  It uploads Actions artifacts without publishing a release. Publication runs
  only on a `v*` tag push and is exercised at the next release.
- After publication, confirm all three binaries and their checksums are present. Select
  the matching Linux architecture when replaying installation in a container.
- GitHub generates release notes from tag history. Keep curated highlights in
  `CHANGELOG.md`; do not maintain `dist/release-notes/` files.
- `cargo run -p xtask -- scrub-public-text --published` checks all of `CHANGELOG.md`,
  `UPGRADE.md`, root/crate READMEs, and one hop of linked Markdown under `docs/`.
  PR tests and release preflights share this set. Tag jobs also scrub that
  version's changelog section; dispatch preflights add supplied notes/PR text.
  `scrub_public_text_passes_published_and_linked_docs` runs on every PR.
  Only fixed public URL hosts/paths are masked by
  `crates/xtask/src/scrub_public_text.rs`; lookalikes remain detectable.
- Browse releases at <https://github.com/CertaMesh/gaze/releases>.

### crates.io

Source: [`.github/workflows/publish-crates.yml`](../../../.github/workflows/publish-crates.yml).

- Triggered on `v*` tag pushes (with `workflow_dispatch` dry-run available).
- Authenticates to crates.io via OIDC trusted-publisher (`rust-lang/crates-io-auth-action`); no long-lived `CARGO_REGISTRY_TOKEN` secret.
- Derives the publish set and topological order from `cargo metadata` with `cargo run -p xtask -- publish-plan`. Every workspace member with `publish != false` is included automatically, including new crates. The core crate is published as `gaze-pii` while its library target remains `gaze`.
- Before OIDC authentication or publishing, run
  `cargo package --no-verify --workspace --exclude xtask`. Workspace packaging
  resolves coordinated, unpublished versions together; per-crate packaging cannot.
- Checks crates.io for every planned crate before publishing. If any crate is absent, the workflow fails up front because OIDC trusted publishing cannot first-publish a new crate.
- Skips crates already at the published version (idempotent re-runs) and retries on index-propagation lag.
- New crates require a one-time manual seed publish with a crates.io token, followed by trusted-publisher linking, before a tag publish can proceed:

```bash
cargo publish -p <crate>
```

After the seed publish, add the crate's Trusted Publisher on crates.io for `CertaMesh/gaze` and `.github/workflows/publish-crates.yml`, then re-run the publish workflow. `workflow_dispatch` has a `check_new_crates` input for exceptional dry-run diagnostics, but tag releases keep the guard on.
- Browse crates at <https://crates.io/crates/gaze-pii> (and sibling crate pages).

After gates pass, create a signed `vX.Y.Z` tag on the merge commit on `main`
and push it. Both workflows fire; seeded, linked crates need no manual publish.

## Replay account-bound tutorial steps

Before tagging, require `tutorial execution` to pass on the exact release head.
Download its `tutorial-execution` artifact and check that every tutorial has
executed blocks. Replay every printed `SKIP` with the required account and the
release-head binary, then record the commit, tutorial, command, and result in
the release PR. Keep account transcripts and restore material private.

The current manual replay is step 3 of
[Protect a coding assistant's document read](../../tutorials/mcp-coding-assistant.md).
Run steps 1 and 2 first in a fresh checkout directory, then run the marked
Claude Code call and its Python assertions. Require both `Called gaze_read_file:
true` and `Tool returned name, email, and phone tokens: true`. A skipped or
failed manual replay blocks the release.

The [tutorial gate contract](../../reference/tutorial-gate.md) defines markers,
exact output matching, pinned dependencies, and the mutation probes.

## Replay the published Rust tutorial

After the release's crates are available on crates.io, replay
[Protect a model call from Rust](../../tutorials/getting-started.md) in a fresh
directory outside a source checkout. Use normal Cargo commands, without the
tutorial gate's path-dependency substitution. In step 1, pin both dependencies
to the released version with
`cargo add gaze-pii@<version> gaze-assembly@<version>`.

Require tokenized model input and the program's exact-restore assertions to
pass. Record the released version and result in the release PR before marking
the release complete. The pre-tag CI gate builds source paths and does not
test installation from crates.io.

## Pre-tag model-setup ownership gate

Before the first `gaze-model-setup` publication, dispatch `release.yml` on the
reviewed preparation branch:

```sh
gh workflow run release.yml --ref <preparation-branch> -f version=<X.Y.Z> -f pr_number=<release-pr>
```

Require `scrub-public-text-preflight` and `model-setup-ownership-preflight` to
pass on that exact head before tagging. Dispatch creates no release assets,
release, or crate publications.

`scripts/gate/model-setup-ownership.sh` uses hosted Linux with an unprivileged
user and passwordless sudo. It fetches and verifies the source-pinned Davlan
mBERT bundle through the shipped installer, then checks:

- Identical hashes before testing a distinct user's copy and foreign-owned cwd.
- The ignored cross-directory effective-user test actually runs and passes.
- A hashed readable foreign-owned copy is rejected specifically for ownership;
  the verifier uses a separate private copy.
- Loose-mode repair passes independent verification: effective-user ownership,
  `0700` directories, `0600` files, and unchanged hashes.

Zero-test results fail. The `model-setup-ownership-<commit>` artifact retains
commit, commands, toolchain, hashes, owner/mode inventory, and test receipts.
Temporary models are deleted on exit. Publish order places `gaze-recognizers`
before `gaze-model-setup` at the coordinated version.

## Homebrew Tap Location

The formula is `dist/homebrew/gaze.rb`. No public `CertaMesh/tap` or
`CertaMesh/homebrew-tap` exists; use `cargo install gaze-cli` for supported CLI
installation. The release workflow uploads binaries, checksums, and generated
notes but does not update an external tap or read committed release notes.

Homebrew requires a tap, even for a local formula smoke check:

```bash
brew tap-new CertaMesh/gaze-smoke
cp dist/homebrew/gaze.rb "$(brew --repo CertaMesh/gaze-smoke)/Formula/gaze.rb"
brew info CertaMesh/gaze-smoke/gaze
brew untap CertaMesh/gaze-smoke
```

When the organization creates a public tap, update this guide, README install
instructions, and `.github/workflows/release.yml` together.
