#!/usr/bin/env python3
"""Pin a captured record to a committed scorecard after correctness calibration."""

import argparse
from pathlib import Path

from scorecard_record import pin_template


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("record", type=Path)
    parser.add_argument("scorecard", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    pin_template(args.record, args.scorecard, args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
