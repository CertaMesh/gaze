#!/usr/bin/env python3
"""Opt-in custom-policy arm for effective collision-family precedence.

Run `policy --input SETUP --output POLICY` on each runtime's own setup policy,
then `full --policy POLICY ...` with the canonical runner's arguments. The
ordinary C/A/D/R populations stay intact; additional A positives and D controls
exercise adopter metadata. This is not the shipped-default headline arm.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys
import tomllib
from unittest.mock import patch

import agentic_layers as agentic
import run_no_opf_benchmark as runner

ROOT = Path(__file__).resolve().parents[2]
DELTA = ROOT / "docs/reference/benchmarks/mechanisms/family-precedence-policy.toml"
SCHEMA_VERSION = 1
CASES = {
    # name: layer, alpha raw, second alpha raw, beta raw, alpha action, family action
    "false_tie_gold": ("A", 10, 5, 10, "tokenize", "preserve"),
    "missed_tie_gold": ("A", 10, 5, 5, "preserve", "tokenize"),
    "no_tie_gold": ("A", 10, 5, 20, "tokenize", "tokenize"),
    "false_tie_benign": ("D", 10, 5, 10, "preserve", "tokenize"),
    "no_tie_benign": ("D", 10, 5, 20, "preserve", "tokenize"),
    "same_variant": ("D", 10, 5, 10, "preserve", "tokenize"),
    "absent_id": ("D", 10, 5, 10, "preserve", "tokenize"),
}
SURFACES = {
    "dev": ("preview={V}", '{"preview_ref":"{V}"}'),
    "test": ("ref={V}", '{"reference":"{V}"}'),
}


def policy_delta() -> str:
    """A real, compiled but non-matching alpha recognizer lowers its variant."""
    detectors, rules = [], []
    for case, (_, a, a2, b, alpha_action, family_action) in CASES.items():
        family = "bench-effective-" + case.replace("_", "-")
        for suffix, precedence in (("alpha", a), ("calibration", a2), ("beta", b)):
            cls = "alpha" if suffix == "calibration" or case == "same_variant" else suffix
            pattern = (rf"\bzzcal-{case}-[0-9]{{6}}\b" if suffix == "calibration"
                       else rf"\bzzref-{case}-[0-9]{{6}}\b")
            block = (f'[[policy.custom_recognizers]]\nkind = "regex"\n'
                     f'name = "bench.effective.{case}.{suffix}"\n'
                     f"pattern = '{pattern}'\nclass = \"custom:bench_effective_{case}_{cls}\"\n")
            if not (case == "absent_id" and suffix == "beta"):
                variant = "alpha" if suffix == "calibration" or case == "same_variant" else suffix
                block += (f'[policy.custom_recognizers.collision]\nfamily = "{family}"\n'
                          f'variant = "{variant}"\nprecedence = {precedence}\n')
            detectors.append(block)
        for cls, action in ((f"custom:bench_effective_{case}_alpha", alpha_action),
                            (f"custom:bench_effective_{case}_beta", "preserve"),
                            (f"custom:family:{family}", family_action)):
            rules.append(f'[[rule]]\nkind = "class"\nclass = "{cls}"\naction = "{action}"\n')
    return "\n".join(detectors + rules)


def extend_policy(source: str, delta: str) -> str:
    base, added = tomllib.loads(source), tomllib.loads(delta)
    if delta != policy_delta():
        raise ValueError("family precedence policy delta differs from its committed generator")
    existing = base.get("policy", {}).get("custom_recognizers", [])
    names = {detector["name"] for detector in existing}
    if names & {detector["name"] for detector in added["policy"]["custom_recognizers"]}:
        raise ValueError("family precedence policy already extended")
    # Class rules precede the setup policy's first-match defaults. Detectors
    # append as absolute TOML table paths without reopening the policy table.
    marker = re.search(r"(?m)^\[\[rule\]\]\s*$", source)
    if marker is None:
        raise ValueError("setup policy has no rule array")
    split = delta.index("[[rule]]")
    result = source[:marker.start()] + delta[split:] + "\n" + source[marker.start():] + "\n" + delta[:split]
    expected = json.loads(json.dumps(base))
    expected.setdefault("policy", {}).setdefault("custom_recognizers", []).extend(added["policy"]["custom_recognizers"])
    expected["rule"] = added["rule"] + expected["rule"]
    if tomllib.loads(result) != expected:
        raise ValueError("policy extension changed settings outside the committed delta")
    return result


def generate(partition: str) -> list[agentic.Record]:
    if partition not in SURFACES:
        raise ValueError(f"unknown partition {partition}")
    records = []
    for case, (layer, *_metadata) in CASES.items():
        rng = agentic.Rng(agentic.PARTITION_SEEDS[partition], f"effective-family/{case}")
        for index in range(agentic.DOCS_PER_FAMILY):
            # Zero-led invented references, not assignable personal identifiers.
            value = f"zzref-{case}-0{rng.digits(5)}"
            for surface, template in enumerate(SURFACES[partition]):
                text, gold, decoys = agentic._fill_with_decoys(
                    template, {"V": (value, "IDCARDNUM" if layer == "A" else None)}
                )
                records.append(agentic.Record(
                    uid=f"effective-family-{partition}-{case}-{index:03d}-{surface}",
                    partition=partition, layer=layer, family=f"effective_family_{case}",
                    surface=f"effective_family_{surface}", validity=agentic.UNCHECKED if layer == "A" else agentic.BENIGN,
                    group=f"effective-family-{partition}-{case}-{index:03d}",
                    template=f"effective-family/{partition}/{surface}", language="en", region="US",
                    text=text, gold=gold, decoys=decoys,
                ))
    return records


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv and argv[0] == "policy":
        parser = argparse.ArgumentParser(description="Add the committed custom-policy delta to a runtime's setup policy")
        parser.add_argument("command")
        parser.add_argument("--input", type=Path, required=True)
        parser.add_argument("--output", type=Path, required=True)
        args = parser.parse_args(argv)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(extend_policy(args.input.read_text(), DELTA.read_text()))
        return 0
    layers_only = bool(argv and argv[0] == "measure")
    if layers_only:
        parser = argparse.ArgumentParser(add_help=False)
        parser.add_argument("--policy", type=Path, required=True)
        args, _ = parser.parse_known_args(argv[1:])
    else:
        args = runner.parse_args(argv)
    if args.policy is None or (not layers_only and args.no_agentic_layers):
        raise ValueError("family precedence arm requires its extended setup policy and generated layers")
    policy = tomllib.loads(args.policy.read_text())
    delta = tomllib.loads(DELTA.read_text())
    custom = policy.get("policy", {}).get("custom_recognizers", [])
    if (not all(item in custom for item in delta["policy"]["custom_recognizers"])
            or policy.get("rule", [])[:len(delta["rule"])] != delta["rule"]):
        raise ValueError("family precedence arm requires the exact committed policy delta")
    ordinary_generate, ordinary_manifest = agentic.generate, agentic.manifest

    def extended_generate(partition):
        return ordinary_generate(partition) + generate(partition)

    def extended_manifest(partition, records):
        manifest = ordinary_manifest(partition, records)
        manifest["extension"] = {
            "name": "effective-family-precedence", "schema_version": SCHEMA_VERSION,
            "generator_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "policy_delta_sha256": hashlib.sha256(DELTA.read_bytes()).hexdigest(),
            "documents": len(generate(partition)),
        }
        return manifest

    # The existing runner, transport, contracts, scorer and gate do all work.
    # The patched generator also supplies original gold to validator probing
    # and the observation writer; no alternate scoring path is introduced.
    with patch.object(agentic, "generate", extended_generate), patch.object(agentic, "manifest", extended_manifest):
        if layers_only:
            # Reuse the current full runner's pinned release validator probe;
            # the measured detector binary is still the supplied tagged one.
            with patch.object(agentic.score, "validator_probe_binary",
                              lambda _root: ROOT / agentic.score.VALIDATOR_PROBE_TARGET / "release/validator-recall-probe"):
                return agentic.main(argv)
        return runner.run(args)


if __name__ == "__main__":
    raise SystemExit(main())
