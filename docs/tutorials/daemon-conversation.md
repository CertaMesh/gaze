# Reuse tokens across daemon turns

We will send three messages through one daemon process. Two messages share a
conversation and reuse a token; the third gets an isolated token. Allow ten minutes.

You need `gaze`, Python 3, and a POSIX shell. This exercise uses deterministic
rules and needs no model download.

## 1. Write a policy

First, create a working directory and a policy that tokenizes detected values:

```sh
umask 077
mkdir gaze-daemon-demo
cd gaze-daemon-demo
cat > policy.toml <<'TOML'
[session]
scope = "conversation"

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "default"
action = "tokenize"
TOML
printf 'Daemon policy ready\n'
```

You should see:

```text
Daemon policy ready
```

## 2. Send three turns

Now, start the daemon and send newline-delimited JSON requests:

```sh
python3 - <<'PY'
import json
import re
import subprocess

email = "{}@{}".format("alice", "example.invalid")
requests = [
	{"session_id": "conversation-a", "text": f"Email {email}."},
	{"session_id": "conversation-a", "text": f"Reply to {email}."},
	{"session_id": "conversation-b", "text": f"Email {email}."},
]
process = subprocess.run(
	["gaze", "daemon", "--policy", "policy.toml"],
	input="".join(json.dumps(request) + "\n" for request in requests),
	text=True, capture_output=True, check=True, timeout=30,
)
responses = [json.loads(line) for line in process.stdout.splitlines()]
assert len(responses) == 3
tokens = []
for number, (request, response) in enumerate(zip(requests, responses), 1):
	assert "error" not in response, response.get("error")
	assert response["session_id"] == request["session_id"]
	assert email not in response["clean_text"]
	token = re.search(r"<[0-9a-f]{8}:Email_\d+>", response["clean_text"])
	assert token is not None
	tokens.append(token.group())
	print(f"Turn {number}: {response['clean_text']}")
assert tokens[0] == tokens[1]
assert tokens[0] != tokens[2]
print("Same conversation reuses token: true")
print("Different conversation isolates token: true")
print("Daemon exited after stdin closed")
PY
```

The output has this shape. The two prefixes differ:

```text
Turn 1: Email <{session_a}:Email_1>.
Turn 2: Reply to <{session_a}:Email_1>.
Turn 3: Email <{session_b}:Email_1>.
Same conversation reuses token: true
Different conversation isolates token: true
Daemon exited after stdin closed
```

Only the cleaned text goes to a model. Keep session management in the owner
application and use a different `session_id` for each isolation boundary.

## What this proves

Exact repeats reuse a token inside a live session. Tokens do not prove that
two different names refer to the same person. Restart or eviction drops the
daemon's session state.

The daemon protocol cleans text; it accepts no restore request and returns no
`session_blob`. For a restorable CLI loop, use
[Your first pseudonymization](first-pseudonymization.md).
For shutdown, eviction, and supervision, use [Run the daemon](../how-to/daemon/run-daemon.md).
