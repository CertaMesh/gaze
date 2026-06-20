#!/usr/bin/env python3
"""Populate OPF direct, observer-residual, and latency benchmark cells."""

from __future__ import annotations

import argparse

from safety_net_bench_lib import add_common_args, scorer_main


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    add_common_args(parser, "opf")
    return parser.parse_args()


if __name__ == "__main__":
    raise SystemExit(scorer_main(parse_args(), "opf"))
