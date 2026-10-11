#!/usr/bin/env python3
"""Re-score value-free supplemental OPF/NER observations without model runs."""

import argparse
import json
from pathlib import Path

from safety_net_bench_lib import rescore_observations, sha256_file


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("observations", type=Path)
    args = parser.parse_args()
    digest = sha256_file(args.observations)
    if not args.observations.name.endswith(f".observations-{digest}.json"):
        raise ValueError("observation filename does not match its SHA-256")
    metrics = rescore_observations(json.loads(args.observations.read_text(encoding="utf-8")))
    print(json.dumps(metrics, indent=2))


if __name__ == "__main__":
    main()
