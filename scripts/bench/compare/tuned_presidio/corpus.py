"""The committed comparison's corpus, rebuilt exactly, and the validation-only view.

The tuned rows sit beside the rows in `comparison.json`, so they must score the
same documents under the same contracts. That report measured the agentic
layers on an older generator; `load_measured` rebuilds that generator's
documents and contract from committed inputs and refuses unless every recorded
identity matches.

Selection must never read test-half text or gold, so the corpus reaches it
through files split by half: `write_halves` (its own step, before selection)
loads and verifies everything once and writes `validation.pickle` and
`test.pickle`; `load_validation`, selection's only corpus input, opens the
validation file alone.
"""

from __future__ import annotations

import contextlib
import functools
import hashlib
import json
import pickle
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


HALVES = ("validation", "test")
HALVES_META = "halves.meta.json"


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def write_halves(layers: Mapping[str, Sequence], out: Path, comparison_path: Path = COMPARISON) -> dict[str, object]:
    """Write each half of `layers` to its own file, with their digests.

    `layers` is `load_measured`'s output; this is the only place both halves are
    held together before selection. The pickles are local run artefacts, never
    committed, and `load_validation` checks the digest before unpickling.
    """
    out.mkdir(parents=True, exist_ok=True)
    meta: dict[str, object] = {"comparison_sha256": _sha256(comparison_path.read_bytes()), "halves": {}}
    for half in HALVES:
        part = {layer: [d for d in documents if split_for_id(d.uid) == half] for layer, documents in layers.items()}
        payload = pickle.dumps(part, protocol=5)
        (out / f"{half}.pickle").write_bytes(payload)
        meta["halves"][half] = {"sha256": _sha256(payload),
                                "documents": {layer: len(documents) for layer, documents in part.items()}}
    (out / HALVES_META).write_text(json.dumps(meta, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return meta


def load_validation(halves: Path, comparison_path: Path = COMPARISON) -> tuple[dict[str, tuple], str]:
    """The validation half and its file digest; never opens the test-half file."""
    meta = json.loads((halves / HALVES_META).read_text(encoding="utf-8"))
    if meta["comparison_sha256"] != _sha256(comparison_path.read_bytes()):
        raise ValueError("the split halves were written for a different comparison report")
    payload = (halves / "validation.pickle").read_bytes()
    digest = _sha256(payload)
    if digest != meta["halves"]["validation"]["sha256"]:
        raise ValueError("validation.pickle differs from the split metadata")
    layers = pickle.loads(payload)
    for documents in layers.values():
        require_validation([document.uid for document in documents])
    return {layer: tuple(documents) for layer, documents in layers.items()}, digest


def read_comparison(path: Path = COMPARISON) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))
