# Release Process

`CertaMesh/gaze` is a public repository. Two release channels are live for adopters; Homebrew remains repo-local pending a public tap.

## Public release channels (live)

### GitHub Releases

Source: [`.github/workflows/release.yml`](../../../.github/workflows/release.yml).

- Triggered on `v*` tag pushes, but creates no GitHub Release until the
  signed post-tag benchmark-readiness receipt passes. Retrying the
  held tag workflow publishes the original tag commit; manual dispatch cannot
  create a release.
- Builds and uploads platform binary artifacts plus a source tarball to the GitHub Releases page.
- The GitHub Release body uses GitHub-generated release notes from the tag history.
- `CHANGELOG.md` remains the curated human source for release highlights and is scrubbed before publication; committed `dist/release-notes/` files are intentionally not maintained.
- `cargo run -p xtask -- scrub-public-text --published` uses one file set in PR tests and release preflights: the complete `CHANGELOG.md`, `UPGRADE.md`, root `README.md`, each crate's explicit or conventional README, and existing Markdown under `docs/` cited by those files. It follows one link hop. Tag jobs also scrub the version's changelog section; the `workflow_dispatch` release preflight adds supplied notes or a PR body. The workspace test `scrub_public_text_passes_published_and_linked_docs` runs the shared set on every PR. The gate masks only fixed public URL shapes in `crates/xtask/src/scrub_public_text.rs`; exact hosts and bounded paths keep lookalikes and free text visible to detection.
- Browse releases at <https://github.com/CertaMesh/gaze/releases>.

### crates.io

Source: [`.github/workflows/publish-crates.yml`](../../../.github/workflows/publish-crates.yml).

- Triggered on `v*` tag pushes (with `workflow_dispatch` dry-run available).
  Real publication is held until the same signed post-tag
  benchmark-readiness receipt passes; manual dispatch rejects non-dry runs.
- Authenticates to crates.io via OIDC trusted-publisher (`rust-lang/crates-io-auth-action`); no long-lived `CARGO_REGISTRY_TOKEN` secret.
- Derives the publish set and topological order from `cargo metadata` with `cargo run -p xtask -- publish-plan`. Every workspace member with `publish != false` is included automatically, including new crates. The core crate is published as `gaze-pii` while its library target remains `gaze`.
- Runs a manifest pre-flight before any real publish: `cargo package --no-verify --workspace --exclude xtask` for the workspace. Workspace packaging resolves coordinated, not-yet-published dependency versions together. Per-crate packaging would resolve those versions against crates.io before they exist. This catches unpublishable manifests before OIDC auth or partial publishing.
- Checks crates.io for every planned crate before publishing. If any crate is absent, the workflow fails up front because OIDC trusted publishing cannot first-publish a new crate.
- Skips crates already at the published version (idempotent re-runs) and retries on index-propagation lag.
- A new crate's first publication cannot use the existing OIDC guard. This
  is a separately authorized onboarding exception, not permission to bypass a
  release hold. Keep normal version publication in the prescribed workflows.
  Before any exceptional first publication, require reviewed crate/version/ref
  scope, all applicable release gates and readiness acceptance, and an approved
  onboarding path with the same hold. Until that path and publisher linking are
  established, HOLD; this document provides no manual `cargo publish` shortcut.
  `workflow_dispatch` with `check_new_crates` remains exceptional **dry-run**
  diagnostics only; tag releases keep the guard on.
- Browse crates at <https://crates.io/crates/gaze-pii> (and sibling crate pages).

The post-tag two-commit sequence is a proposal whose substantive timing
approval is unresolved. This source change does not select it or waive the
existing pretag public benchmark/document dogfood requirements. Hold before a
public release tag until timing applicability, original release gates and the
external rollout prerequisites below are accepted. The proposed mechanics are:
merge and freeze measured `R`; measure and accept private evidence `E`; create
signed annotated `vX.Y.Z` with `T = R`; render/dogfood/merge public documentation
`D`; then create signed annotated `release-readiness/vX.Y.Z` targeting `D`.
Only retry the original guarded workflows after independent readiness acceptance.

## Readiness acceptance boundary

The source gate is a bounded publication verifier, not a benchmark runner or
native evidence framework. Its versioned aggregate evidence is committed at `D`;
raw private evidence, PII and native custody artifacts stay private. Export only
reviewed aggregate identities and acceptance receipts. The signed readiness tag
is the **maintainer attestation trust boundary**: the signer accepts actual gate
execution, freshness, numerical gains, native/custody assertions, preview/public
byte dogfood, timing authority and external settings. The source checks identity,
completeness, digest joins and PASS status; it cannot prove those assertions or
replace independent review. An attestation cannot waive an unresolved requirement.

`docs/reference/benchmarks/release-readiness/vX.Y.Z.json` has `schema_version: 1`,
`version` without `v`, `release_commit`, `release_tag_object`, `harness_commit`,
`renderer_commit`, `evidence` (`manifest_path`, `sha256`), `scorecards` (path →
SHA-256), `history` and `policy` (each `path`, `sha256`), and `successful_checks`
including `docs`. Its exact bytes and the evidence digest are signed by the
`gaze-release-readiness-v1` receipt, which additionally names `documentation_commit`.
Paths must be under `docs/reference/benchmarks/`, with no traversal components.

The evidence manifest uses exactly these top-level fields:

| Field | Required value or binding |
|---|---|
| `schema_version`, `format` | `1`, `gaze-release-evidence-v1` |
| `version`, `release_commit`, `harness_commit`, `renderer_commit` | Equal the documentation manifest and signed receipt |
| `measured_commit`, `status` | Actual `R`, `PASS` |
| `history` | Equal the documentation history path/digest binding |
| `scorecards` | Complete version-owned inventory below |
| `acceptance` | `authority: signed-maintainer`, complete `gates` map below |

The current inventory is exactly `scorecard-vX.Y.Z.json` (contract v1),
`scorecard-vX.Y.Z-scored-labels-v2.json` and
`scorecard-vX.Y.Z-scored-labels-v3.json`. Each evidence entry binds `sha256` plus
`identities`: the scorecard's dataset identity (`repository`, `revision`, `file`,
`integrity`, `evaluated_population`, `sampling` when present), `parameters`,
`policy_sha256`, `model_bundles` and `scored_label_contract` (null for implicit v1).
The gate reuses the existing schema-v4 renderer projection, requires nonempty
runs and explicit clean `gaze.revision = R`, and verifies the current history
row and each contract result against those projections and byte digests.
The three cards share frozen input/parameter/model identities; policy bytes
match both recorded policy digests, model expected/observed digests agree, and
v2/v3 scored-contract files are fetched at frozen harness commit `H` and hashed.
If the runner records `harness_revision`, it must equal `H`. H/P execution and
model/corpus custody remain attested, rather than inferred from repeated SHAs.
Historical rows keep their own measured commits; they are not forced to equal R.
Changing this inventory requires a reviewed contract revision, not an omitted card.

Every acceptance gate has `status: PASS` and `receipt: {path, sha256}`. Its exact
JSON receipt must contain `schema_version: 1`, `gate` equal to the map key,
`status: PASS`, and `release_commit: R`; retain reviewed aggregate proof in that
receipt as appropriate. Required keys are `release_preflight`,
`benchmark_gain_v1`, `benchmark_gain_v2`, `historical_comparisons`, `competitors`,
`native`, `restore`, `manifest`, `private_preview`, `public_documentation`,
`timing_authority`, `tag_namespace`, and `publisher_refs`. Missing/unknown gate
keys, failed statuses, HOLD and NOT_RUN are rejected. These categories name the
existing obligations; this contract sets no new native or gain threshold and
provides no acceptance waiver. Unsupported evidence formats remain held.

The signed `D` must descend from `R` and belong to the currently protected default
branch, proved with GitHub compare responses. Each required check must have D's
head, completed success and the GitHub Actions app identity. One selected trusted
`docs` check supplies the exact repository/run/job URL; the fetched run joins its
run ID, check suite, workflow ID and canonical `.github/workflows/docs.yml` path,
D's head, repository/head repository, default branch and successful push event.
A successful unmerged PR run is insufficient. See the official
[workflow-run](https://docs.github.com/en/rest/actions/workflow-runs#get-a-workflow-run),
[check-run](https://docs.github.com/en/rest/checks/runs#list-check-runs-for-a-git-reference)
and [compare](https://docs.github.com/en/rest/commits/commits#compare-two-commits)
API contracts. GitHub signature verification is trusted for the signed payload;
Python tests exercise sanitized API envelopes, not cryptographic forgery proof.

### External rollout prerequisites — hold before public tag

Tag objects are immutable; tag **refs** require protections. No settings changes
are authorized by this document or the source gate. Before adopting the hold,
review authoritative proof of authorized creation plus no update/deletion for
both `v*` release tags and `release-readiness/v*` refs, including bypass actors.
Review actual crates.io trusted-publisher repository/workflow/environment/ref
restrictions and the GitHub publication environment's protections. They must
exclude older or untrusted workflow refs that could omit the gate, including
historic `workflow_dispatch` definitions; latest YAML alone does not revoke them.
The reviewed rollout receipt must identify the effective settings and allowed
refs. Source-level protected-branch checks do not prove namespace or publisher
rollout. Missing, unknown or unapproved proof leaves both the public tag and
publication held, even if all Python checks pass.

## Pre-tag model-setup ownership gate

Before the first `gaze-model-setup` publication, run `release.yml` with
`workflow_dispatch` on the reviewed preparation branch. This path scrubs the
release text and runs `scripts/gate/model-setup-ownership.sh` on hosted Linux;
it does not build release assets, create a release, or publish crates.

The ownership gate uses the shipped installer to fetch and strictly verify
the source-pinned real Davlan mBERT NER bundle. It checks identical artifact hashes
before testing a copy owned by a distinct user, uses a foreign-owned working
directory, and explicitly runs the ignored cross-directory effective-user test.
It also verifies loose-mode repair with an independent bundle check and
exact effective-user ownership, 0700 directory modes, and 0600 file modes.
A separately hashed, readable foreign-owned copy proves setup rejects the
owner mismatch specifically; the verifier keeps its separate private copy.
Post-repair hashes and owner/mode inventory are retained in the receipts. Exact test names must report a passing test;
a zero-test cargo result cannot pass the gate.

After review, dispatch with `gh workflow run release.yml --ref <preparation-branch>
-f version=<X.Y.Z> -f pr_number=<release-pr>`. Require successful
`scrub-public-text-preflight` and `model-setup-ownership-preflight` jobs for
that exact preparation head before tagging. The ownership job's
`model-setup-ownership-<commit>` artifact
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

Future public tap work is an org-level operation outside this repository. When a public tap exists, update this document, the README install section, and `.github/workflows/release.yml` together so the formula location, adopter instructions, and release automation agree.
