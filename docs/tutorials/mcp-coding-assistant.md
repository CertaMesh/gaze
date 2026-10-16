# Protect a coding assistant's document read

We will configure Claude Code to read a fictional invoice through Gaze's MCP
server and verify that the tool returns tokens. Allow ten minutes.

Start in a Gaze source checkout. You need `gaze` with the `mcp` and `document`
features, Tesseract, Python 3, and a signed-in Claude Code CLI. See
[MCP prerequisites](../how-to/mcp/set-up-mcp-adapter.md#prerequisites).
The assistant step makes one model call with your Claude Code account.

This protects reads through Gaze's tool. Native reads and text pasted into a
chat bypass this boundary. Use only the fictional fixture in this exercise.

## 1. Install into a sample project

First, copy the sample image into a private directory and install Gaze there:

```sh
umask 077
mkdir -p target/mcp-tutorial
cp crates/gaze-document/testdata/synthetic_image.png target/mcp-tutorial/invoice.png
cd target/mcp-tutorial
gaze mcp install --client claude-code
```

The installer reports `claude-code installed` for `.mcp.json` and
`agents-md installed` for `AGENTS.md`. Paths depend on the checkout.

## 2. Check what the client launches

Now, inspect the entry without printing local paths:

```sh
python3 - <<'PY'
import json
from pathlib import Path

entry = json.loads(Path(".mcp.json").read_text())["mcpServers"]["gaze"]
assert Path(entry["command"]).is_absolute()
assert entry["args"][:2] == ["mcp", "serve"]
print("Client launches gaze mcp serve")
PY
```

You should see:

```text
Client launches gaze mcp serve
```

## 3. Read through the Gaze tool

We will enable only this MCP server and disable built-in tools for the call.
The prompt names the file without including its contents:

```sh
claude --strict-mcp-config --mcp-config .mcp.json --tools "" \
	--allowedTools mcp__gaze__gaze_read_file --print \
	--output-format stream-json --verbose \
	'Use gaze_read_file to read invoice.png. Return only its clean_markdown.' \
	> assistant.jsonl
python3 - <<'PY'
import json
from pathlib import Path

events = [json.loads(line) for line in Path("assistant.jsonl").read_text().splitlines()]
calls = []
results = []
for event in events:
	for block in event.get("message", {}).get("content", []):
		if block.get("type") == "tool_use" and block.get("name") == "mcp__gaze__gaze_read_file":
			calls.append(block["id"])
		if block.get("type") == "tool_result" and block.get("tool_use_id") in calls:
			assert not block.get("is_error", False)
			results.append(block["content"])
assert calls and results
protected = json.dumps(results)
assert all(label in protected for label in [":Name_", ":Email_", ":Custom:phone_"])
assert "example.invalid" not in protected
assert "jane.doe" not in protected
assert "Jane Doe" not in protected
print("Called gaze_read_file: true")
print("Tool returned name, email, and phone tokens: true")
PY
```

Claude Code starts `gaze mcp serve`, calls the tool, and exits. The checks report:

```text
Called gaze_read_file: true
Tool returned name, email, and phone tokens: true
```

Keep the source image and local transcripts on the owner side. The MCP server
keeps its manifests in owner-side storage; never upload that directory to the model.

For other clients and doctor checks, use [Set up the MCP adapter](../how-to/mcp/set-up-mcp-adapter.md).
For the enforced boundary, see [MCP runtime](../explanation/mcp/mcp-runtime.md#scope).
