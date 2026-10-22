# Protect an order ID with a custom recognizer

We will add an order-ID recognizer, check that it produces a token, and prove
that the token restores the original bytes. Allow ten minutes.

You need `gaze`, Python 3, and a POSIX shell. This exercise uses deterministic
rules and needs no model download.

## 1. Write a policy

First, create a private working directory and a policy:

```sh
umask 077
mkdir gaze-order-demo
cd gaze-order-demo
cat > policy.toml <<'TOML'
[session]
scope = "conversation"

[policy.rulepacks]
bundled = ["core"]

[[policy.custom_recognizers]]
kind = "regex"
name = "order-id"
class = "custom:order_id"
pattern = '\bORD-\d{6}\b'

[[rule]]
kind = "default"
action = "tokenize"
TOML
printf 'Policy ready\n'
```

You should see:

```text
Policy ready
```

The custom pattern matches six-digit `ORD-` values. The default rule also
tokenizes any class that the bundled `core` rules detect.

## 2. Clean a fictional order

Now, clean a sample order and inspect its token:

```sh
printf 'Order ORD-123456 is ready.\n' > input.txt
gaze clean --policy policy.toml < input.txt > owner-clean.json
python3 - <<'PY'
import json
from pathlib import Path

result = json.loads(Path("owner-clean.json").read_text())
assert "ORD-123456" not in result["clean_text"]
assert ":Custom:order_id_" in result["clean_text"]
print(result["clean_text"], end="")
PY
```

The output has this shape, with this run's session prefix:

```text
Order <{session_hex}:Custom:order_id_1> is ready.
```

Keep `owner-clean.json` private. Only its `clean_text` value may go to a model.

## 3. Prove the round trip

Restore with the returned snapshot and compare the bytes:

```sh
python3 - <<'PY'
import json
from pathlib import Path
import subprocess

result = json.loads(Path("owner-clean.json").read_text())
request = {"session_blob": result["session_blob"], "text": result["clean_text"]}
response = subprocess.run(
	["gaze", "restore"], input=json.dumps(request), text=True,
	capture_output=True, check=True,
)
restored = json.loads(response.stdout)["text"]
assert restored.encode() == Path("input.txt").read_bytes()
print("Order token and byte-exact restore: OK")
PY
```

The assertions pass:

```text
Order token and byte-exact restore: OK
```

For dictionaries, locale restrictions, and library assembly, use
[Add a custom recognizer](../how-to/policy/custom-recognizers.md).
For regex fields and validation, see the [policy reference](../reference/policy.md#policycustom_recognizers).
