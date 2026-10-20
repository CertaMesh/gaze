# Route an SDK call through the proxy

<!-- tutorial-gate: start=checkout -->

We will send an OpenAI SDK request through Gaze to a local echo server.
The server receives tokens; the caller receives its restored reply. Allow fifteen minutes.

Start in a Gaze source checkout. You need `gaze` with its default proxy feature,
Python 3.10 or newer as `python3`, and a POSIX shell. Port 8787 must be free.
The only upstream is a local mock. Use the dummy key below.

## 1. Prepare the application

First, install the SDK and write the local mock server:

```sh
umask 077
mkdir -p target/proxy-tutorial
cd target/proxy-tutorial
python3 -m venv .venv
.venv/bin/python -m pip install --quiet openai==2.31.0
cat > policy.toml <<'TOML'
[session]
scope = "ephemeral"

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "default"
action = "tokenize"
TOML
cat > mock_openai.py <<'PY'
"""Local, non-streaming Chat Completions echo server for the proxy tutorial."""

from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path


class Echo(BaseHTTPRequestHandler):
	def do_POST(self):
		if self.path != "/v1/chat/completions":
			self.send_error(404)
			return
		request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
		Path("upstream-request.json").write_text(json.dumps(request))
		response = {
			"id": "mock-completion",
			"object": "chat.completion",
			"created": 0,
			"model": "mock-model",
			"choices": [{
				"index": 0,
				"message": {"role": "assistant", "content": request["messages"][-1]["content"]},
				"finish_reason": "stop",
			}],
		}
		body = json.dumps(response).encode()
		self.send_response(200)
		self.send_header("Content-Type", "application/json")
		self.send_header("Content-Length", str(len(body)))
		self.end_headers()
		self.wfile.write(body)

	def log_message(self, *_args):
		pass


if __name__ == "__main__":
	with HTTPServer(("127.0.0.1", 0), Echo) as server:
		host, port = server.server_address
		Path("mock-upstream-url.txt").write_text(f"http://{host}:{port}")
		server.serve_forever()
PY
printf 'SDK and policy ready\n'
```

You should see:

```text
SDK and policy ready
```

## 2. Start the mock and proxy

Keep this shell open. It owns both processes and stops them at exit:

```sh
rm -f mock-upstream-url.txt upstream-request.json
python3 mock_openai.py > mock.log 2>&1 &
mock_pid=$!
trap 'kill "$mock_pid" 2>/dev/null || true' EXIT
python3 - <<'PY'
from pathlib import Path
import time

for _ in range(100):
	if Path("mock-upstream-url.txt").is_file():
		break
	time.sleep(0.1)
else:
	raise RuntimeError("mock server did not start; inspect mock.log")
print("Mock upstream ready")
PY
gaze proxy serve --policy policy.toml \
	--upstream-openai "$(cat mock-upstream-url.txt)" > proxy.log 2>&1 &
proxy_pid=$!
trap 'kill "$proxy_pid" "$mock_pid" 2>/dev/null || true' EXIT
python3 - <<'PY'
import time
from urllib.request import urlopen

base_url = "http://127.0.0.1:8787"
for _ in range(100):
	try:
		with urlopen(base_url + "/_gaze_proxy/healthz", timeout=1) as response:
			assert response.status == 200
		break
	except OSError:
		time.sleep(0.1)
else:
	raise RuntimeError("proxy did not start; inspect proxy.log")
print("Proxy ready")
PY
```

The readiness checks report:

```text
Mock upstream ready
Proxy ready
```

## 3. Make the SDK call

Now, point the SDK at Gaze's `/v1` base URL:

```sh
.venv/bin/python - <<'PY'
import json
from pathlib import Path
from openai import OpenAI

original = "Contact {}@{} for details.".format("alice", "example.invalid")
with OpenAI(api_key="tutorial-dummy", base_url="http://127.0.0.1:8787/v1") as client:
	reply = client.chat.completions.create(
		model="mock-model", messages=[{"role": "user", "content": original}],
	)
upstream = json.loads(Path("upstream-request.json").read_text())
protected = upstream["messages"][0]["content"]
assert ":Email_" in protected
assert "example.invalid" not in json.dumps(upstream)
assert reply.choices[0].message.content == original
print(f"Upstream received: {protected}")
print("Owner received the exact original: true")
PY
```

The mock sees the token and echoes it. Gaze restores that token before the SDK
returns the response:

```text
Upstream received: Contact <{session_hex}:Email_1> for details.
Owner received the exact original: true
```

## 4. Stop both processes

Stop the sample's processes and wait for them:

```sh
kill "$proxy_pid" "$mock_pid"
for pid in "$proxy_pid" "$mock_pid"; do
	if wait "$pid" 2>/dev/null; then
		:
	else
		exit_status=$?
		[ "$exit_status" -eq 143 ] || exit "$exit_status"
	fi
done
trap - EXIT
printf 'Proxy and mock stopped\n'
```

You should see:

```text
Proxy and mock stopped
```

The checked waits accept a normal exit or status 143 from the requested
SIGTERM. Any other exit status fails the exercise.

For real provider keys, Anthropic and Gemini configuration, streaming, and
deployment limits, use [Set up the proxy](../how-to/proxy/set-up-proxy.md).
