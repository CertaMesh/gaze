"""Producer-owned build receipts; imported binaries remain exploratory evidence.

The local builder and its tools are trusted. This binds an executed producer
build to a clean source revision and its resulting bytes, not a public source
attestation or a defense against consistently fabricated artifacts.
"""

import os
import re

import gaze_bench_score as score


def unqualified(reason):
    return {"version": 1, "mode": "unqualified", "reason": reason}


def build(root, configs, *, release, builder):
    before = score.git_metadata(root)
    binary = builder(root, configs, release=release)
    after = score.git_metadata(root)
    if os.environ.get("CARGO_TARGET_DIR") is not None:
        receipt = unqualified("external_target_directory")
    elif before != after or before.get("dirty") is not False:
        receipt = unqualified("source_changed_or_dirty")
    else:
        receipt = {"version": 1, "mode": "producer-build", "source": before,
                   "binary_sha256": score.sha256_file(binary),
                   "profile": "release" if release else "debug", "configs": list(configs)}
    return binary, receipt


def validate(card, binary_sha256=None):
    receipt = card.get("subject_build")
    if (not isinstance(receipt, dict) or receipt.get("version") != 1
            or receipt.get("mode") != "producer-build"
            or receipt.get("source") != card.get("gaze")
            or receipt.get("source", {}).get("dirty") is not False
            or not re.fullmatch(r"[0-9a-f]{40}", str(receipt.get("source", {}).get("revision", "")))
            or not re.fullmatch(r"[0-9a-f]{64}", str(receipt.get("binary_sha256", "")))
            or (binary_sha256 is not None and receipt["binary_sha256"] != binary_sha256)
            or receipt.get("profile") != card.get("parameters", {}).get("binary_profile")
            or receipt.get("configs") != card.get("parameters", {}).get("configs")):
        raise ValueError("unqualified_subject_build")
