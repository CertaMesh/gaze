#!/usr/bin/env python3
"""Compose admitted text and native MCP measurements into the v2 and v1 gain gate.

Original text scorecards and their execution receipts remain unchanged. Derived
scorecards retain both components and add MCP operations to A, D and R. The gate
admits the original captures before composing counts and rejects regressions in
either component, so an improvement cannot hide a new leak in the other.
"""
from __future__ import annotations
import argparse
from collections import Counter
import copy
import hashlib
import gzip
import json
from pathlib import Path
import tomllib

import agentic_layers as layers
import gaze_bench_score as score
import mcp_operation_order as mcp
import mcp_text_path_identity as path_identity


def _bind(core, native, contract):
    identity = score.scorecard_scored_label_contract_identity(core)
    expected_id = "scored-labels-" + contract
    expected_version = int(contract[1:])
    if identity[:2] != (expected_id, expected_version):
        raise ValueError("text scorecard uses the wrong scored-label contract")
    if contract == "v2" and identity[2] != native["contracts"]["v2"]:
        raise ValueError("text and MCP scored-label digests differ")
    if core["gaze"] != {"revision": native["runtime_revision"], "dirty": False}:
        raise ValueError("text and MCP captures do not measure the same clean runtime revision")
    document = native["policy_document"]
    policy_sha = hashlib.sha256(document.encode("utf-8")).hexdigest()
    if policy_sha != native["policy_sha256"] or core["parameters"]["policy_sha256"] != policy_sha:
        raise ValueError("text and MCP captures do not use the same setup policy")
    policy = tomllib.loads(document)
    if core["parameters"]["ner_threshold"] != policy["ner"]["threshold"]:
        raise ValueError("text and MCP effective NER thresholds differ")
    models = layers.model_bundle_identity(core["runner_provenance"]["model_bundles"])
    for environment, model in (("GAZE_NER_MODEL_DIR", "davlan-mbert-ner-hrl-onnx"),
                               ("GAZE_NYM_MODEL_DIR", "nym-small-int8")):
        if models.get(model) != native["model_files"][environment]["SHA256SUMS"]:
            raise ValueError("text and MCP captures use different model bundles")


def _compose(core_totals, native, contract):
    """Pure composition after both input artifacts and their bindings were admitted."""
    totals = copy.deepcopy(core_totals)
    attempted = Counter(row["layer"] for row in native["scores"][contract]["cases"])
    for layer, row in native["scores"][contract]["layers"].items():
        target = totals[layer]
        target["attempted"] += attempted[layer]
        target["documents"] += attempted[layer] - row["refused"]
        target["headline_leaked"] += row["leaked"]
        target["leaked"] += row["leaked"]
        target["false_positive"] += row["false_positive"]
        target["failed_closed"] += row["refused"]
        target["restore_exact"] += row["restore_exact"]
        target["manifest_valid"] += row["manifest_valid"]
        # Name and Email gold have no checksum-invalid credit or guarded D family.
    return totals


def _core_regressions(before, after):
    failures = []
    for layer in before:
        a, b = before[layer], after[layer]
        if a["attempted"] != b["attempted"]:
            failures.append(layer + ":attempted")
        for metric in ("headline_leaked", "leaked", "failed_closed"):
            if b[metric] > a[metric]:
                failures.append(layer + ":" + metric)
        for metric in ("restore_exact", "manifest_valid"):
            if b[metric] < a[metric] or b["documents"] - b[metric] > a["documents"] - a[metric]:
                failures.append(layer + ":" + metric)
    failures.extend("D:credit-guard:" + family for family in layers.credit_guard_rise(before, after))
    return failures


def compare(core_cards, core_paths, native_cards, native_paths):
    """Artifact admission, immutable component binding, then the canonical count rule."""
    native_result = mcp.compare(*native_cards)
    result = {"schema_version": 1, "kind": "mcp-operation-composed-gain-gate", "contracts": {}, "scorecards": {}}
    for contract in ("v2", "v1"):
        a, b = core_cards[contract]
        paths = core_paths[contract]
        admitted_core = layers.gate(a, b, scorecard_paths=paths)
        if admitted_core["verdict"] == "not_comparable":
            raise ValueError("text component is not comparable: " + contract)
        for core, native in zip((a, b), native_cards, strict=True):
            _bind(core, native, contract)
        config = admitted_core["config"]
        text = [layers.layer_totals(card, config) for card in (a, b)]
        combined = [_compose(totals, native, contract) for totals, native in zip(text, native_cards, strict=True)]
        decision = layers.decide(*combined)
        regressions = _core_regressions(*text) + native_result[contract]["regressions"]
        if regressions:
            decision = {**decision, "verdict": "fail", "reason": "component regression: " + ", ".join(regressions)}
        result["contracts"][contract] = {**decision, "component_regressions": regressions,
            "core_component": admitted_core, "mcp_component": native_result[contract]}
        cards = []
        for side, core, path, native, native_path, totals, text_totals in zip(
                ("base", "candidate"), (a, b), paths, native_cards, native_paths, combined, text, strict=True):
            cards.append({"schema_version": 1, "kind": "mcp-operation-composed-scorecard", "contract": contract,
                "side": side, "runtime": core["gaze"], "policy_sha256": core["parameters"]["policy_sha256"],
                "inputs": {"text_scorecard": {"path": str(path.resolve()), "sha256": mcp.digest(path)},
                           "native_capture": {"path": str(native_path.resolve()), "sha256": mcp.digest(native_path)}},
                "mcp_specification": native["specification"], "gate_totals": totals,
                "components": {"text": text_totals, "mcp_operation": native["scores"][contract]},
                "composition_source_sha256": mcp.digest(Path(__file__))})
        result["scorecards"][contract] = cards
    result["passed"] = all(row["verdict"] == "pass" for row in result["contracts"].values())
    return result


def compare_unchanged_text(record, native_cards, native_paths, identity_path, publication_revision="HEAD"):
    """The explicit reuse ruling: prove source identity, never invent text counts."""
    if (record["base_revision"], record["candidate_revision"]) != tuple(card["runtime_revision"] for card in native_cards):
        raise ValueError("text identity does not bind both measured native revisions")
    path_identity.verify(record)
    publication = path_identity.verify(record, publication_revision)
    measured = path_identity._tree(native_cards[1]["runtime_revision"])
    published = path_identity._tree(publication["candidate_revision"])
    runtime_paths = [path for path in measured.keys() | published.keys() if path.startswith("crates/") or path in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"} or path.startswith(".cargo/")]
    if any(measured.get(path) != published.get(path) for path in runtime_paths):
        raise ValueError("published runtime differs from the measured MCP runtime")
    native_result = mcp.compare(*native_cards)
    result = {"schema_version": 1, "kind": "mcp-operation-gain-gate-with-unchanged-text", "fresh_text_capture": False,
              "text_layers": record["text_layers"], "path_identity_sha256": mcp.digest(identity_path),
              "protected_file_count": record["protected_file_count"], "publication_revision": publication["candidate_revision"],
              "reuse_authority": "Explicit orchestrator ruling: same source-identity reuse rule as PR #869", "contracts": {}, "scorecards": {}}
    for contract in ("v2", "v1"):
        result["contracts"][contract] = {"verdict": "pass" if native_result[contract]["pass"] else "fail", "native": native_result[contract]}
        result["scorecards"][contract] = [{"schema_version": 1, "kind": "mcp-operation-scorecard-with-unchanged-text",
            "side": side, "contract": contract, "runtime_revision": card["runtime_revision"], "fresh_text_capture": False,
            "text_layers": record["text_layers"], "path_identity_sha256": mcp.digest(identity_path),
            "native_capture_sha256": mcp.digest(path), "native_operation_layers": card["scores"][contract]["layers"],
            "d_counterweights": card["scores"][contract]["d_counterweights"]}
            for side, card, path in zip(("base", "candidate"), native_cards, native_paths, strict=True)]
    result["passed"] = all(row["verdict"] == "pass" for row in result["contracts"].values())
    return result


def read_json(path):
    raw = path.read_bytes()
    return json.loads(gzip.decompress(raw) if path.suffix == ".gz" else raw)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for contract in ("v1", "v2"):
        parser.add_argument("--base-" + contract, type=Path)
        parser.add_argument("--candidate-" + contract, type=Path)
    parser.add_argument("--base-native", type=Path, required=True)
    parser.add_argument("--candidate-native", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--text-path-identity", type=Path)
    parser.add_argument("--publication-revision", default="HEAD")
    args = parser.parse_args()
    native_paths = (args.base_native, args.candidate_native)
    native = tuple(read_json(path) for path in native_paths)
    if args.text_path_identity:
        report = compare_unchanged_text(read_json(args.text_path_identity), native, native_paths, args.text_path_identity, args.publication_revision)
    else:
        paths = {contract: (getattr(args, "base_" + contract), getattr(args, "candidate_" + contract)) for contract in ("v1", "v2")}
        if any(path is None for pair in paths.values() for path in pair):
            parser.error("supply both contracts' text captures, or an authorized text-path identity record")
        cards = {contract: tuple(read_json(path) for path in pair) for contract, pair in paths.items()}
        report = compare(cards, paths, native, native_paths)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for contract, pair in report.pop("scorecards").items():
        for card in pair:
            (args.output_dir / (card["side"] + "-" + contract + ".scorecard.json")).write_text(json.dumps(card, indent=2) + "\n")
    (args.output_dir / "gate.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"passed": report["passed"], "contracts": {key: value["verdict"] for key, value in report["contracts"].items()}}))
    raise SystemExit(0 if report["passed"] else 1)
