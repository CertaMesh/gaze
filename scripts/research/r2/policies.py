#!/usr/bin/env python3
"""Apply predeclared research deltas to this checkout's own setup policy."""
import argparse
from pathlib import Path
import re

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("baseline", type=Path)
parser.add_argument("out", type=Path)
args = parser.parse_args()
baseline = args.baseline.read_text()


def without(text, heading):
    return re.sub(r"(?ms)^\[" + re.escape(heading) + r"\]\n.*?(?=^\[|\Z)", "", text)


arms = {"full": baseline}
ner = without(without(baseline, "safety_net.nym"), "safety_net")
arms["rules+ner"] = ner
arms["rules"] = without(ner, "ner")
for threshold in (0.1, 0.9):
    arms[f"name-{threshold}"] = baseline + f'''
# Predeclared sensitivity arm; all non-name op-B thresholds stay unchanged.
labels = ["BUILDING_NUMBER", "DATE_OF_BIRTH", "GIVEN_NAME", "LICENSE_PLATE", "SURNAME", "USERNAME"]
'''
    # Insert into existing safety_net.nym table, before the next table.
    labels = arms[f"name-{threshold}"].split("# Predeclared")[1]
    labels = "# Predeclared" + labels
    arms[f"name-{threshold}"] = baseline.replace(
        "[address_blocks]", labels + f'''
[safety_net.nym.threshold]
BUILDING_NUMBER = 0.5
DATE_OF_BIRTH = 0.9
GIVEN_NAME = {threshold}
LICENSE_PLATE = 0.5
SURNAME = {threshold}
USERNAME = 0.5

[address_blocks]''')
args.out.mkdir(parents=True, exist_ok=True)
for name, text in arms.items():
    (args.out / f"{name}.toml").write_text(text)
