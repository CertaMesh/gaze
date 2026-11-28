"""Rebuild the bundled chat-name vocabulary from public-domain US government counts.

Supply baby-names.csv from https://github.com/hadley/data-baby-names/tree/b6525579cf7d2816e77fa885299445d15a09c906
(SSA national top 1,000 per sex; use the latest complete year in that snapshot),
https://www2.census.gov/topics/genealogy/2010surnames/Names_2010Census_Top1000.xlsx,
and the Library of Congress language code table
https://www.loc.gov/standards/iso639-2/ISO-639-2_utf-8.txt (English names).
These are evidence for a cued match, not an exhaustive list of personal names.

    python3 scripts/fetch/build_chat_name_vocab.py baby-names.csv Names_2010Census_Top1000.xlsx ISO-639-2_utf-8.txt
"""

import argparse
import csv
import hashlib
import io
import json
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2] / "crates/gaze-recognizers"
MARKER = "# BEGIN generated chat name vocabulary (scripts/fetch/build_chat_name_vocab.py)"
END = "# END generated chat name vocabulary"


def checked(path, expected):
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise SystemExit(f"unexpected SHA-256: {path}")
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ssa_csv", type=Path)
    parser.add_argument("census_xlsx", type=Path)
    parser.add_argument("languages", type=Path)
    args = parser.parse_args()
    data = checked(args.ssa_csv, "1259523fa76e5c18151a4c7612b854b22605d07127c596126940e2941ba15d3c")
    given = {r["name"].casefold() for r in csv.DictReader(io.StringIO(data.decode())) if r["year"] == "2008"}
    data = checked(args.census_xlsx, "ea30217a15a8b4f4241ee1940ab0d8e0fa6928158ab8ec63076c98aa60e614d8")
    with zipfile.ZipFile(io.BytesIO(data)) as book:
        strings = [node.findtext("{*}t") for node in ET.fromstring(book.read("xl/sharedStrings.xml"))]
        rows = ET.fromstring(book.read("xl/worksheets/sheet1.xml")).find("{*}sheetData")
        surnames = {
            strings[int(row[0].findtext("{*}v"))].casefold()
            for row in rows
            if len(row) >= 2
            and row[0].get("t") == "s"
            and row[1].get("t") is None
            and (row[1].findtext("{*}v") or "").isdecimal()
        }
    # Ordinary names must not become unanchored repeat matches in this or later
    # turns. Admit an ambiguous introduction only if the sweep will not copy it.
    sweep_words = {
        word
        for line in (ROOT.parent / "gaze/src/sweep_common_words.txt").read_text().splitlines()
        if not line.startswith("#")
        for word in line.split()
    }
    languages = checked(args.languages, "42b71885e4dc885559fda5ad059fd81838cf4782cedb5fd08cc3431f7d067371")
    ambiguous = {
        name.strip().casefold()
        for line in languages.decode("utf-8-sig").splitlines()
        for name in line.split("|")[3].split(";")
    } | sweep_words
    for asset in ("record-common-name-words-en-v1.txt", "record-common-names-v1.txt"):
        ambiguous.update(
            line
            for line in (ROOT / "assets" / asset).read_text().splitlines()
            if line and not line.startswith("#")
        )
    core = ROOT / "embedded/core.toml"
    before, block = core.read_text().split(MARKER)
    _, after = block.split(END)
    content = (
        before
        + MARKER
        + "\n# Common-word source and redistribution notice: assets/record-common-name-words-en-v1.txt.\n"
    )
    for bucket, values in [
        ("chat_distinct_given", given - ambiguous),
        ("chat_ambiguous_given", given & sweep_words),
        ("chat_surnames", surnames),
    ]:
        words = sorted(values)
        content += f"[locale.{bucket}]\nnames = [\n"
        for i in range(0, len(words), 10):
            content += "  " + ", ".join(json.dumps(w, ensure_ascii=False) for w in words[i : i + 10]) + ",\n"
        content += "]\n"
    core.write_text(content + END + after)


if __name__ == "__main__":
    main()
