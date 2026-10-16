# Route an SDK call through the proxy

We will send an OpenAI SDK request through Gaze to a local echo server.
The server receives tokens; the caller receives its restored reply. Allow fifteen minutes.

Start in a Gaze source checkout. You need `gaze` with its default proxy feature,
Python 3.10 or newer as `python3`, and a POSIX shell. Port 8787 must be free.
The only upstream is a local mock. Use the dummy key below.

## 1. Prepare the application

First, copy the mock server and install the SDK in a virtual environment:

```sh
umask 077
mkdir -p target/proxy-tutorial
cp docs/tutorials/examples/mock_openai.py target/proxy-tutorial/
cd target/proxy-tutorial
python3 -m venv .venv
.venv/bin/python -m pip install --quiet openai
cat > policy.toml <<'TOML'
[session]
scope = "ephemeral"

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "default"
action = "tokenize"
TOML
printf 'SDK and policy ready\n'
```

You should see:

```text
SDK and policy ready
```

## 2. Start the mock and proxy

Keep this shell open. It owns both processes and stops them at exit:

```sh
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
wait "$proxy_pid" 2>/dev/null || true
wait "$mock_pid" 2>/dev/null || true
trap - EXIT
printf 'Proxy and mock stopped\n'
```

You should see:

```text
Proxy and mock stopped
```

For real provider keys, Anthropic and Gemini configuration, streaming, and
deployment limits, use [Set up the proxy](../how-to/proxy/set-up-proxy.md).
