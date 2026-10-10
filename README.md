<p align="center">
  <img src="docs/assets/gaze-logo.png" alt="Gaze" width="600">
</p>

[![Crates.io](https://img.shields.io/crates/v/gaze-pii.svg)](https://crates.io/crates/gaze-pii) [![License](https://img.shields.io/crates/l/gaze-pii.svg)](https://github.com/CertaMesh/gaze#license) [![docs.rs](https://docs.rs/gaze-pii/badge.svg)](https://docs.rs/gaze-pii) [![Tests](https://github.com/CertaMesh/gaze/actions/workflows/test.yml/badge.svg)](https://github.com/CertaMesh/gaze/actions/workflows/test.yml) [![GitHub stars](https://img.shields.io/github/stars/CertaMesh/gaze?style=social)](https://github.com/CertaMesh/gaze/stargazers)

Gaze replaces personal details with placeholders before an AI model sees them,
then restores them in the reply. The model sees `<Name_N>`; your server keeps
the originals in a restore manifest. The goal is zero PII bytes reaching the
model outside that contract.

Pre-1.0: the API is stabilizing. Older minor-version manifests restore on newer
minors ([UPGRADE.md](UPGRADE.md)).

![Gaze replaces personal details before the model and restores them after; the manifest stays on your server.](docs/assets/gaze-promise-loop.svg)

Gaze controls outbound PII. It does not defend against prompt injection or filter
unsafe content. Tool-call JSON carries placeholders too; Gaze restores them
before the tool runs ([integration](docs/explanation/how-gaze-works.md#how-it-fits-your-stack)).

## How good is it

The [v0.16.0 benchmark](docs/reference/benchmarks/README.md#current-release)
compares the `gaze setup` policy with Presidio, DataFog, scrubadub, GLiNER and
OPF on their declared configurations. “Leaked” counts PII bytes still reaching
the model. The target is zero.

<!-- BEGIN GENERATED: readme-chart -->

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/benchmarks/benchmark-panels-dark.svg">
  <img alt="Bar panels of character-level F2 and false-positive bytes per 1,000 bytes for Gaze releases and competitors on three benchmarks; the values are printed on the bars." src="docs/assets/benchmarks/benchmark-panels-light.svg">
</picture>

Gaze 0.16 leaks 13,291 PII bytes on our holdout (character-level F2 0.879; credentials counted since contract v4). Methods, competitor setups and how to reproduce: [benchmarks](docs/reference/benchmarks/README.md#benchmark-panels).

<!-- END GENERATED: readme-chart -->

## How does it work

![Steps 1 to 4, normalize, recognize, resolve and swap, are the deterministic floor. Step 5, the safety net on by default, and step 6, the output check, give a second opinion. The AI model sees placeholders only, and step 7 restores the reply.](docs/assets/gaze-seven-steps.svg)

1. Forty bundled rules find structured details using formats, checksums and cues.
2. NER finds names and places.
3. The resolver picks one candidate per span; Gaze emits a placeholder and manifest entry.
   The same input gives the same output; each placeholder traces to a versioned rule.
4. The `gaze setup` policy runs local Nym to find missed PII and tokenize it.
5. Restore replaces issued placeholders with originals. Unknown placeholders are refused.

Gaze fails closed on missing models and unknown rules. Unresolvable safety-net
suspects become one-way `[REDACTED:<class>]` markers or cause refusal. Only
`tolerant` development mode lets them through.

[How Gaze works](docs/explanation/how-gaze-works.md) covers the seven steps,
a support ticket and safety-net modes.

## Quickstart

Install the CLI, write the default policy, then clean and restore a synthetic contact:

```sh
cargo install gaze-cli --version 0.16.0
gaze setup
printf 'From: %s %s <%s@%s>' Ada Example ada example.invalid | gaze clean --policy gaze.toml > clean.json
jq -r .clean_text clean.json
jq '{session_blob, text: .clean_text}' clean.json | gaze restore | jq -r .text
```

The first line is model input; the second is restored output. For publication,
`sed` removes session prefixes and replaces `@` with ` [at] `.

```console
$ jq -r .clean_text clean.json | sed -E 's/<[0-9a-f]{8}:([A-Za-z]+)_[0-9]+>/<\1_N>/g'
From: <Name_N> <<Email_N>>
$ jq '{session_blob, text: .clean_text}' clean.json | gaze restore | jq -r .text | sed 's/@/ [at] /'
From: Ada Example <ada [at] example.invalid>
```

`gaze setup` verifies pinned NER and Nym bundles, writes `gaze.toml` with Nym on,
and checks both detectors. It prints Nym's MIT model licence and the open
[training-data licence review](docs/explanation/safety-net/safety-nets.md#licence-review-open).
Use `gaze setup --safety-net none` for NER only.

`gaze setup --dob-judge` installs a SHA-pinned local GLiNER bundle and enables
cue-less birth-date judgments. It stays opt-in while its bundle is 352 MB:
the benchmark reduces leaked DOB bytes from 810 to 725, adds no false-positive
bytes, and adds 664 MiB peak memory
([mechanism arms](docs/reference/benchmarks/README.md#per-mechanism-arms)).
`--dob-model-dir <path>` selects its directory. Setup checks a synthetic birth
date before writing the policy.

Send only `clean_text` to the model. Keep `clean.json` and its `session_blob`
on your server: the blob contains originals and is needed for restore.

## Where next

- [How Gaze works](docs/explanation/how-gaze-works.md): a support ticket from input to restore, the seven steps, the safety-net modes, and a glossary.
- [Feature comparison](docs/explanation/feature-comparison.md): sourced capabilities and limits alongside Presidio, DataFog, scrubadub, GLiNER, OPF, and LLM Guard.
- [Getting started](docs/tutorials/getting-started.md): the same round trip from Rust, in about ten minutes.
- [Use the `gaze setup` policy from Rust](docs/how-to/rust-library.md): the Nym-enabled pipeline as a library.
- [Audit and restore](docs/how-to/audit-and-restore.md): the metadata audit log and the restore contract.
- [AI support drafts in production](docs/explanation/support-drafts-in-production.md): how [`gaze-ghostwriter`](https://github.com/CertaMesh/gaze-ghostwriter) keeps the customer out of the model.
- [CLI reference](crates/gaze-cli/README.md) and [policy reference](docs/reference/policy.md).
- [All documentation](docs/README.md).

## Install

Install the CLI from crates.io:

```sh
cargo install gaze-cli --version 0.16.0
```

Or build from source (latest `main`, or to enable extra features):

```sh
git clone https://github.com/CertaMesh/gaze.git
cd gaze
cargo install --path crates/gaze-cli
```

Pre-built binaries for Apple Silicon macOS and Linux x86_64 (glibc 2.39+) are attached to each [GitHub release](https://github.com/CertaMesh/gaze/releases). Other targets: `cargo build --release -p gaze-cli`.

For the LLM API proxy:

```sh
cargo install --path crates/gaze-cli
gaze proxy start
export OPENAI_BASE_URL=http://127.0.0.1:8787/v1
export ANTHROPIC_BASE_URL=http://127.0.0.1:8787
```

For MCP hosts (Claude Code, Claude Desktop, Cursor):

```sh
cargo install --path crates/gaze-cli --features mcp
gaze mcp install --client=claude-code
gaze mcp doctor
```

The MCP server exposes `gaze_read_file` and `gaze_read_text`, returning tokenized content plus a `manifest_id` for authorized restore flows. Client config paths: [`crates/gaze-cli/README.md`](crates/gaze-cli/README.md#mcp-installation).

For library use, see [Use the `gaze setup` policy from Rust](docs/how-to/rust-library.md).

## Workspace and crates.io

Choose from the 15 published crates in the [crate map](docs/reference/crates.md).
For the Rust runtime (package `gaze-pii`, library name `gaze`):

```sh
cargo add gaze-pii
```

The core has no `rusqlite` dependency; SQLite auditing lives in `gaze-audit`.
[Document codec extensions](docs/explanation/document/document-extension.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for local gates and signed DCO commits (`git commit -S -s`, no CLA). Governance: [`docs/explanation/governance.md`](docs/explanation/governance.md).

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
