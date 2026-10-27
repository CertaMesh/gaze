#!/usr/bin/env python3
"""Capture and compare synthetic MCP operations through each revision's real dispatch.

This arm supplements C/A/D/R; it never substitutes flattened text for dispatch.
Load-coordinator approval is required before capture. Inputs and the independent
byte oracle are frozen by hash. Run base first with its own gaze setup policy.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = Path(__file__).with_name("fixtures") / "mcp_operation_order.json"
TOKEN = re.compile(r"<[0-9a-f]{8}:[^<>]+>")
FIELDS = ("leaked", "false_positive", "refused", "dispatch_failed", "restore_exact", "restore_failed", "manifest_valid", "manifest_invalid")


def digest(path):
    result = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def identity():
    folder = Path(__file__).parent
    files = [FIXTURE, Path(__file__), folder / "build_native_mcp_operation.py", folder / "native-mcp-operation/main.rs"]
    return {str(path.relative_to(ROOT)): digest(path) for path in files}


def fixture():
    data = json.loads(FIXTURE.read_text())
    if data["schema_version"] != 1 or data["contract"] != "mcp-operation-order-v5":
        raise ValueError("unsupported MCP operation fixture")
    rows = data["cases"]
    if len({row["id"] for row in rows}) != len(rows) or {row["layer"] for row in rows} != {"A", "D", "R"}:
        raise ValueError("duplicate fixture ids or missing measurement layer")
    return rows


def canonical_class(value):
    if isinstance(value, str) and value in {"Name", "Email", "Organization", "Location"}:
        return value.lower()
    if isinstance(value, dict) and set(value) == {"Custom"} and isinstance(value["Custom"], str) and value["Custom"]:
        return "custom:" + value["Custom"].lower()
    raise ValueError("invalid manifest class")


def entries_from_snapshot(encoded):
    """Verify the actual signed owner manifest, independently of the Rust reporter."""
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    from cryptography.exceptions import InvalidSignature
    blob = bytes.fromhex(encoded)
    if len(blob) < 97 or blob[0] not in (6, 7):
        raise ValueError("unsupported or incomplete signed manifest")
    try:
        Ed25519PublicKey.from_public_bytes(blob[1:33]).verify(blob[33:97], blob[:33] + blob[97:])
    except InvalidSignature as error:
        raise ValueError("invalid signed manifest") from error
    payload = json.loads(blob[97:])
    prefix = payload["session_hex"]
    if not re.fullmatch(r"[0-9a-f]{8}", prefix):
        raise ValueError("invalid manifest namespace")
    entries = {}
    for entry in payload["entries"]:
        token = entry["token"]
        if token in entries or not TOKEN.fullmatch(token) or not token.startswith("<" + prefix + ":"):
            raise ValueError("duplicate or foreign manifest token")
        if not isinstance(entry["raw"], str) or not isinstance(entry["family"], str):
            raise ValueError("invalid manifest entry")
        canonical_class(entry["class"])
        if entry.get("spelling", "plain") != "plain":
            raise ValueError("unsupported manifest source spelling")
        entries[token] = entry
    return entries


def leaf_mask(original, protected, entries):
    """Exact literal/token reconstruction; one owner class or None per original UTF-8 byte."""
    parts, owners, cursor = [], [], 0
    for match in TOKEN.finditer(protected):
        if match.group() not in entries:
            # Other namespaces are literal bytes, never credited token coverage.
            continue
        gap = protected[cursor:match.start()]
        entry = entries[match.group()]
        parts.extend((gap, entry["raw"]))
        owners.extend([None] * len(gap.encode()))
        owners.extend([canonical_class(entry["class"])] * len(entry["raw"].encode()))
        cursor = match.end()
    tail = protected[cursor:]
    parts.append(tail)
    owners.extend([None] * len(tail.encode()))
    if "".join(parts) != original:
        raise ValueError("protected leaf does not restore to the authoritative original")
    return owners


def masks(original, protected, entries, path=()):
    if type(original) is not type(protected):
        raise ValueError("JSON leaf type changed")
    if isinstance(original, str):
        return {path: leaf_mask(original, protected, entries)}
    if isinstance(original, list):
        if len(original) != len(protected):
            raise ValueError("JSON array shape changed")
        result = {}
        for index, (a, b) in enumerate(zip(original, protected)):
            result.update(masks(a, b, entries, path + (index,)))
        return result
    if isinstance(original, dict):
        if original.keys() != protected.keys():
            raise ValueError("JSON object keys changed")
        result = {}
        for key in original:
            result.update(masks(original[key], protected[key], entries, path + (key,)))
        return result
    if original != protected:
        raise ValueError("non-text sibling changed")
    return {}


def verify_owned_identity(original, protected, entries, seed_names):
    if isinstance(original, str):
        for index, name in enumerate(seed_names):
            count = original.count("{{owned_" + str(index) + "}}")
            if not count:
                continue
            tokens = [token for token, entry in entries.items() if entry["class"] == "Name" and entry["raw"] == name]
            if len(tokens) != 1 or protected.count(tokens[0]) < count:
                raise ValueError("caller-owned token identity changed")
    elif isinstance(original, list):
        for left, right in zip(original, protected):
            verify_owned_identity(left, right, entries, seed_names)
    elif isinstance(original, dict):
        for key in original:
            verify_owned_identity(original[key], protected[key], entries, seed_names)


def ulid_bytes(text):
    alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"
    if len(text) != 26:
        raise ValueError("invalid manifest call id")
    value = 0
    for char in text:
        value = value * 32 + alphabet.index(char)
    return value.to_bytes(16, "big")


def validate_audit(record, argument_released_before_failure=False):
    audit = record["audit"]
    if argument_released_before_failure:
        if (audit["begin"], audit["finish"], audit["fail"]) != (1, 0, 1) or audit["args"] != record["tool_args"] or audit["snapshot"] is not None or record["response"] is not None:
            raise ValueError("incomplete failed manifest call after argument release")
        return
    if (audit["begin"], audit["finish"], audit["fail"]) != (1, 1, 0) or audit["args"] != record["tool_args"]:
        raise ValueError("incomplete or inconsistent manifest call")
    wire = record["response_wire"].encode()
    if json.loads(wire) != record["response"]:
        raise ValueError("response wire differs from released value")
    snapshot = audit["snapshot"]
    preimage = audit["call"].encode() + b"\0" + ulid_bytes(audit["call"]) + b"\0" + wire
    if snapshot != {"locator": "inline-sha256:" + audit["call"], "sha256_hex": hashlib.sha256(preimage).hexdigest(), "byte_len": len(wire)}:
        raise ValueError("response snapshot reference does not match released bytes")


def contracts():
    path = ROOT / "docs/reference/benchmarks/scored-labels-v2.json"
    v2 = json.loads(path.read_text())
    required = {"FIRSTNAME", "SURNAME", "EMAIL"}
    if not required <= {row["label"] for row in v2["labels"] if row["scored"]}:
        raise ValueError("MCP fixture gold is outside the v2 contract")
    return {
        "v1": {"identity": "scored-labels-v1", "neutral": set()},
        "v2": {"identity": digest(path), "neutral": {row["class"].lower() for row in v2["neutral_prediction_classes"]}},
    }


def score_case(case, record, neutral=()):
    result = dict.fromkeys(FIELDS, 0)
    if case["id"] != record["id"]:
        raise ValueError("capture order or fixture identity mismatch")
    failed = record["error"] is not None
    result["dispatch_failed"] = int(failed)
    argument_released = record["phase"] == "target" and case["direction"] == "arguments" and record.get("tool_args") is not None
    if record["phase"] != "target" or (failed and not argument_released):
        result["refused"] = 1
        return result
    try:
        entries = entries_from_snapshot(record["snapshot_hex"])
        validate_audit(record, failed and argument_released)
    except (ValueError, KeyError, TypeError):
        result["manifest_invalid"] = 1
        return result
    result["manifest_valid"] = 1
    protected = record["tool_args"] if case["direction"] == "arguments" else record["response"]
    try:
        coverage = masks(case["expanded"], protected, entries)
        masks(case["expanded"], record["restored"], {})
        verify_owned_identity(case.get("payload", case["expanded"]), protected, entries, case.get("seed_names", []))
    except (ValueError, KeyError, TypeError):
        result["restore_failed"] = 1
        return result
    result["restore_exact"] = 1
    gold = {path: [False] * len(mask) for path, mask in coverage.items()}
    for span in case["gold"]:
        path, (start, end) = tuple(span["path"]), span["span"]
        if span["class"] not in ("Name", "Email") or path not in gold or not 0 <= start < end <= len(gold[path]):
            raise ValueError("invalid gold source extent")
        gold[path][start:end] = [True] * (end - start)
    for path, mask in coverage.items():
        result["leaked"] += sum(private and owner is None for private, owner in zip(gold[path], mask))
        result["false_positive"] += sum(not private and owner is not None and owner not in neutral for private, owner in zip(gold[path], mask))
    return result


def score(rows, records):
    if len(rows) != len(records):
        raise ValueError("missing or extra MCP capture records")
    result = {}
    for contract, config in contracts().items():
        layers = {layer: dict.fromkeys(FIELDS, 0) for layer in ("A", "D", "R")}
        categories, cases = {}, []
        for case, record in zip(rows, records):
            metrics = score_case(case, record, config["neutral"])
            cases.append({"id": case["id"], "layer": case["layer"], **metrics})
            for key, value in metrics.items():
                layers[case["layer"]][key] += value
                if case["layer"] == "D":
                    categories.setdefault(case["category"], dict.fromkeys(FIELDS, 0))[key] += value
        result[contract] = {"contract": config["identity"], "layers": layers, "d_counterweights": categories, "cases": cases}
    return result


def _compare_totals(base, candidate):
    for field in ("specification", "contracts", "model_files", "host_registry_lock", "policy_sha256"):
        if base[field] != candidate[field]:
            raise ValueError("non-comparable MCP arm identity: " + field)
    decisions = {}
    for contract in ("v2", "v1"):
        a, b = base["scores"][contract], candidate["scores"][contract]
        if [row["id"] for row in a["cases"]] != [row["id"] for row in b["cases"]]:
            raise ValueError("non-comparable MCP case populations")
        regressions = []
        for layer in ("A", "D", "R"):
            old, new = a["layers"][layer], b["layers"][layer]
            for metric in ("leaked", "refused", "dispatch_failed", "restore_failed", "manifest_invalid"):
                if new[metric] > old[metric]:
                    regressions.append(layer + ":" + metric)
            for metric in ("restore_exact", "manifest_valid"):
                if new[metric] < old[metric]:
                    regressions.append(layer + ":" + metric)
        # Pair by fixture, so moving a refusal between cases cannot hide a lost success.
        for old, new in zip(a["cases"], b["cases"]):
            if old["refused"] == 0 and new["refused"] != 0:
                regressions.append(new["id"] + ":new-refusal")
        removed = sum(a["layers"][l]["leaked"] - b["layers"][l]["leaked"] for l in ("A", "D", "R"))
        added_fp = sum(b["layers"][l]["false_positive"] - a["layers"][l]["false_positive"] for l in ("A", "D", "R"))
        improved = removed > 0 or added_fp < 0 or any(b["layers"][l][m] > a["layers"][l][m] for l in ("A", "D", "R") for m in ("restore_exact", "manifest_valid"))
        integrity_gain = any(b["layers"][l][m] > a["layers"][l][m] for l in ("A", "D", "R") for m in ("restore_exact", "manifest_valid"))
        no_fp_layer_rise = all(b["layers"][l]["false_positive"] <= a["layers"][l]["false_positive"] for l in ("A", "D", "R"))
        eligible_gain = added_fp < removed or (removed == added_fp == 0 and integrity_gain and no_fp_layer_rise)
        decisions[contract] = {"pass": not regressions and improved and eligible_gain, "regressions": regressions, "leaked_removed": removed, "false_positive_added": added_fp, "net_bytes": removed - added_fp, "base": a["layers"], "candidate": b["layers"], "d_counterweights_base": a["d_counterweights"], "d_counterweights_candidate": b["d_counterweights"]}
    return decisions


def compare(base, candidate):
    """Admit observations and independently replay the oracle before the gate."""
    specification = identity()
    known_contracts = {key: value["identity"] for key, value in contracts().items()}
    rows = fixture()
    for card in (base, candidate):
        if card["specification"] != specification or card["contracts"] != known_contracts:
            raise ValueError("capture is not bound to this committed MCP specification")
        build = card["host_build"]
        expected = {"runtime_revision": card["runtime_revision"], "binary_sha256": card["binary_sha256"],
                    "source_sha256": specification["scripts/bench/native-mcp-operation/main.rs"],
                    "builder_sha256": specification["scripts/bench/build_native_mcp_operation.py"],
                    "runtime_lock_sha256": card["host_registry_lock"]}
        if build["registry_dependencies_pinned_to_runtime"] is not True or any(build[key] != value for key, value in expected.items()):
            raise ValueError("MCP capture host provenance mismatch")
        if score(rows, card["records"]) != card["scores"]:
            raise ValueError("saved MCP scores differ from replayed observations")
    return _compare_totals(base, candidate)


def verify_policy_bundle_paths(policy, directories):
    data = tomllib.loads(Path(policy).read_text())
    selected = {
        "GAZE_NER_MODEL_DIR": data.get("ner", {}).get("model_dir"),
        "GAZE_NYM_MODEL_DIR": data.get("safety_net", {}).get("nym", {}).get("model_dir"),
    }
    for name, actual in selected.items():
        if not isinstance(actual, str) or Path(actual).resolve() != Path(directories[name]).resolve():
            raise ValueError("setup policy does not select the declared isolated bundle: " + name)


def capture(runtime_root, binary, policy, build_evidence):
    runtime_root, binary, policy = runtime_root.resolve(), binary.resolve(), policy.resolve()
    build = json.loads(build_evidence.read_text())
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=runtime_root, text=True).strip()
    expected = {"runtime_revision": revision, "source_sha256": identity()["scripts/bench/native-mcp-operation/main.rs"], "builder_sha256": identity()["scripts/bench/build_native_mcp_operation.py"], "binary_sha256": digest(binary), "runtime_lock_sha256": digest(runtime_root / "Cargo.lock")}
    if not build["registry_dependencies_pinned_to_runtime"] or any(build[key] != value for key, value in expected.items()):
        raise ValueError("host provenance differs from capture artifacts")
    # Require explicit isolated copies rather than guessing a shared live bundle.
    model_files = {}
    verify_policy_bundle_paths(policy, os.environ)
    for name in ("GAZE_NER_MODEL_DIR", "GAZE_NYM_MODEL_DIR"):
        folder = Path(os.environ[name]).resolve()
        if not folder.is_dir():
            raise ValueError("missing isolated model bundle " + name)
        model_files[name] = {str(p.relative_to(folder)): digest(p) for p in sorted(folder.rglob("*")) if p.is_file()}
        if not model_files[name]:
            raise ValueError("empty model bundle " + name)
    specification = identity()
    policy_sha256 = digest(policy)
    if subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=runtime_root):
        raise ValueError("runtime checkout has tracked changes before MCP capture")
    rows = fixture()
    fields = ("id", "direction", "payload", "seed_names")
    wire = "".join(json.dumps({key: row[key] for key in fields}, ensure_ascii=False) + "\n" for row in rows)
    process = subprocess.run([str(binary), str(policy)], input=wire, capture_output=True, text=True, cwd=runtime_root, timeout=1800)
    if process.returncode:
        raise ValueError("MCP benchmark host failed; preserve stderr separately: " + process.stderr[-2000:])
    records = [json.loads(line) for line in process.stdout.splitlines()]
    for name, before in model_files.items():
        folder = Path(os.environ[name]).resolve()
        after = {str(p.relative_to(folder)): digest(p) for p in sorted(folder.rglob("*")) if p.is_file()}
        if before != after:
            raise ValueError("model files changed during MCP capture")
    after_revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=runtime_root, text=True).strip()
    dirty = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=runtime_root)
    if after_revision != revision or dirty or identity() != specification or digest(binary) != expected["binary_sha256"] or digest(policy) != policy_sha256:
        raise ValueError("MCP capture artifacts changed during execution")
    return {"specification": specification, "contracts": {k: v["identity"] for k, v in contracts().items()}, "model_files": model_files, "host_registry_lock": build["runtime_lock_sha256"], "runtime_revision": revision, "host_build": build, "binary_sha256": digest(binary), "policy_sha256": policy_sha256, "policy_document": policy.read_bytes().decode("utf-8"), "machine": platform.platform(), "records": records, "scores": score(rows, records)}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-root", type=Path)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--policy", type=Path)
    parser.add_argument("--build-evidence", type=Path)
    parser.add_argument("--compare", nargs=2, type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.compare:
        result = compare(*(json.loads(path.read_text()) for path in args.compare))
    else:
        if any(value is None for value in (args.runtime_root, args.binary, args.policy, args.build_evidence)):
            parser.error("capture needs runtime-root, binary, policy and build-evidence")
        result = capture(args.runtime_root, args.binary, args.policy, args.build_evidence)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
