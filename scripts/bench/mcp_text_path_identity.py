#!/usr/bin/env python3
"""Hash every unchanged repository input outside the isolated MCP change.

This records structural identity, never fresh C/A/D/R inference. The explicit
allowlist is limited to the MCP runtime, new native MCP tools/evidence, the
rmcp-only golden counter, and exact CI evidence-path additions. Every other
tracked file must be identical.
"""
from __future__ import annotations
import argparse
import ast
import hashlib
import gzip
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
NATIVE_FILES = frozenset({
    "scripts/bench/build_native_mcp_operation.py", "scripts/bench/fixtures/mcp_operation_order.json",
    "scripts/bench/mcp_operation_order.py", "scripts/bench/mcp_operation_gate.py",
    "scripts/bench/test_mcp_operation_order.py", "scripts/bench/test_mcp_operation_gate.py",
    "scripts/bench/mcp_text_path_identity.py", "scripts/bench/test_mcp_text_path_identity.py",
    "scripts/bench/publish_mcp_operation_evidence.py", "scripts/bench/test_publish_mcp_operation_evidence.py",
})
NATIVE_PREFIXES = ("crates/gaze-mcp-core/", "scripts/bench/native-mcp-operation/", "docs/reference/benchmarks/gates/mcp-operation-order/")
# Documentation changes do not alter text inference. Keep their exact hashes
# in the record so the explicit exception remains reviewable and replayable.
DOCUMENTATION_FILES = frozenset({"CHANGELOG.md", "docs/explanation/mcp/mcp-runtime.md", "crates/gaze-mcp-rmcp/README.md"})
RMCP_FRONTEND = "crates/gaze-mcp-rmcp/src/frontend.rs"
RMCP_RESOURCE_ARM = '''        DispatchError::ResourceLimit => {
            CallToolResult::error(vec![ContentBlock::text("resource_limit")])
        }
'''
RMCP_RESOURCE_TEST = '''    #[test]
    fn resource_limit_returns_stable_error_class() {
        let result = dispatch_error_to_tool_result(DispatchError::ResourceLimit);
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].as_text().unwrap().text, "resource_limit");
    }
'''


def _rmcp_resource_limit_only(before, after):
    if after.count(RMCP_RESOURCE_ARM) != 1 or after.count(RMCP_RESOURCE_TEST) != 1:
        raise ValueError("rmcp resource-limit mapping or assertion missing")
    if after.replace(RMCP_RESOURCE_ARM, "").replace(RMCP_RESOURCE_TEST, "") != before:
        raise ValueError("rmcp changed beyond the resource-limit mapping and test")


GOLDEN = "scripts/bench/fixtures/evidence/mcp_route_v1.golden.json"
CI_REGISTRY = "scripts/ci/full-inputs.json"
EVIDENCE_PREFIX = "docs/reference/benchmarks/gates/mcp-operation-order/"
EVIDENCE_NAMES = frozenset({"README.md", "authority-and-cleanup.json", "base-native.json.gz",
    "base-v1.scorecard.json", "base-v2.scorecard.json", "candidate-native.json.gz",
    "candidate-v1.scorecard.json", "candidate-v2.scorecard.json", "gate.json",
    "native-comparison.json", "space-diagnostic.json", "text-path-identity.json.gz", "whitespace-diagnostic.json"})


def _tree(revision):
    raw = subprocess.check_output(["git", "ls-tree", "-rz", "--full-tree", revision], cwd=ROOT)
    result = {}
    for row in raw.split(b"\0"):
        if not row:
            continue
        meta, path = row.split(b"\t", 1)
        mode, kind, oid = meta.decode().split()
        result[path.decode()] = (mode, kind, oid)
    return result


def _allowed(path):
    return path in NATIVE_FILES or path in DOCUMENTATION_FILES or path.startswith(NATIVE_PREFIXES) or path in {GOLDEN, CI_REGISTRY, RMCP_FRONTEND}


def _ci_registry_only(before, after):
    # The measured runtime may predate the optional diagnostic publication.
    for names in (EVIDENCE_NAMES, EVIDENCE_NAMES - {"whitespace-diagnostic.json"}):
        a, b = json.loads(before), json.loads(after)
        a["paths"] = sorted(set(a["paths"]) | {EVIDENCE_PREFIX + name for name in names})
        if a == b:
            return
    raise ValueError("CI registry changed beyond the native evidence path declarations")


def _golden_only(before, after):
    a, b = json.loads(before), json.loads(after)
    if a["receipt"]["counts"]["observer_recognizer_source_events"] != 1 or b["receipt"]["counts"]["observer_recognizer_source_events"] != 3:
        raise ValueError("unexpected rmcp golden counter change")
    a["receipt"]["counts"]["observer_recognizer_source_events"] = 3
    if a != b:
        raise ValueError("rmcp golden changed more than the source-event counter")


def _blob(oid):
    return subprocess.check_output(["git", "cat-file", "blob", oid], cwd=ROOT)


def build(base, candidate):
    def revision(value):
        return subprocess.check_output(["git", "rev-parse", "--verify", value + "^{commit}"], cwd=ROOT, text=True).strip()
    base, candidate = revision(base), revision(candidate)
    a, b = _tree(base), _tree(candidate)
    changed = sorted(path for path in a.keys() | b.keys() if a.get(path) != b.get(path))
    forbidden = [path for path in changed if not _allowed(path)]
    if forbidden:
        raise ValueError("text-path or unclassified files changed: " + ", ".join(forbidden))
    # The only changed pre-existing benchmark fixture is rmcp route evidence,
    # not the C/A/D/R corpus. Check its exact permitted edit, rather than skip it.
    if RMCP_FRONTEND in changed:
        _rmcp_resource_limit_only(_blob(a[RMCP_FRONTEND][2]).decode(), _blob(b[RMCP_FRONTEND][2]).decode())
    if GOLDEN in changed:
        _golden_only(_blob(a[GOLDEN][2]), _blob(b[GOLDEN][2]))
    if CI_REGISTRY in changed:
        _ci_registry_only(_blob(a[CI_REGISTRY][2]), _blob(b[CI_REGISTRY][2]))
    for path in changed:
        if path in NATIVE_FILES and path in a:
            raise ValueError("native allowlist unexpectedly overlaps a base text input: " + path)
    documentation_changes = [{"path": path, "base_sha256": hashlib.sha256(_blob(a[path][2])).hexdigest(),
                              "candidate_sha256": hashlib.sha256(_blob(b[path][2])).hexdigest()}
                             for path in changed if path in DOCUMENTATION_FILES]
    transport_changes = [{"path": path, "base_sha256": hashlib.sha256(_blob(a[path][2])).hexdigest(),
                          "candidate_sha256": hashlib.sha256(_blob(b[path][2])).hexdigest()}
                         for path in changed if path == RMCP_FRONTEND]
    protected = sorted(path for path in a.keys() | b.keys() if not _allowed(path))
    digests, records = {}, []
    process = subprocess.Popen(["git", "cat-file", "--batch"], cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    try:
        for path in protected:
            mode, kind, oid = a[path]
            if kind != "blob":
                raise ValueError("unsupported protected git object: " + path)
            if oid not in digests:
                process.stdin.write((oid + "\n").encode()); process.stdin.flush()
                header = process.stdout.readline().decode().split()
                if len(header) != 3 or header[:2] != [oid, "blob"]:
                    raise ValueError("cannot read protected git blob")
                size = remaining = int(header[2])
                digest = hashlib.sha256()
                while remaining:
                    chunk = process.stdout.read(min(1024 * 1024, remaining))
                    if not chunk:
                        raise ValueError("truncated protected git blob")
                    digest.update(chunk); remaining -= len(chunk)
                if process.stdout.read(1) != b"\n":
                    raise ValueError("invalid protected git framing")
                digests[oid] = (digest.hexdigest(), size)
            digest, size = digests[oid]
            records.append({"path": path, "mode": mode, "base_sha256": digest, "candidate_sha256": digest, "bytes": size})
    finally:
        process.stdin.close(); process.stdout.close(); process.wait()
    # Unchanged text modules cannot import any of the newly added native tools.
    native_modules = {Path(path).stem for path in NATIVE_FILES if path.endswith(".py")}
    for path in protected:
        if not path.startswith("scripts/bench/") or not path.endswith(".py"):
            continue
        syntax = ast.parse(_blob(a[path][2]), filename=path)
        for node in ast.walk(syntax):
            names = []
            if isinstance(node, ast.Import):
                names = [alias.name.split(".")[0] for alias in node.names]
            elif isinstance(node, ast.ImportFrom):
                names = [node.module.split(".")[0]] if node.module else [alias.name for alias in node.names]
            if native_modules.intersection(names):
                raise ValueError("text module imports an excluded native module: " + path)
    return {"schema_version": 1, "kind": "mcp-text-path-identity", "base_revision": base, "candidate_revision": candidate,
            "text_layers": {layer: "unchanged_by_construction" for layer in ("C", "A", "D", "R")},
            "fresh_text_capture": False, "documentation_changes": documentation_changes, "transport_changes": transport_changes, "protected_files": records, "protected_file_count": len(records),
            "allowed_changed_paths": changed, "native_exact_paths": sorted(NATIVE_FILES), "native_prefixes": list(NATIVE_PREFIXES),
            "rmcp_counter_fixture": {"path": GOLDEN, "scope": "rmcp evidence only; sole change is source events 1 to 3"},
            "ci_registry_exception": {"path": CI_REGISTRY, "scope": "only the native evidence path declarations, with an optional whitespace diagnostic"},
            "generator_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}


def verify(record, candidate=None):
    replay = build(record["base_revision"], candidate or record["candidate_revision"])
    for field in ("kind", "text_layers", "fresh_text_capture", "protected_files", "protected_file_count", "generator_sha256", "documentation_changes", "transport_changes"):
        if replay[field] != record[field]:
            raise ValueError("text path-identity record differs: " + field)
    return replay


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base")
    parser.add_argument("--candidate", default="HEAD")
    parser.add_argument("--verify", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.verify:
        record = verify(json.loads(gzip.decompress(args.verify.read_bytes()) if args.verify.suffix == ".gz" else args.verify.read_bytes()), args.candidate)
    else:
        if not args.base or not args.output:
            parser.error("generation requires --base and --output")
        record = build(args.base, args.candidate)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps({"protected_files": record["protected_file_count"], "text_layers": record["text_layers"], "fresh_capture": False}))
