#!/usr/bin/env python3
"""Derive Gaze ablations from the exact setup policy used for the full row."""

from __future__ import annotations

import argparse
import re
import tomllib
from pathlib import Path


def without_sections(source: str, sections: tuple[str, ...]) -> str:
    result = source
    for section in sections:
        pattern = rf"(?ms)^\[{re.escape(section)}\]\n.*?(?=^\[|\Z)"
        result, count = re.subn(pattern, "", result)
        if count != 1:
            raise ValueError(f"expected exactly one [{section}] section, got {count}")
    before = tomllib.loads(source)
    after = tomllib.loads(result)
    for section in sections:
        before.pop(section.split('.')[0], None)
    if after != before:
        raise ValueError("ablation changed a policy section other than the requested ones")
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("policy", type=Path)
    args = parser.parse_args()
    source = args.policy.read_text(encoding="utf-8")
    for suffix, sections in (
        ("rules-ner", ("safety_net", "safety_net.nym")),
        ("rules-only", ("ner", "safety_net", "safety_net.nym")),
    ):
        output = args.policy.with_name(f"policy-{suffix}.toml")
        output.write_text(without_sections(source, sections), encoding="utf-8")
        print(output)


if __name__ == "__main__":
    main()
