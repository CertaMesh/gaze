# Gaze MCP Adapter Quickstart

This page is an adopter setup guide for `gaze mcp`, the stdio MCP surface that
routes supported tool reads through the Gaze chokepoint. For the full runtime
contract, see [`docs/explanation/mcp/mcp-runtime.md`](../../explanation/mcp/mcp-runtime.md).

## When To Use

Use `gaze mcp` when your agent host already speaks MCP and you want Gaze to
provide a chokepoint tool surface for potentially sensitive file or text reads.
The client calls `gaze_read_file` or `gaze_read_text`; the server redacts tool
inputs and outputs through `PiiEnvelope::dispatch` before content reaches the
model-facing side of the MCP response.

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

## One-Command Install

Install into Claude Code project config:

```sh
gaze mcp install --client=claude-code
```

Install into Claude Desktop config:

```sh
gaze mcp install --client=claude-desktop
```

Install into Cursor project config:

```sh
gaze mcp install --client=cursor
```

Install into every supported client target:

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

It also creates or updates a marker-fenced AGENTS.md section:

```md
<!-- BEGIN GAZE MCP -->
# Gaze MCP

When Gaze MCP is available, route potentially sensitive file or text reads through `gaze_read_file` or `gaze_read_text` before using the content in an LLM response.

Gaze output contains pseudonymous tokens such as `:Email_`, `:Name_`, and `:Custom:phone_`. Treat these as placeholders, not missing data. Do not invent originals. Preserve the `manifest_id` returned by Gaze so authorized restore flows can round-trip values later.

This section is agent guidance, not a security boundary. The MCP chokepoint is the server-side `PiiEnvelope::dispatch` path.
<!-- END GAZE MCP -->
```

Use `--dry-run` to inspect the install summary without writing:

```sh
gaze mcp install --client=claude-code --dry-run
```

Use `--skip-agents-md` when you only want to update the client config:

```sh
gaze mcp install --client=claude-code --skip-agents-md
```

## Diagnostics

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

## Run Standalone

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

## Tools Exposed

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

## Five-Axis Pitch

- Reliability: tool calls pass through `PiiEnvelope::dispatch` before content
  reaches the model-facing response.
- Reversibility: the returned `manifest_id` points at owner-retained restore
  material.
- Agentic-first: supported agent hosts can install the stdio server with one
  command.
- Trust: tool registration is explicit, and manifest records are written for
  MCP calls.
- Adopter ergonomics: `install`, `doctor`, and `serve` cover setup,
  diagnostics, and standalone operation.

## Next Steps

- [`docs/explanation/mcp/mcp-runtime.md`](../../explanation/mcp/mcp-runtime.md) — full
  MCP runtime contract.
- [`crates/gaze-mcp-core/README.md`](../../../crates/gaze-mcp-core/README.md) —
  transport-free chokepoint runtime.
- [`crates/gaze-mcp-rmcp/README.md`](../../../crates/gaze-mcp-rmcp/README.md) —
  rmcp stdio transport sink.
