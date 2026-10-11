#!/usr/bin/env python3
"""Capture and replay the new custom-policy cells on displayed release tags.

Build each tag's CLI and clean_for_bench with rustc 1.96 and --locked in its
own target directory. v0.14.0 needs the committed transport-only policy bridge:
`git apply scripts/bench/family-precedence-v014-policy-bridge.patch`. Measure
requires a build receipt pinning the tag, Cargo.lock, CLI, example source and
binary. It runs that CLI's own setup policy plus the arm's exact policy delta.
The existing canonical scorer and observation-record replay own every metric.
Only the added arm cells are captured here; the default headline is unchanged.
Callers serialize measurement through their benchmark-machine lease.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import tempfile
import tomllib
from pathlib import Path
import subprocess
import shutil
from unittest.mock import patch

import agentic_layers as agentic
import family_precedence_arm as arm
import gaze_bench_score as score
import scorecard_record as records
from tagged_gaze import tag_commit

ROOT = Path(__file__).resolve().parents[2]
BRIDGE = ROOT / "scripts/bench/family-precedence-v014-policy-bridge.patch"
RELEASES = ("v0.14.0", "v0.15.0", "v0.15.1", "v0.16.0")
HISTORY_ROOT = ROOT / "docs/reference/benchmarks/mechanisms/family-precedence-history"


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def metric_totals(run: dict) -> dict:
    utf8 = run["metrics"]["utf8_bytes"]
    contract = run["pipeline_contract"]
    available = run["pipeline_availability"]
    return {
        "leaked": utf8["leaked"], "false_positive": utf8["false_positive"],
        "refused": available["failed_closed_documents"],
        "attempted": available["attempted_documents"],
        "leaked_on_all_processed": utf8["leaked"],
        "exact_restores": contract["restore_exact_documents"],
        "restore_failures": contract["documents"] - contract["restore_exact_documents"],
        "valid_manifests": contract["manifest_valid_documents"],
        "invalid_manifests": contract["documents"] - contract["manifest_valid_documents"],
        "cells": {key: block["utf8_bytes"] for key, block in run["per_cell"].items()},
    }


def contracts() -> dict:
    return {1: score.SCORED_LABEL_CONTRACT_V1, **{
        version: score.load_scored_label_contract(ROOT / f"docs/reference/benchmarks/scored-labels-v{version}.json")
        for version in (2, 3, 4)
    }}


def expected_tag_source(release: str, relative: str) -> bytes:
    return subprocess.check_output(["git", "show", f"refs/tags/{release}:{relative}"], cwd=ROOT)


def expected_example_sha(release: str) -> str:
    relative = "crates/gaze-recognizers/examples/clean_for_bench.rs"
    source = expected_tag_source(release, relative)
    if release != "v0.14.0":
        return hashlib.sha256(source).hexdigest()
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        example = root / relative
        example.parent.mkdir(parents=True)
        example.write_bytes(source)
        subprocess.run(["git", "apply", str(BRIDGE)], cwd=root, check=True)
        return sha(example)


def validate_receipt(receipt: dict, release: str) -> None:
    if (release not in RELEASES or receipt.get("release") != release
            or receipt.get("revision") != tag_commit(release, ROOT)
            or receipt.get("fresh_locked_build") is not True
            or not receipt.get("toolchain", "").startswith("rustc 1.96.0 ")
            or receipt.get("cargo_lock_sha256") != hashlib.sha256(expected_tag_source(release, "Cargo.lock")).hexdigest()
            or receipt.get("example_source_sha256") != expected_example_sha(release)):
        raise ValueError("release build receipt differs from its tag or committed adapter")
    for key in ("binary_sha256", "cli_sha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(receipt.get(key, ""))):
            raise ValueError("release binary receipt has no digest")
    expected_changes = ["crates/gaze-recognizers/examples/clean_for_bench.rs"] if release == "v0.14.0" else []
    if receipt.get("changed_paths") != expected_changes:
        raise ValueError("release receipt declares detection source changes")
    if release == "v0.14.0" and receipt.get("adapter_patch_sha256") != sha(BRIDGE):
        raise ValueError("release adapter receipt differs from the committed bridge")


def validate_build(receipt: dict, tree: Path, release: str) -> None:
    validate_receipt(receipt, release)
    expected = tag_commit(release, ROOT)
    if (release not in RELEASES or receipt.get("release") != release
            or receipt.get("revision") != expected
            or score.git_metadata(tree)["revision"] != expected
            or receipt.get("fresh_locked_build") is not True
            or not receipt.get("toolchain", "").startswith("rustc 1.96.0 ")):
        raise ValueError("release build receipt is not bound to the requested tag and toolchain")
    pins = {
        "cargo_lock_sha256": tree / "Cargo.lock",
        "example_source_sha256": tree / "crates/gaze-recognizers/examples/clean_for_bench.rs",
        "binary_sha256": tree / "target/release/examples/clean_for_bench",
        "cli_sha256": tree / "target/release/gaze",
    }
    if any(receipt.get(key) != sha(path) for key, path in pins.items()):
        raise ValueError("release build input or binary differs from its receipt")
    expected_changes = ["crates/gaze-recognizers/examples/clean_for_bench.rs"] if release == "v0.14.0" else []
    changed = subprocess.check_output(["git", "diff", "HEAD", "--name-only"], cwd=tree, text=True).splitlines()
    if changed != expected_changes or receipt.get("changed_paths") != expected_changes:
        raise ValueError("release detection sources changed")
    if release == "v0.14.0":
        if receipt.get("adapter_patch_sha256") != sha(BRIDGE):
            raise ValueError("historical adapter is not the committed transport bridge")
        # Reverse application proves every changed byte belongs to this patch.
        subprocess.run(["git", "apply", "--reverse", "--check", str(BRIDGE)], cwd=tree, check=True)


def build(release: str, tree: Path, receipt_file: Path) -> dict:
    if score.git_metadata(tree)["revision"] != tag_commit(release, ROOT):
        raise ValueError("build worktree does not contain the requested release tag")
    relative = "crates/gaze-recognizers/examples/clean_for_bench.rs"
    if release == "v0.14.0" and sha(tree / relative) != expected_example_sha(release):
        subprocess.run(["git", "apply", "--check", str(BRIDGE)], cwd=tree, check=True)
        subprocess.run(["git", "apply", str(BRIDGE)], cwd=tree, check=True)
    changed = subprocess.check_output(["git", "diff", "HEAD", "--name-only"], cwd=tree, text=True).splitlines()
    if changed != ([relative] if release == "v0.14.0" else []):
        raise ValueError("build worktree has changes beyond the historical transport bridge")
    if sha(tree / relative) != expected_example_sha(release):
        raise ValueError("build adapter differs from committed release source")
    rustc = Path(subprocess.check_output(["rustup", "which", "--toolchain", "1.96.0", "rustc"], text=True).strip())
    env = {**os.environ, "RUSTC": str(rustc), "RUSTDOC": str(rustc.with_name("rustdoc")),
           "PATH": str(rustc.parent) + os.pathsep + os.environ["PATH"],
           "CARGO_TARGET_DIR": str(tree / "target"), "CARGO_BUILD_JOBS": "4"}
    feature = "safety-net-kiji" if release == "v0.14.0" else "safety-net-nym"
    for package in ("gaze-cli", "gaze-recognizers"):
        if shutil.disk_usage(tree).free < 42 * 1024 ** 3:
            raise ValueError("build disk floor is below 42 GiB")
        command = [str(rustc.with_name("cargo")), "build", "--locked", "--release", "-p", package]
        if package == "gaze-recognizers":
            command += ["--example", "clean_for_bench", "--features", feature]
        subprocess.run(command, cwd=tree, env=env, check=True)
    receipt = {
        "release": release, "revision": tag_commit(release, ROOT), "fresh_locked_build": True,
        "toolchain": subprocess.check_output([str(rustc), "--version"], text=True).strip(),
        "cargo_lock_sha256": sha(tree / "Cargo.lock"), "example_source_sha256": sha(tree / relative),
        "binary_sha256": sha(tree / "target/release/examples/clean_for_bench"),
        "cli_sha256": sha(tree / "target/release/gaze"), "changed_paths": changed,
        "adapter_patch_sha256": sha(BRIDGE) if release == "v0.14.0" else None,
    }
    validate_build(receipt, tree, release)
    receipt_file.parent.mkdir(parents=True, exist_ok=True)
    receipt_file.write_text(json.dumps(receipt, indent=2) + "\n")
    return receipt


def capture(release: str, tree: Path, receipt_file: Path, probe: Path, output: Path) -> dict:
    receipt = json.loads(receipt_file.read_text())
    validate_build(receipt, tree, release)
    output.mkdir(parents=True, exist_ok=True)
    setup = tree / "target/family-precedence/setup.toml"
    policy = setup.with_name("policy.toml")
    with (output / "setup.log").open("w") as log:
        subprocess.run([str(tree / "target/release/gaze"), "setup", "--non-interactive", "--force", "--policy-out", str(setup)],
                       cwd=tree, stdout=log, stderr=subprocess.STDOUT, check=True)
    policy.write_text(arm.extend_policy(setup.read_text(), arm.DELTA.read_text()))
    policy_data = tomllib.loads(policy.read_text())
    model = Path(policy_data["ner"]["model_dir"]).expanduser().resolve()
    raw_records = arm.generate("test")
    corpus = hashlib.sha256(agentic.corpus_bytes(raw_records)).hexdigest()
    result = {
        "schema_version": 1, "release": release, "revision": receipt["revision"],
        "scope": "added effective-family-precedence arm only; not default headline",
        "build": receipt, "generator_sha256": sha(Path(arm.__file__)),
        "history_harness_sha256": sha(Path(__file__)), "corpus_sha256": corpus,
        "policy_delta_sha256": sha(arm.DELTA), "policy_sha256": sha(policy),
        "setup_policy_home_normalized_sha256": hashlib.sha256(setup.read_text().replace(str(Path.home()), "~").encode()).hexdigest(),
        "validator_probe_sha256": sha(probe), "layers": {},
    }
    for layer in ("A", "D"):
        selected = [row for row in raw_records if row.layer == layer]
        documents = [row.to_document() for row in selected]
        measurements = score.collect_validator_measurements(probe, documents, [doc.uid for doc in documents])
        writer = records.RecordWriter(documents, measurements, corpus_sha256=corpus)
        with patch.object(score, "COMMITTED_SOURCE_ID_VOCABULARY", score.load_committed_source_id_vocabulary(tree)):
            run = score.run_config(
                repo_root=ROOT, binary=tree / "target/release/examples/clean_for_bench", config="policy-file",
                documents=documents, model_dir=model, opf_command=None, opf_checkpoint=None, opf_daemon_socket=None,
                threshold=policy_data["ner"]["threshold"], diagnostics_dir=output / f"logs-{layer}",
                base_environment=dict(os.environ), warmup_count=0, validator_measurements=measurements,
                policy_path=policy, replacing_actions=(score.PRE_REDACT_MANIFEST_ACTIONS if release == "v0.14.0" else score.MANIFEST_REPLACING_ACTIONS),
                split_composite_source_ids=release == "v0.14.0",
                record_document=lambda config, document, response, validators: writer.add("C", config, document, response, validators),
            )
        population = score.population_summary(documents)
        card = score.assemble_scorecard(
            repo_root=tree, dataset_metadata={"repository": "gaze-effective-family-precedence", "revision": corpus},
            dataset_report={"integrity": {"sha256": corpus}},
            sampling_report={"available_population": population, "evaluated_population": population},
            parameters={"sampling_seed": agentic.PARTITION_SEEDS["test"], "policy_sha256": sha(policy), "arm_layer": layer},
            runs=[run], scored_label_contract=score.scored_label_contract_report(score.SCORED_LABEL_CONTRACT_V1, documents),
        )
        reference = writer.write(output / f"{layer}-observations.jsonl.gz", card, add_reference=True)
        result["layers"][layer] = {"record": reference, "contracts": {}}
        for version, contract in contracts().items():
            replay = records.rescore(output / reference["file"], contract, max_workers=1)
            result["layers"][layer]["contracts"][str(version)] = metric_totals(replay["runs"][0])
    (output / "capture.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def check(directory: Path) -> dict:
    capture = json.loads((directory / "capture.json").read_text())
    if capture.get("release") not in RELEASES or capture.get("revision") != tag_commit(capture["release"], ROOT):
        raise ValueError("history capture does not name a displayed release tag")
    validate_receipt(capture.get("build", {}), capture["release"])
    expected_corpus = hashlib.sha256(agentic.corpus_bytes(arm.generate("test"))).hexdigest()
    if (capture.get("corpus_sha256") != expected_corpus or capture.get("generator_sha256") != sha(Path(arm.__file__))
            or capture.get("history_harness_sha256") != sha(Path(__file__)) or capture.get("policy_delta_sha256") != sha(arm.DELTA)):
        raise ValueError("history inputs differ from committed arm")
    if set(capture.get("layers", {})) != {"A", "D"}:
        raise ValueError("history capture omits positives or benign controls")
    for layer, block in capture["layers"].items():
        path = directory / block["record"]["file"]
        if sha(path) != block["record"]["sha256"]:
            raise ValueError("history observations differ from their pin")
        header, observations = records._read(path)
        expected_documents = {row.uid: records._document_row(row.to_document())
                              for row in arm.generate("test") if row.layer == layer}
        if (header.get("corpus_sha256") != expected_corpus
                or header.get("scorecard", {}).get("gaze", {}).get("revision") != capture["revision"]
                or len(observations) != len(expected_documents)
                or block["record"].get("observations") != len(expected_documents)
                or len(header.get("documents", [])) != len(expected_documents)):
            raise ValueError("history observation population differs from committed cells")
        actual_documents = {row["id"]: {key: row[key] for key in records._document_row(arm.generate("test")[0].to_document())}
                            for row in header["documents"]}
        if actual_documents != expected_documents:
            raise ValueError("history document IDs or gold bounds differ from committed cells")
        for version, contract in contracts().items():
            replay = records.rescore(path, contract, max_workers=1)
            if metric_totals(replay["runs"][0]) != block["contracts"][str(version)]:
                raise ValueError("history metrics differ from canonical record replay")
    return capture


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    producer = commands.add_parser("build")
    producer.add_argument("--release", choices=RELEASES, required=True)
    producer.add_argument("--worktree", type=Path, required=True)
    producer.add_argument("--build-receipt", type=Path, required=True)
    measure = commands.add_parser("measure")
    measure.add_argument("--release", choices=RELEASES, required=True)
    measure.add_argument("--worktree", type=Path, required=True)
    measure.add_argument("--build-receipt", type=Path, required=True)
    measure.add_argument("--validator-probe", type=Path, required=True)
    measure.add_argument("--output", type=Path, required=True)
    replay = commands.add_parser("check")
    replay.add_argument("directory", type=Path, nargs="?", default=HISTORY_ROOT)
    args = parser.parse_args()
    if args.command == "build":
        build(args.release, args.worktree.resolve(), args.build_receipt)
    elif args.command == "measure":
        capture(args.release, args.worktree.resolve(), args.build_receipt, args.validator_probe.resolve(), args.output)
    elif (args.directory / "capture.json").is_file():
        check(args.directory)
    else:
        if {path.name for path in args.directory.iterdir() if path.is_dir()} != set(RELEASES):
            raise ValueError("history does not cover every displayed release")
        for release in RELEASES:
            check(args.directory / release)
    print("effective-family-precedence capture matches canonical scoring")


if __name__ == "__main__":
    main()
