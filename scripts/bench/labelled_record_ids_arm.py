#!/usr/bin/env python3
"""Prepare the labelled-record-ID ablation; main captures keep shipped policies.

Both mechanism arms use the candidate binary. The base keeps every old core
recognizer; the candidate appends exactly the three labelled-ID recognizers.
No model, detector, or benchmark is run by this preparer.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[2]
NEW_IDS = frozenset({"customer_id.labelled", "employee_id.labelled", "record_id.labelled"})
CORE = Path("crates/gaze-recognizers/embedded/core.toml")
DELTA = Path("scripts/bench/mechanisms/labelled-record-ids.toml")
RULEPACK_PATH = Path("target/bench-mechanisms/labelled-record-ids.toml")


def split_rulepack(source):
    header, *blocks = source.split("\n[[recognizers]]")
    parsed = tomllib.loads(source)
    ids = [row["id"] for row in parsed["recognizers"]]
    if len(ids) != len(set(ids)) or not NEW_IDS <= set(ids):
        raise ValueError("core must contain each labelled-ID recognizer exactly once")
    if parsed["rulepack_id"] != "gaze-core":
        raise ValueError("expected the embedded gaze-core rulepack")
    old, new = [], []
    for block in blocks:
        raw = "\n[[recognizers]]" + block
        spec = tomllib.loads(header + raw)["recognizers"][0]
        (new if spec["id"] in NEW_IDS else old).append(raw)
    # Locale declarations belong to base-core once; the feature adds only rules.
    new_header = "\n".join(key + " = " + json.dumps(parsed[key]) for key in
                           ("schema_version", "rulepack_version", "default_locales"))
    new_header += '\nrulepack_id = "gaze-labelled-record-ids"\n'
    base, candidate = header + "".join(old), new_header + "".join(new)
    rows = tomllib.loads(base)["recognizers"] + tomllib.loads(candidate)["recognizers"]
    if rows != parsed["recognizers"]:
        raise ValueError("core partition changed recognizer definitions or order")
    return base, candidate


def prepare(source, output):
    policy = tomllib.loads(source)
    bundles = policy["policy"]["rulepacks"]["bundled"]
    if bundles.count("core") != 1:
        raise ValueError("use a shipped policy with exactly one core bundle")
    delta_raw = (ROOT / DELTA).read_text()
    if tomllib.loads(delta_raw) != {"mechanism_delta": {
            "append_rulepack_paths": [RULEPACK_PATH.as_posix()]}}:
        raise ValueError("committed labelled-ID mechanism delta differs")
    core_raw = (ROOT / CORE).read_text()
    old, new = split_rulepack(core_raw)
    old_path = output.resolve() / "base-core.toml"
    new_path = ROOT / RULEPACK_PATH
    base = copy.deepcopy(policy)
    base["policy"]["rulepacks"]["bundled"].remove("core")
    paths = base["policy"]["rulepacks"].setdefault("paths", [])
    if str(old_path) in paths or RULEPACK_PATH.as_posix() in paths:
        raise ValueError("mechanism rulepack slots already present")
    paths.append(str(old_path))
    candidate = copy.deepcopy(base)
    candidate["policy"]["rulepacks"]["paths"].append(RULEPACK_PATH.as_posix())

    def rewrite(expected):
        section = re.search(r'(?ms)^\[policy\.rulepacks\][^\n]*\n(.*?)(?=^\[|\Z)', source)
        if section is None:
            raise ValueError("shipped policy has no rulepack section")
        replacement = '[policy.rulepacks]\n' + "\n".join(
            key + " = " + json.dumps(value)
            for key, value in expected["policy"]["rulepacks"].items()) + "\n\n"
        result = source[:section.start()] + replacement + source[section.end():]
        if tomllib.loads(result) != expected:
            raise ValueError("arm preparation changed unrelated policy settings")
        return result

    outputs = {old_path: old, new_path: new, output / "base.toml": rewrite(base),
               output / "candidate.toml": rewrite(candidate), output / "delta.toml": delta_raw}
    for path, raw in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(raw)
    def digest(raw):
        return hashlib.sha256(raw.encode()).hexdigest()
    receipt = {"schema_version": 1, "mechanism": "labelled-record-ids",
               "recognizer_ids": sorted(NEW_IDS), "source_core_sha256": digest(core_raw),
               "shipped_policy_sha256": digest(source),
               "outputs": {str(path): digest(raw) for path, raw in outputs.items()},
               "recognizers_equal_in_source_order": True,
               "unrelated_policy_settings_preserved": True,
               "measurement_pending": True}
    (output / "preparation.json").write_text(json.dumps(receipt, indent=2) + "\n")
    return base, candidate


def pair_equivalence(main_dir, mechanism_dir):
    """Cross-check candidate metrics; this never substitutes for a mechanism gate."""
    import agentic_layers as agentic
    import benchmark_spec as specs
    import subject_build

    receipt = {"schema_version": 1, "mechanism": "labelled-record-ids", "contracts": {}}
    for number in (2, 1, 4):
        paths = [directory / f"scorecard-v{number}.json" for directory in (main_dir, mechanism_dir)]
        cards = [json.loads(path.read_text()) for path in paths]
        specs.require_same(*cards)
        for card in cards:
            subject_build.validate(card)
            if card["scoring"]["scored_label_contract"]["version"] != number:
                raise ValueError("scorecard contract differs from filename")
        if cards[0]["subject_build"] != cards[1]["subject_build"]:
            raise ValueError("candidate producer builds differ")
        totals = [agentic.layer_totals(card, "policy-file") for card in cards]
        if totals[0] != totals[1]:
            raise ValueError(f"candidate layer metrics differ under v{number}")
        receipt["contracts"][str(number)] = {
            "same_layer_metrics": True, "layers": totals[0],
            "scorecards": [{"file": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                           for path in paths]}
    receipt["candidate"] = cards[0]["gaze"]["revision"]
    receipt["binary_sha256"] = cards[0]["subject_build"]["binary_sha256"]
    receipt["benchmark_spec_id"] = cards[0]["benchmark_spec"]["id"]
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prep = commands.add_parser("prepare")
    prep.add_argument("--setup-policy", type=Path, required=True)
    prep.add_argument("--output-dir", type=Path, required=True)
    compare = commands.add_parser("equivalence")
    compare.add_argument("--main-dir", type=Path, required=True)
    compare.add_argument("--mechanism-dir", type=Path, required=True)
    compare.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.setup_policy.read_text(), args.output_dir)
        print("Prepared labelled-record-ids mechanism policies; measurement pending.")
    else:
        receipt = pair_equivalence(args.main_dir, args.mechanism_dir)
        args.output.write_text(json.dumps(receipt, indent=2) + "\n")
        print("Main and mechanism candidate layer metrics match under v2, v1 and v4.")


if __name__ == "__main__":
    main()
