#!/usr/bin/env python3
"""Prepare a same-binary credential mechanism arm without running a benchmark.

Both policies retain the setup policy's other bundles, rules and model paths.
The base replaces bundled secrets with the two pre-existing recognizers;
the candidate appends only the new credential rulepack. mechanism_arms.py
accepts the committed append_rulepack_paths delta for this comparison.
"""
import argparse
import copy
import json
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[2]
NEW_IDS = {"password.hash", "password.code", "security_token.code"}
DELTA = ROOT / "scripts/bench/mechanisms/password-code-credentials.toml"
RULEPACK_PATH = Path("target/bench-mechanisms/password-code-credentials.toml")


def split_rulepack(source):
    header, *blocks = source.split("\n[[recognizers]]")
    old, new = [], []
    for block in blocks:
        raw = "\n[[recognizers]]" + block
        spec = tomllib.loads(header + raw)["recognizers"][0]
        (new if spec["id"] in NEW_IDS else old).append(raw)
    if {row["id"] for row in tomllib.loads(header + "".join(new))["recognizers"]} != NEW_IDS:
        raise ValueError("credential recognizer set differs")
    # The external arm is the same definitions, with a distinct pack identity.
    return (header + "".join(old),
            header.replace('rulepack_id = "gaze-secrets"', 'rulepack_id = "gaze-credential-code"') + "".join(new))


def prepare(source, output):
    policy = tomllib.loads(source)
    bundles = policy["policy"]["rulepacks"]["bundled"]
    if bundles.count("secrets") != 1:
        raise ValueError("use a setup policy with exactly one secrets bundle")
    old, new = split_rulepack((ROOT / "crates/gaze-recognizers/embedded/secrets.toml").read_text())
    output.mkdir(parents=True, exist_ok=True)
    old_path = output.resolve() / "base-secrets.toml"
    new_path = ROOT / RULEPACK_PATH
    new_path.parent.mkdir(parents=True, exist_ok=True)
    # The committed delta uses this repository-relative generated input.
    # Run both arms from ROOT, as the benchmark runner does.
    base = copy.deepcopy(policy)
    base["policy"]["rulepacks"]["bundled"].remove("secrets")
    base["policy"]["rulepacks"].setdefault("paths", []).append(str(old_path))
    candidate = copy.deepcopy(base)
    candidate["policy"]["rulepacks"]["paths"].append(RULEPACK_PATH.as_posix())

    def rewrite(expected):
        result = source
        section = re.search(r'(?ms)^\[policy\.rulepacks\][^\n]*\n(.*?)(?=^\[|\Z)', source)
        if section is None:
            raise ValueError("setup policy has no rulepack section")
        replacement = '[policy.rulepacks]\n' + "\n".join(
            key + " = " + json.dumps(value) for key, value in expected["policy"]["rulepacks"].items()
        ) + "\n\n"
        result = result[:section.start()] + replacement + result[section.end():]
        if tomllib.loads(result) != expected:
            raise ValueError("arm preparation changed unrelated policy settings")
        return result

    old_path.write_text(old)
    new_path.write_text(new)
    (output / "base.toml").write_text(rewrite(base))
    (output / "candidate.toml").write_text(rewrite(candidate))
    delta = tomllib.loads(DELTA.read_text())
    if delta != {"mechanism_delta": {"append_rulepack_paths": [RULEPACK_PATH.as_posix()]}}:
        raise ValueError("committed credential mechanism delta differs")
    (output / "delta.toml").write_text(DELTA.read_text())
    return base, candidate


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--setup-policy", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    prepare(args.setup_policy.read_text(), args.output_dir)
    print(f"Prepared base.toml, candidate.toml, delta.toml and {RULEPACK_PATH}; measurement pending.")


if __name__ == "__main__":
    main()
