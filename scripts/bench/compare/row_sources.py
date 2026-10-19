"""Compose the Spanish scrubadub supplement without relabeling inherited measurements."""

from __future__ import annotations

import copy
import hashlib
import json
import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath

import compare
from comparison_availability import digest, unavailable_tools

REPLACED = ("scrubadub-base", "scrubadub-spacy")
# Reviewed predecessor: the only adapter change adds native es_ES routing.
ORIGINAL_ADAPTER = "ae0541144f907d070ad425d2467c794dda6a41d84f87d8c48fecb693273e424d"
SPANISH_ADAPTER = "5feba7769302ee34383ba60f8fc04ca3bea881435deaa8c5f5ce5d4e49474a31"
ORIGINAL_FINALIZER = "28d5d670463f8eb8724f634d09e0a78c65adae22a99558e95b9e1a2983980bab"
COMPATIBLE = (
    "corpus", "contracts", "scorer_sha256", "dataset_loader_sha256",
    "mapping_sha256", "model_pins_sha256", "requirements_sha256",
    "scrubadub_requirements_sha256", "comparison_metrics_sha256",
    "cpu_contention_sha256", "opf_adapter_sha256", "runner_sha256",
    "heldout_split", "common_intersection_labels", "hardware", "device",
)


def file_hash(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def protect_inputs(output: Path, inputs: list[Path]) -> None:
    """Reject path, symlink and existing hardlink aliases without opening output."""
    for source in inputs:
        if output.resolve() == source.resolve() or (
                output.exists() and source.exists() and output.samefile(source)):
            raise ValueError("input evidence would be overwritten by output")


def native_receipt_path(report: Path) -> Path:
    # Fixed by the publication filename, never redirected by submitted fields.
    return report.with_name(report.name + ".native-receipt.json")


def _native_receipt_bytes(manifest: dict) -> bytes:
    receipt = {"schema_version": 1, "sources": {
        name: {"report_sha256": manifest[name]["sha256"], "records": manifest[name]["native_records"]}
        for name in ("base", "supplement")}}
    return (json.dumps(receipt, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _retain_native_receipt(path: Path, raw: bytes) -> None:
    """Create once; an existing reviewed receipt can only be reused unchanged."""
    if path.is_symlink():
        raise ValueError("native receipt must be a retained regular file")
    try:
        with path.open("xb") as destination:
            destination.write(raw)
    except FileExistsError:
        if path.read_bytes() != raw:
            raise ValueError("retained native receipt differs; use a new publication path")


def adapter_is_compatible(recorded: str, current: str) -> bool:
    return recorded == current or (recorded == ORIGINAL_ADAPTER and current == SPANISH_ADAPTER)


def finalizer_is_compatible(recorded: str, current: str) -> bool:
    return recorded in (ORIGINAL_FINALIZER, current)


def complete(report: dict) -> None:
    unavailable = unavailable_tools(report)
    if (report["schema_version"] != 2 or set(report["tools"]) | unavailable != set(compare.TOOLS)
            or report["skipped"]):
        raise ValueError("only a complete, unskipped schema-v2 comparison can be finalized")
    expected = set(report["tools"]) | {"gaze-full", "gaze-rules-only", "gaze-rules-ner"}
    if set(report.get("contention_samples", {})) != expected:
        raise ValueError("foreign CPU samples are missing for a measured configuration")


def _compatible(base: dict, supplement: dict) -> None:
    complete(base)
    if unavailable_tools(base) != {"opf"}:
        raise ValueError("Spanish composition requires the original OPF unavailable receipt")
    if base.get("latency_validity", {}).get("publishable") is not False:
        raise ValueError("composition requires the original timing-withheld publication")
    if "row_sources" in base or "row_sources" in supplement:
        raise ValueError("composition accepts original snapshots, not nested compositions")
    if (base["compare_sha256"] != ORIGINAL_ADAPTER
            or supplement["compare_sha256"] != SPANISH_ADAPTER
            or file_hash(Path(compare.__file__)) != SPANISH_ADAPTER):
        raise ValueError("composition requires the reviewed Spanish-only adapter binding")
    if (base.get("harness_dirty") is not False or supplement.get("harness_dirty") is not False
            or supplement["schema_version"] != 2 or supplement.get("skipped")
            or supplement.get("unavailable") or set(supplement["tools"]) != set(REPLACED)
            or set(supplement.get("contention_samples", {})) != set(REPLACED)):
        raise ValueError("supplement must contain exactly two clean measured scrubadub configurations")
    if "measurement_compare_sha256" in supplement or "rescore_source_revision" in supplement:
        raise ValueError("supplement must be fresh inference, not a replay")
    for key in COMPATIBLE:
        if key not in base or key not in supplement or base[key] != supplement[key]:
            raise ValueError(f"composition input differs in {key}")
    for source in (base, supplement):
        if (not re.fullmatch(r"[0-9a-f]{40}", source["harness_revision"])
                or not isinstance(source.get("generated_at"), str)):
            raise ValueError("source lacks revision or collection/replay time")
    for name in REPLACED:
        old = base["tools"][name]["provenance"]
        new = supplement["tools"][name]["provenance"]
        ignored = {"supported_languages", "measurement_harness_revision", "measurement_compare_sha256"}
        if {k: v for k, v in old.items() if k not in ignored} != {k: v for k, v in new.items() if k not in ignored}:
            raise ValueError(f"{name} tool configuration or model pins differ")
        if set(new["supported_languages"]) != set(old["supported_languages"]) | {"es"}:
            raise ValueError(f"{name} must add only native Spanish support")
        for key, value in (("measurement_harness_revision", supplement["harness_revision"]),
                           ("measurement_compare_sha256", supplement["compare_sha256"])):
            if key in new and new[key] != value:
                raise ValueError(f"{name} measurement adapter/source binding differs")
        tool = supplement["tools"][name]
        if tool["tool"] != name or set(tool["contracts"]) != set(compare.CONTRACTS):
            raise ValueError(f"{name} lacks every scored contract")
        for contract in tool["contracts"].values():
            if set(contract) != set(base["corpus"]["layers"]):
                raise ValueError(f"{name} layer roster differs")
            for layer, row in contract.items():
                count = base["corpus"]["layers"][layer]["documents"]
                if (row["documents"] != count or row["processed_documents"] != count
                        or row["skipped_documents"] != 0 or row["skipped_languages"]
                        or row["skipped_gold_bytes"] != 0 or row["latency"]["samples"] != count):
                    raise ValueError(f"{name}/{layer} requires full denominators and zero skips")


def native_hashes(report: dict, root: Path) -> dict:
    """Hash private native spans as bytes; never decode or publish their records."""
    if root.resolve().is_relative_to(compare.REPO.resolve()):
        raise ValueError("native predictions must stay outside the repository")
    names = list(report["tools"])
    if report.get("gaze_inference"):
        names += ["gaze-full", "gaze-rules-only", "gaze-rules-ner"]
    return {name: {layer: {"sha256": file_hash(path), "bytes": path.stat().st_size}
                   for layer in report["corpus"]["layers"]
                   for path in (compare.prediction_path(root, layer, name),)}
            for name in names}


def source_ref(path: Path, expected_hash: str, output: Path, records: dict) -> tuple[dict, dict]:
    # Snapshots travel with the publication; no git history or private paths needed.
    relative = path.resolve().relative_to(output.resolve().parent).as_posix()
    raw = path.read_bytes()
    for destination in (output, output.with_suffix(output.suffix + ".tmp")):
        protect_inputs(destination, [path])
    if hashlib.sha256(raw).hexdigest() != expected_hash:
        raise ValueError("source snapshot hash differs or would be overwritten")
    report = json.loads(raw)
    return report, {"path": relative, "sha256": expected_hash, "native_records": records,
                    "native_records_sha256": digest({"report_sha256": expected_hash, "records": records})}


def _assemble(base: dict, supplement: dict, manifest: dict) -> dict:
    _compatible(base, supplement)
    result = copy.deepcopy(base)
    for name in REPLACED:
        result["tools"][name] = copy.deepcopy(supplement["tools"][name])
        result["contention_samples"][name] = copy.deepcopy(supplement["contention_samples"][name])
    result["latency_validity"] = {
        **base["latency_validity"], "publishable": False,
        "contended_configurations": sorted(name for name, sample in result["contention_samples"].items()
                                            if sample["contended"]),
    }
    result["row_sources"] = manifest
    return result


def compose(base_path: Path, base_sha: str, supplement_path: Path, supplement_sha: str,
            output: Path, base_predictions: Path, supplement_predictions: Path) -> dict:
    base, base_ref = source_ref(base_path, base_sha, output, {})
    supplement, supplement_ref = source_ref(supplement_path, supplement_sha, output, {})
    _compatible(base, supplement)
    base_ref["native_records"] = native_hashes(base, base_predictions)
    supplement_ref["native_records"] = native_hashes(supplement, supplement_predictions)
    for ref in (base_ref, supplement_ref):
        ref["native_records_sha256"] = digest({"report_sha256": ref["sha256"], "records": ref["native_records"]})
    manifest = {
        "schema_version": 1, "base": base_ref, "supplement": supplement_ref,
        "tool_sources": {name: "supplement" if name in REPLACED else "base" for name in base["tools"]},
        "aggregation": {
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "harness_revision": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=compare.REPO, text=True).strip(),
            "harness_dirty": bool(subprocess.check_output(
                ["git", "status", "--porcelain"], cwd=compare.REPO, text=True).strip()),
            "composer_sha256": file_hash(Path(__file__)),
            "compare_sha256": file_hash(Path(compare.__file__)),
            "finalizer_sha256": file_hash(Path(__file__).with_name("finalize_report.py")),
        },
    }
    raw_receipt = _native_receipt_bytes(manifest)
    manifest["native_receipt_sha256"] = hashlib.sha256(raw_receipt).hexdigest()
    result = _assemble(base, supplement, manifest)
    # Validate all current public inputs before creating any publication evidence.
    from render import validate_current
    validate_current(base, base_path)
    validate_current(supplement, supplement_path)
    receipt_path = native_receipt_path(output)
    protect_inputs(receipt_path, [base_path, supplement_path, output, output.with_suffix(output.suffix + ".tmp")])
    _retain_native_receipt(receipt_path, raw_receipt)
    validate(result, output)
    return result


def validate(report: dict, path: Path) -> None:
    manifest = report["row_sources"]
    if set(manifest) != {"schema_version", "base", "supplement", "tool_sources", "aggregation", "native_receipt_sha256"} or manifest["schema_version"] != 1:
        raise ValueError("unsupported row-source manifest")
    receipt_path = native_receipt_path(path)
    if receipt_path.is_symlink():
        raise ValueError("native receipt must be a retained regular file")
    for destination in (path, path.with_suffix(path.suffix + ".tmp")):
        protect_inputs(destination, [receipt_path])
    raw_receipt = receipt_path.read_bytes()
    if (hashlib.sha256(raw_receipt).hexdigest() != manifest["native_receipt_sha256"]
            or raw_receipt != _native_receipt_bytes(manifest)):
        raise ValueError("native-record map differs from retained native receipt")
    sources = {}
    for name in ("base", "supplement"):
        ref = manifest[name]
        if set(ref) != {"path", "sha256", "native_records", "native_records_sha256"}:
            raise ValueError("source must bind a snapshot and aggregate native hashes")
        if ref["native_records_sha256"] != digest({"report_sha256": ref["sha256"], "records": ref["native_records"]}):
            raise ValueError("native-record hashes differ from source binding")
        relative = PurePosixPath(ref["path"])
        if relative.is_absolute() or ".." in relative.parts or "\\" in ref["path"]:
            raise ValueError("source snapshot must use a portable relative path")
        source_path = path.parent / ref["path"]
        source, _ = source_ref(source_path, ref["sha256"], path, ref["native_records"])
        sources[name] = source
        names = set(source["tools"])
        if source.get("gaze_inference"):
            names |= {"gaze-full", "gaze-rules-only", "gaze-rules-ner"}
        records = ref["native_records"]
        if set(records) != names:
            raise ValueError("native-record tool roster differs")
        for layers in records.values():
            if set(layers) != set(source["corpus"]["layers"]):
                raise ValueError("native-record layer roster differs")
            for record in layers.values():
                if (set(record) != {"sha256", "bytes"} or not re.fullmatch(r"[0-9a-f]{64}", record["sha256"])
                        or type(record["bytes"]) is not int or record["bytes"] < 0):
                    raise ValueError("invalid native-record hash")
    expected_sources = {name: "supplement" if name in REPLACED else "base" for name in sources["base"]["tools"]}
    if manifest["tool_sources"] != expected_sources:
        raise ValueError("tool source attribution differs")
    aggregation = manifest["aggregation"]
    if (set(aggregation) != {"generated_at", "harness_revision", "harness_dirty", "composer_sha256", "compare_sha256", "finalizer_sha256"}
            or type(aggregation["harness_dirty"]) is not bool
            or not re.fullmatch(r"[0-9a-f]{40}", aggregation["harness_revision"])
            or not isinstance(aggregation["generated_at"], str)):
        raise ValueError("invalid aggregation identity")
    for key, source in (("composer_sha256", Path(__file__)), ("compare_sha256", Path(compare.__file__)),
                        ("finalizer_sha256", Path(__file__).with_name("finalize_report.py"))):
        if aggregation[key] != file_hash(source):
            raise ValueError(f"aggregation {key} changed")
    expected = _assemble(sources["base"], sources["supplement"], manifest)
    if report != expected:
        raise ValueError("composed rows or inherited metadata differ from source snapshots")
    complete(report)


def publication_note(report: dict) -> str:
    if "row_sources" not in report:
        if report.get("compare_sha256") == ORIGINAL_ADAPTER:
            return "The original scrubadub measurements predate the Spanish adapter correction; their Spanish inputs count as fully missed."
        return ""
    refs = report["row_sources"]
    return (
        "Mixed-source publication: only scrubadub-base and scrubadub-spacy were freshly measured with native Spanish support. "
        "The other eleven configurations, Gaze measurements, threshold choices and OPF unavailable receipt retain their original identities. "
        + " ".join(f"{name.capitalize()} snapshot: [`{refs[name]['path']}`]({refs[name]['path']}), SHA-256 `{refs[name]['sha256']}`."
                   for name in ("base", "supplement"))
        + " Snapshot source revisions, adapter hashes, collection/replay times and per-row provenance remain authoritative; "
        "the row_sources aggregation time records composition only. Timing comparisons remain withheld."
    )
