"""Emulates the input and output contract of `opf` at privacy-filter f7f00ca7, without a model.

Input selection copies `opf/_cli/args.py::iter_inputs` and `_read_text_file`: each `--text-file`
is read whole in Python text mode (so `\\r\\n` and a lone `\\r` become `\\n`) and skipped when
empty; otherwise piped stdin yields one input per non-blank line with the line ending stripped.
Every input prints its own JSON object, followed by the ANSI colour section unless
`--no-print-color-coded-text` is passed. Offsets are Python `str` indices into the analysed input.

The test writes a JSON config next to this file (`emulator.json`):
  {"needles": [[label, substring], ...]}  -> one span per occurrence of each substring, or
  {"spans": [[label, start, end], ...]}   -> these offsets verbatim, for every input.
"""
import json
import sys
from pathlib import Path

sys.stdin.reconfigure(encoding="utf-8")
sys.stdout.reconfigure(encoding="utf-8")
config = json.loads((Path(__file__).parent / "emulator.json").read_text(encoding="utf-8"))
argv = sys.argv[1:]
text_files = [argv[i + 1] for i, arg in enumerate(argv[:-1]) if arg in ("--text-file", "-f")]
print_colour = "--no-print-color-coded-text" not in argv


def inputs():
    if text_files:
        for path in text_files:
            text = Path(path).expanduser().read_text(encoding="utf-8")
            if not text:
                continue
            yield text
        return
    for raw in sys.stdin:
        line = raw.rstrip("\r\n")
        if not line.strip():
            continue
        yield line


def spans_for(text):
    if "spans" in config:
        return [{"label": label, "start": start, "end": end} for label, start, end in config["spans"]]
    spans = []
    for label, needle in config["needles"]:
        start = text.find(needle)
        while start != -1:
            spans.append({"label": label, "start": start, "end": start + len(needle)})
            start = text.find(needle, start + 1)
    return sorted(spans, key=lambda span: span["start"])


for text in inputs():
    output = {"schema_version": 1, "text": text, "detected_spans": spans_for(text), "redacted_text": ""}
    print(json.dumps(output, indent=2))
    if print_colour:
        print("color coded text:\n\x1b[38;5;201m" + text + "\x1b[0m")
