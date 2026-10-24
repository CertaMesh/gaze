"""Producer-owned build receipts; imported binaries remain exploratory evidence.

The local builder and its tools are trusted. This binds an executed producer
build to a clean source revision and its resulting bytes, not a public source
attestation or a defense against consistently fabricated artifacts.
"""

import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

import gaze_bench_score as score


# Requirements shared by the historical and current producer configurations.
# Availability is checked against the measured checkout, never the harness root.
CONFIG_FEATURES = {
    "rule-floor-core": (), "rule-floor-extended": (), "pass2-ner": (),
    "policy-file": ("safety-net-nym",),
    "full-stack-kiji-resolve": ("safety-net-kiji",),
    "pass3-kiji": ("safety-net-kiji",),
    "full-stack-nym-resolve": ("safety-net-nym",),
    "full-stack-nym-redact": ("safety-net-nym",),
    "full-stack-opf-resolve": ("safety-net-openai",),
    "pass3-opf": ("safety-net-openai",),
    "pass3-locale-aware": ("safety-net-kiji", "safety-net-openai"),
}


def required_features(configs):
    if any(config not in CONFIG_FEATURES for config in configs):
        raise ValueError("unsupported_benchmark_config")
    return sorted({feature for config in configs for feature in CONFIG_FEATURES[config]})


def build_features(root, configs):
    features = required_features(configs)
    if configs:
        package = Path(root) / "crates/gaze-recognizers"
        manifest = tomllib.loads((package / "Cargo.toml").read_text())
        source = (package / "examples/clean_for_bench.rs").read_text()
        available = set(re.findall(r'"([a-z0-9-]+)"\s*=>\s*BenchConfig::', source))
        if not set(configs) <= available or not set(features) <= manifest.get("features", {}).keys():
            raise ValueError("unsupported_checkout_build_features")
    return features


def unqualified(reason):
    return {"version": 1, "mode": "unqualified", "reason": reason}


def build(root, configs, *, release, builder):
    before = score.git_metadata(root)
    features = build_features(root, configs)
    binary = builder(root, configs, release=release)
    after = score.git_metadata(root)
    if os.environ.get("CARGO_TARGET_DIR") is not None:
        receipt = unqualified("external_target_directory")
    elif before != after or before.get("dirty") is not False:
        receipt = unqualified("source_changed_or_dirty")
    else:
        receipt = {"version": 1, "mode": "producer-build", "source": before,
                   "binary_sha256": score.sha256_file(binary),
                   "profile": "release" if release else "debug", "configs": list(configs),
                   "features": features}
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
    try:
        if receipt.get("features") != required_features(receipt["configs"]):
            raise ValueError("unqualified_subject_build")
    except (KeyError, TypeError, ValueError) as error:
        raise ValueError("unqualified_subject_build") from error


def cargo_build_binary(repo_root: Path, configs: tuple[str, ...], *, release: bool = False) -> Path:
    command = [
        "cargo",
        "build",
        "--locked",
        "--message-format=json-render-diagnostics",
        "-p",
        "gaze-recognizers",
        "--example",
        "clean_for_bench",
    ]
    features = build_features(repo_root, configs)
    if features:
        command.extend(["--features", ",".join(features)])
    if release:
        command.append("--release")
    result = subprocess.run(command, cwd=repo_root, check=True, text=True, capture_output=True)
    artifacts = []
    for line in result.stdout.splitlines():
        item = json.loads(line)
        if (item.get("reason") == "compiler-artifact"
                and item.get("target", {}).get("name") == "clean_for_bench"
                and "example" in item.get("target", {}).get("kind", [])
                and item.get("executable")):
            artifacts.append(Path(item["executable"]).resolve())
    if len(artifacts) != 1 or not artifacts[0].is_file():
        raise FileNotFoundError("benchmark build did not return one executable")
    return artifacts[0]
