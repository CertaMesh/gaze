"""The committed comparison's corpus, rebuilt exactly, and the validation-only view.

The tuned rows sit beside the rows in `comparison.json`, so they must score the
same documents under the same contracts. That report measured the agentic
layers on an older generator; `load_measured` rebuilds that generator's
documents and contract from committed inputs and refuses unless every recorded
identity matches.
"""

from __future__ import annotations

import contextlib
import functools
import hashlib
import json
import sys
from pathlib import Path
from typing import Iterator, Mapping, Sequence

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE.parents[1]))

import compare  # noqa: E402
from comparison_metrics import split_for_id  # noqa: E402

LAYERS = ("C", "A", "D", "R")
COMPARISON = compare.REPO / "docs/reference/benchmarks/comparison.json"


class SplitGuardError(RuntimeError):
    """Selection code touched a test-half document."""


def load_measured(dataset: Path, comparison: Mapping[str, object]) -> dict[str, list]:
    """Layers C/A/D/R exactly as `comparison` measured them, or raise."""
    layers, identity = compare.load_corpus(dataset, None)
    recorded = comparison["corpus"]
    for key in ("main_dataset", "negative_corpus_sha256", "main_selection_digest"):
        if identity[key] != recorded[key]:
            raise ValueError(f"layer C input differs from the comparison report: {key}")
    agentic = compare.agentic
    version = recorded["agentic"]["generator_version"]
    records = agentic.records_as_of(version, agentic.generate(agentic.PUBLISHED_PARTITION))
    if hashlib.sha256(agentic.corpus_bytes(records)).hexdigest() != recorded["agentic"]["corpus_sha256"]:
        raise ValueError(f"generator v{version} does not rebuild the measured agentic corpus")
    contract = agentic.load_contract(compare.REPO, version=version)
    if contract.sha256 != comparison["contracts"]["agentic"]:
        raise ValueError(f"generator v{version} contract differs from the measured one")
    documents = agentic.apply_contract([record.to_document() for record in records], contract)
    result = {"C": layers["C"]}
    for layer in ("A", "D", "R"):
        result[layer] = [d for d in documents if d.cell and d.cell.startswith(layer + "|")]
    for layer in LAYERS:
        ids = [document.uid for document in result[layer]]
        if (len(ids) != recorded["layers"][layer]["documents"]
                or compare.score.document_ids_digest(ids) != recorded["layers"][layer]["ids_sha256"]):
            raise ValueError(f"layer {layer} documents differ from the comparison report")
    return result


@contextlib.contextmanager
def measured_agentic_contract(comparison: Mapping[str, object]) -> Iterator[None]:
    """Score A/D/R under the agentic contract the comparison was measured with.

    compare.measure loads the current generator's contract; the committed rows
    used the one for their generator version, so both must score alike.
    """
    agentic = compare.agentic
    version = comparison["corpus"]["agentic"]["generator_version"]
    original = agentic.load_contract
    agentic.load_contract = functools.partial(original, version=version)
    try:
        if agentic.load_contract(compare.REPO).sha256 != comparison["contracts"]["agentic"]:
            raise ValueError("measured agentic contract does not match the comparison report")
        yield
    finally:
        agentic.load_contract = original


def validation_only(layers: Mapping[str, Sequence]) -> dict[str, tuple]:
    """The validation half of every layer; selection never receives anything else."""
    return {layer: tuple(d for d in documents if split_for_id(d.uid) == "validation")
            for layer, documents in layers.items()}


def require_validation(uids: Sequence[str]) -> None:
    for uid in uids:
        if split_for_id(uid) != "validation":
            raise SplitGuardError(f"selection touched test-half document {uid}")


def read_comparison(path: Path = COMPARISON) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))
