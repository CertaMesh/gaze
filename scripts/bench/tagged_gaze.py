"""One rule for every public benchmark block: Gaze numbers come from tagged releases.

The comparison report, the third-party files, the leak ledger and the mechanism
ledger all hold measurements of an untagged build (`main`, a candidate branch).
That evidence stays committed as JSON, but no generated public page or block may
show it. Each renderer routes its output through `check_public`, which refuses
text that names or numbers such a build, so a new untagged row fails the
renderer instead of reaching the README.
"""

from __future__ import annotations

import re
import subprocess
from pathlib import Path

TAG = re.compile(r"v\d+\.\d+\.\d+")


class UntaggedGazeError(ValueError):
    """A public block would show a Gaze measurement that no release tag owns."""


#: Shapes an untagged Gaze row takes in generated Markdown. A tagged row reads
#: `Gaze v0.15.1` / `Gaze 0.15`; none of these appear in it.
UNTAGGED_SHAPES: tuple[tuple[str, re.Pattern[str]], ...] = (
    ("a main-branch Gaze row", re.compile(r"\bgaze-(?:full|rules-ner|rules-only)\b")),
    ("a Gaze main bar or label", re.compile(r"\bGaze main\b", re.IGNORECASE)),
    ("an unreleased build", re.compile(r"\(unreleased\)")),
    ("a Gaze table row from the comparison report", re.compile(r"^\|[^|\n]*\|[^|\n]*\| gaze \|", re.MULTILINE)),
    ("a revision-pinned Gaze measurement", re.compile(r"\bGaze\b[^|\n]{0,40}`[0-9a-f]{7,40}`")),
    ("a Gaze crates tree", re.compile(r"Gaze ran on crates tree")),
    ("a main-branch ledger", re.compile(r"\bMain `?[0-9a-f]{7,40}")),
)


def check_public(text: str, where: str) -> str:
    """Return `text` unchanged, or raise when it shows an untagged Gaze build."""
    for what, pattern in UNTAGGED_SHAPES:
        match = pattern.search(text)
        if match:
            raise UntaggedGazeError(
                f"{where}: shows {what} ({match.group(0)[:60]!r}); public blocks show "
                "tagged releases only"
            )
    return text


def require_tag(version: str, where: str) -> str:
    """A Gaze version that is a release tag, or raise."""
    if not TAG.fullmatch(version):
        raise UntaggedGazeError(f"{where}: {version!r} is not a release tag")
    return version


REPO = Path(__file__).resolve().parents[2]


def tag_commit(version: str, repo: Path = REPO) -> str:
    """The commit a release tag points at, resolved as `refs/tags/<version>` only.

    A bare `<version>:crates` would also resolve a branch or a hex-named ref, so a
    version-shaped branch could pass for a release. This names the tag namespace.
    """
    require_tag(version, "release tag")
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "--verify", "--quiet", f"refs/tags/{version}^{{commit}}"],
            cwd=repo, text=True, stderr=subprocess.PIPE,
        ).strip()
    except (subprocess.CalledProcessError, FileNotFoundError) as error:
        raise UntaggedGazeError(
            f"{version} is not a git tag in this checkout (a branch of that name does not "
            "count); fetch tags, for example actions/checkout with fetch-depth: 0"
        ) from error


def require_release_tag(version: str, where: str) -> str:
    """`version` names a real release tag, or raise."""
    require_tag(version, where)
    tag_commit(version)
    return version


#: What a tagged release is measured with, pinned by digest and reviewed like any other pin:
#: the `gaze setup` policy the release writes (home paths normalised) and the two model bundles
#: it names (`compare.digest_tree`). A different policy or bundle is a different measurement.
RELEASE_PINS = {
    "v0.15.1": {
        "policy_home_normalized_sha256": "481f5df7a9b0b562bf2c2db7274cfbf2ea701231c52f1287b27de00554453ff1",
        "ner_model_tree_sha256": "1196662dfcf78d3ef4a9fbbd474d5dd4ce9e656d5fdc38ec38eff3d3249ba1fe",
        "nym_model_tree_sha256": "1878b4af812a531f5710692287e529106951d78e0e270820b05fa8e6ae082a2d",
    },
}


def check_model_receipt(receipt: dict, pinned: dict, where: str) -> None:
    """A model a measurement used must be the pinned one: repository, revision and tree digest.

    `receipt` is what the producer wrote after hashing the directory it ran; `pinned` is the
    reviewed declaration (vendor-tuned.json). Used by the producer (before inference), the
    harness (before scoring) and the merge, so no step trusts the one before it.
    """
    for key in ("model", "revision", "tree_sha256"):
        if receipt.get(key) != pinned[key]:
            raise ValueError(f"{where}: model {key} is {receipt.get(key)!r}, the pinned value is {pinned[key]!r}")


def check_own_input(own: dict, prediction_sha256: str, dataset_sha256: str, where: str) -> None:
    """A vendor evaluator's score must be of the measured row's own predictions on the pinned dataset.

    `own["input"]` is the receipt the replay wrote: the SHA-256 of the prediction file it read and
    of the dataset it scored against.
    """
    receipt = own.get("input")
    if not receipt:
        raise ValueError(f"{where}: the own-scorer result records no input receipt")
    if receipt.get("prediction_sha256") != prediction_sha256:
        raise ValueError(f"{where}: the own scorer read predictions {receipt.get('prediction_sha256')}, "
                         f"the measured row's are {prediction_sha256}")
    if receipt.get("dataset_sha256") != dataset_sha256:
        raise ValueError(f"{where}: the own scorer used dataset {receipt.get('dataset_sha256')}, "
                         f"the pinned one is {dataset_sha256}")
