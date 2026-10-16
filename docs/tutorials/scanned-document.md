# Clean a scanned document into a SafeBundle

<!-- tutorial-gate: start=checkout -->

We will turn a fictional invoice image into tokenized Markdown and keep its
restore manifest in a separate owner directory. Allow fifteen minutes.

Start in a Gaze source checkout. You need Python 3, Tesseract on your path,
and `gaze` built with the `document` feature. For installation, use
[Ingest documents](../how-to/document/ingest-documents.md#prerequisites).

## 1. Copy the sample image

First, copy the repository's fictional invoice into a private working directory:

```sh
umask 077
mkdir -p target/document-tutorial
cp crates/gaze-document/testdata/synthetic_image.png target/document-tutorial/invoice.png
cd target/document-tutorial
python3 - <<'PY'
from pathlib import Path
assert Path("invoice.png").is_file()
print("Fictional invoice ready")
PY
```

You should see:

```text
Fictional invoice ready
```

## 2. Create separate agent and owner outputs

Now, run OCR and pseudonymization:

```sh
gaze document clean invoice.png --agent-out agent --owner-out owner > summary.json
python3 - <<'PY'
import json
from pathlib import Path

assert json.loads(Path("summary.json").read_text())["ok"] is True
assert sorted(p.name for p in Path("agent").iterdir()) == ["clean.md", "report.json"]
assert sorted(p.name for p in Path("owner").iterdir()) == ["manifest.json"]
print("Agent: clean.md, report.json")
print("Owner: manifest.json")
PY
```

The directories contain:

```text
Agent: clean.md, report.json
Owner: manifest.json
```

The manifest contains restore material. Keep `owner/`, the source image, and
the working directory private. Share only the files in `agent/`.

## 3. Inspect the protected Markdown

Read the agent's text and check the sample's detected classes:

```sh
python3 - <<'PY'
import json
from pathlib import Path

clean = Path("agent/clean.md").read_text()
assert ":Email_" in clean
assert ":Name_" in clean
assert ":Custom:phone_" in clean
assert "example.invalid" not in clean
assert "jane.doe" not in clean
assert "Jane Doe" not in clean
assert "555-0142" not in clean
print("Name, email, and phone are tokenized")
report = json.loads(Path("agent/report.json").read_text())
print(f"Report version: {report['bundle_version']}")
print("Sample PII checks: OK")
PY
```

The checks confirm that the name, email, and phone appear as tokens:

```text
Name, email, and phone are tokenized
Report version: 2
Sample PII checks: OK
```

OCR can misread characters; this sample checks only the supplied image.
For PDF support, report fields, and other OCR backends, use
[Ingest documents](../how-to/document/ingest-documents.md).
