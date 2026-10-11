"""Committed dictionary generator registry, shared by replay and gate identity.

The hash covers the decompressed, retained producer source. Only its input
construction definitions are loaded; historical replay/scoring code is excluded.
"""
from __future__ import annotations

import ast
import gzip
import hashlib
import random
import sys
import zlib
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType

import gaze_bench_score as score

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_GENERATOR = "35e4a0950451eefc5022700d8424b13369a08e5f822a4a3083cb0a7a65b8ec90"
GENERATOR_SOURCES = {
    DEFAULT_GENERATOR: "docs/reference/benchmarks/dictionary-sequences/generator-v1.py.gz",
}


class GeneratorProvenanceError(ValueError):
    """Unknown or altered generator provenance, without exposing input values."""

    def __init__(self):
        super().__init__("dictionary generator source is not verified")


def resolve(generator_sha256):
    """Verify retained bytes on every resolution, then load input functions only."""
    try:
        path = ROOT / GENERATOR_SOURCES[generator_sha256]
        source = gzip.decompress(path.read_bytes())
    except (KeyError, TypeError, OSError, EOFError, zlib.error):
        raise GeneratorProvenanceError() from None
    if hashlib.sha256(source).hexdigest() != generator_sha256:
        raise GeneratorProvenanceError()
    # A separate namespace uses today's canonical scorer value contracts. No
    # snapshot imports, top-level commands, replay, or scorer can be executed.
    tree = ast.parse(source)
    names = {"LAYERS", "SEQUENCES", "VERSION", "SEED", "Case", "generate", "documents"}
    definitions = []
    for node in tree.body:
        if isinstance(node, (ast.ClassDef, ast.FunctionDef)) and node.name in names:
            definitions.append(node)
        elif isinstance(node, ast.Assign) and any(
                isinstance(target, ast.Name) and target.id in names for target in node.targets):
            definitions.append(node)
    isolated = ModuleType("_gaze_dictionary_generator")
    isolated.__dict__.update(score=score, random=random, dataclass=dataclass)
    # Dataclasses consult their defining module while processing annotations.
    previous = sys.modules.get(isolated.__name__)
    sys.modules[isolated.__name__] = isolated
    try:
        exec(compile(ast.Module(body=definitions, type_ignores=[]), str(path), "exec"),
             isolated.__dict__)
    finally:
        if previous is None:
            del sys.modules[isolated.__name__]
        else:
            sys.modules[isolated.__name__] = previous
    return isolated
