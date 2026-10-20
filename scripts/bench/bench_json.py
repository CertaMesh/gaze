#!/usr/bin/env python3
"""Committed benchmark JSON stored as deterministic gzip.

Release scorecards (``docs/reference/benchmarks/scorecard-vX.Y.Z*.json.gz``)
are committed gzipped. Their recorded ``scorecard_sha256`` is the digest of
the *decompressed* JSON bytes, so pins taken on the plain files stay valid
after compression and do not depend on the gzip encoder. Other committed
``.gz`` evidence (observation records, mechanism and full-name scorecards)
keeps pinning its compressed bytes.

    python3 scripts/bench/bench_json.py compress SOURCE.json TARGET.json.gz
    python3 scripts/bench/bench_json.py sha256 FILE.json[.gz]
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import sys
from pathlib import Path
from typing import Any, Sequence

GZIP_SUFFIX = ".gz"
GZIP_LEVEL = 9


def read_content(path: Path) -> bytes:
    """The JSON bytes of ``path``, decompressed when it ends in ``.gz``."""
    raw = path.read_bytes()
    return gzip.decompress(raw) if path.suffix == GZIP_SUFFIX else raw


def load(path: Path) -> Any:
    return json.loads(read_content(path).decode("utf-8"))


def content_sha256(path: Path) -> str:
    """SHA-256 of the decompressed JSON bytes; the digest every pin records."""
    return hashlib.sha256(read_content(path)).hexdigest()


def gzip_bytes(content: bytes) -> bytes:
    """Byte-reproducible gzip: no file name, zero mtime, fixed level."""
    buffer = io.BytesIO()
    with gzip.GzipFile(
        filename="", mode="wb", fileobj=buffer, mtime=0, compresslevel=GZIP_LEVEL
    ) as stream:
        stream.write(content)
    return buffer.getvalue()


def compress(source: Path, target: Path) -> str:
    """Write ``source`` to ``target`` as gzip and return the content digest."""
    if source.suffix == GZIP_SUFFIX:
        raise ValueError(f"{source} is already compressed")
    if target.suffix != GZIP_SUFFIX:
        raise ValueError(f"{target} must end in {GZIP_SUFFIX}")
    content = source.read_bytes()
    json.loads(content.decode("utf-8"))
    target.write_bytes(gzip_bytes(content))
    if read_content(target) != content:
        raise ValueError(f"{target} does not decompress to {source}")
    return hashlib.sha256(content).hexdigest()


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    compress_parser = commands.add_parser("compress", help="gzip one JSON file")
    compress_parser.add_argument("source", type=Path)
    compress_parser.add_argument("target", type=Path)
    sha_parser = commands.add_parser("sha256", help="print the content digest")
    sha_parser.add_argument("path", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "compress":
            print(compress(args.source, args.target))
        else:
            print(content_sha256(args.path))
    except (OSError, ValueError) as error:
        print(f"bench_json: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
