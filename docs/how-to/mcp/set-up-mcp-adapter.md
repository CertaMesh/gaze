# Set up the MCP adapter

Expose Gaze's tools over stdio MCP. See the
[runtime contract](../../explanation/mcp/mcp-runtime.md).

## When to use the MCP adapter

Use `gaze_read_file` or `gaze_read_text` for sensitive reads from an MCP agent
host. `PiiEnvelope::dispatch` redacts inputs and outputs before model-facing
responses.

## Prerequisites

- A `gaze` binary built with the `mcp` feature.
- Add the `document` feature when `gaze_read_file` is needed.
- A supported MCP client installed: Claude Code, Claude Desktop, or Cursor.
- Optional: Tesseract and pdfium when `gaze_read_file` will read image or PDF
  documents.

Install from the repository with both MCP and document tools enabled:

```sh
cargo install --path crates/gaze-cli --features mcp,document
```

## Install into an MCP client

Choose a client (Claude Code and Cursor use project config):

```sh
gaze mcp install --client=claude-code
```

```sh
gaze mcp install --client=claude-desktop
```

```sh
gaze mcp install --client=cursor
```

All supported targets:

```sh
gaze mcp install --client=all
```

The installer writes `mcpServers.gaze` with the absolute `current_exe()` path
and these server arguments:

```json
{
  "mcpServers": {
    "gaze": {
      "command": "/absolute/path/to/gaze",
      "args": ["mcp", "serve"],
      "env": {}
    }
  }
}
```

The installer also updates a marker-fenced AGENTS.md section: route sensitive
reads through Gaze, treat tokens as placeholders, never invent originals, and
retain `manifest_id` for restore. This guidance is not a security boundary;
server-side `PiiEnvelope::dispatch` enforces it.

Use `--dry-run` to inspect the install summary without writing:

```sh
gaze mcp install --client=claude-code --dry-run
```

Use `--skip-agents-md` when you only want to update the client config:

```sh
gaze mcp install --client=claude-code --skip-agents-md
```

## Check the install

Run doctor after install:

```sh
gaze mcp doctor
```

The default output is a table with `pass`, `warn`, or `fail` state for runtime
dependencies, client configs, the MCP manifest directory, and AGENTS.md
guidance.

Emit JSON for automation:

```sh
gaze mcp doctor --json
```

Treat warnings as failures:

```sh
gaze mcp doctor --strict
```

Check a non-default AGENTS.md path:

```sh
gaze mcp doctor --agents-md ./AGENTS.md
```

## Run the server standalone

Run the stdio server directly:

```sh
gaze mcp serve
```

Write call manifests under a specific directory:

```sh
gaze mcp serve --manifest-dir ./.gaze/mcp-manifests
```

Cap file input size for `gaze_read_file`:

```sh
gaze mcp serve --max-file-size 26214400
```

## Tools the server exposes

`gaze_read_text` accepts already-extracted text and returns safe Markdown plus
manifest metadata. Use it when the caller already has the text payload.

Input shape:

```json
{"text":"Contact alice@example.invalid before the meeting."}
```

Output shape:

```json
{
  "clean_markdown": "Contact <:Email_1> before the meeting.",
  "manifest_id": "manifest-test-1",
  "file_metadata": null
}
```

`gaze_read_file` accepts a PNG, JPG, or PDF path, performs document ingestion,
and returns the same safe response shape:

```json
{"path":"./invoice.pdf"}
```

The response includes `{ clean_markdown, manifest_id, file_metadata }`. Preserve
`manifest_id` for authorized restore flows; do not ask the model to infer the
original values from tokens.

## Next steps

- [`docs/explanation/mcp/mcp-runtime.md`](../../explanation/mcp/mcp-runtime.md) — full
  MCP runtime contract.
- [`crates/gaze-mcp-core/README.md`](../../../crates/gaze-mcp-core/README.md) —
  transport-free chokepoint runtime.
- [`crates/gaze-mcp-rmcp/README.md`](../../../crates/gaze-mcp-rmcp/README.md) —
  rmcp stdio transport sink.
