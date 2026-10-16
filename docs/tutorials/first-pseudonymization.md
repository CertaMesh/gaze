# Your first pseudonymization

We will turn a fictional email address into a token, then restore the original
file byte for byte. Allow five minutes after the model download.

You need `gaze` with its default features, Python 3, and a POSIX shell.
See [installation](../../README.md#install) if the binary is not on your path.
The first setup needs network access and disk space for the pinned models.

## 1. Create a policy

First, create a working directory with owner-only permissions:

```sh
umask 077
mkdir gaze-first-run
cd gaze-first-run
gaze setup --non-interactive
```

Setup installs or verifies the pinned NER and Nym bundles, writes `gaze.toml`,
and checks the pipeline. Its output includes:

```text
doctor Nym pass: synthetic licence plate tokenized
Setup complete.
```

Keep this shell open for the remaining steps.

## 2. Create a fictional input

Now, write a sample address from the reserved `example.invalid` domain:

```sh
printf 'Contact %s@%s for details.\n' alice example.invalid > input.txt
python3 - <<'PY'
from pathlib import Path
print(f"Created input.txt ({len(Path('input.txt').read_bytes())} bytes)")
PY
```

The command reports the file size:

```text
Created input.txt (43 bytes)
```

## 3. Inspect the cleaned text

Keep `owner-clean.json` private. It contains the session snapshot needed to
restore the original address. Send only `clean.txt` to a model.

```sh
gaze clean --policy gaze.toml < input.txt > owner-clean.json
python3 - <<'PY'
import json
from pathlib import Path

result = json.loads(Path("owner-clean.json").read_text())
Path("clean.txt").write_text(result["clean_text"])
print(result["clean_text"], end="")
print("Saved owner-clean.json and clean.txt")
PY
```

The email becomes a token:

```text
Contact <{session_hex}:Email_1> for details.
Saved owner-clean.json and clean.txt
```

`{session_hex}` stands for the eight-character prefix returned by this run.
Use the actual token in `clean.txt` in the next step.

## 4. Restore with the same session

We will restore the cleaned text unchanged so we can compare the exact bytes.
First, prepare the restore request from the saved snapshot:

```sh
python3 - <<'PY'
import json
from pathlib import Path

result = json.loads(Path("owner-clean.json").read_text())
request = {
	"session_blob": result["session_blob"],
	"text": Path("clean.txt").read_text(),
}
Path("owner-restore.json").write_text(json.dumps(request))
print("Prepared owner-restore.json from the cleaning session")
PY
```

You should see:

```text
Prepared owner-restore.json from the cleaning session
```

Now, restore and compare the files:

```sh
gaze restore < owner-restore.json > owner-restored.json
python3 - <<'PY'
import json
from pathlib import Path

restored = json.loads(Path("owner-restored.json").read_text())["text"]
Path("restored.txt").write_text(restored)
assert Path("restored.txt").read_bytes() == Path("input.txt").read_bytes()
print("Byte-exact restore: OK")
PY
```

The comparison succeeds:

```text
Byte-exact restore: OK
```

Keep `input.txt`, `restored.txt`, and the `owner-*.json` files on the owner side.
For a model response, use its text as the restore request's `text` value and
keep the same `session_blob`.

This sample proves one round trip. Detection coverage and one-way fallback
limits are described in [How Gaze works](../explanation/how-gaze-works.md#limits).
For policy choices, use [Set up a policy by hand](../how-to/manual-policy.md).
