#!/usr/bin/env python3
"""Record measured adjacency layers and render their benchmark table."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DOC = ROOT / "docs/reference/benchmarks/README.md"
HISTORY = ROOT / "docs/reference/benchmarks/agentic-adjacency-v4-history.json"
CONTRACT = ROOT / "docs/reference/benchmarks/scored-labels-agentic.json"
GENERATOR_VERSION = 4
CORPUS_SHA256 = "782052b31d9e6061a5e93961aeb245e482b34e3915d0dab5610331be33fed981"
POLICY_SHA256 = "f909a23aecacc5695388223be5e71bc1e303c845563396d6658448396a0a9ebe"
EXPECTED_ROWS = (
    ("v0.15.1", "policy-file"),
    ("v0.15.0", "policy-file"),
    ("v0.14.0", "full-stack-kiji-resolve"),
    ("v0.14.0", "pass2-ner"),
)
BEGIN = "<!-- BEGIN GENERATED: agentic-adjacency-v4 -->"
END = "<!-- END GENERATED: agentic-adjacency-v4 -->"


class HistoryError(ValueError):
    pass


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _hex64(value: object, field: str) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value):
        raise HistoryError(f"{field} must be a SHA-256 digest")
    return value


def _metric(run: dict, field: str) -> int:
    value = run.get("metrics", {}).get("utf8_bytes", {}).get(field)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise HistoryError(f"invalid utf8_bytes.{field} metric")
    return value


def rows_from_scorecard(path: Path) -> list[dict]:
    scorecard = json.loads(path.read_text(encoding="utf-8"))
    version = scorecard.get("measured")
    if version not in {release for release, _ in EXPECTED_ROWS}:
        raise HistoryError(f"unsupported measured release {version!r}")
    generator = scorecard.get("layers", {}).get("generator", {})
    if generator.get("generator_version") != GENERATOR_VERSION or generator.get("corpus_sha256") != CORPUS_SHA256:
        raise HistoryError("scorecard did not measure the pinned v4 test corpus")
    contract = scorecard["layers"].get("scored_label_contract", {})
    if contract.get("file_sha256") != sha256(CONTRACT):
        raise HistoryError("scorecard agentic contract differs from the committed contract")
    parameters = scorecard.get("parameters", {})
    if parameters.get("policy_sha256") != POLICY_SHA256:
        raise HistoryError("scorecard policy differs from the setup policy")
    binary_sha = _hex64(scorecard.get("binary_sha256"), "binary_sha256")
    binary_commit = scorecard.get("binary_commit", {})
    revision = binary_commit.get("revision")
    if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-f]{40}", revision) or binary_commit.get("dirty"):
        raise HistoryError("release binary must have a clean source revision")
    configs = {run["config"] for run in scorecard["layers"]["A"]["runs"]}
    rows = []
    for config in sorted(configs):
        if (version, config) not in EXPECTED_ROWS:
            raise HistoryError(f"unsupported release arm {version}/{config}")
        layers = {}
        for name in ("A", "D", "R"):
            matches = [run for run in scorecard["layers"][name]["runs"] if run["config"] == config]
            if len(matches) != 1:
                raise HistoryError(f"{version}/{config} needs one {name} run")
            run = matches[0]
            failed = run.get("pipeline_availability", {}).get("failed_closed_documents")
            if failed != 0:
                raise HistoryError(f"{version}/{config}/{name} has {failed!r} failed-closed documents")
            layers[name] = {
                "gold": _metric(run, "pii"),
                "leaked": _metric(run, "leaked"),
                "false_positive": _metric(run, "false_positive"),
            }
        rows.append({
            "version": version, "arm": config, "binary_revision": revision,
            "binary_sha256": binary_sha, "scorecard_sha256": sha256(path),
            "manifest_actions": parameters.get("manifest_replacing_actions"),
            "split_composite_source_ids": parameters.get("split_composite_source_ids"),
            "layers": layers,
        })
    return rows


def load_history(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if value.get("schema_version") != 1 or value.get("generator_version") != GENERATOR_VERSION:
        raise HistoryError("adjacency history schema or generator version differs")
    if value.get("corpus_sha256") != CORPUS_SHA256 or value.get("policy_sha256") != POLICY_SHA256:
        raise HistoryError("adjacency history corpus or policy differs")
    if value.get("contract_sha256") != sha256(CONTRACT):
        raise HistoryError("adjacency history contract differs")
    rows = value.get("rows")
    if not isinstance(rows, list) or {(row.get("version"), row.get("arm")) for row in rows} != set(EXPECTED_ROWS):
        raise HistoryError("adjacency history needs exactly the displayed release arms")
    for row in rows:
        _hex64(row.get("binary_sha256"), "binary_sha256")
        _hex64(row.get("scorecard_sha256"), "scorecard_sha256")
        for layer in ("A", "D", "R"):
            for field in ("gold", "leaked", "false_positive"):
                metric = row.get("layers", {}).get(layer, {}).get(field)
                if isinstance(metric, bool) or not isinstance(metric, int) or metric < 0:
                    raise HistoryError(f"invalid {layer}.{field} metric")
    return value


def render(history: dict) -> str:
    by_key = {(row["version"], row["arm"]): row for row in history["rows"]}
    lines = [
        "| Release and arm | A leaked / gold B | A FP B | D FP B | R leaked / gold B | R FP B |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for key in EXPECTED_ROWS:
        row = by_key[key]
        a, d, r = (row["layers"][layer] for layer in ("A", "D", "R"))
        lines.append(
            f"| `{row['version']}` `{row['arm']}` | "
            f"{a['leaked']:,} / {a['gold']:,} | {a['false_positive']:,} | "
            f"{d['false_positive']:,} | {r['leaked']:,} / {r['gold']:,} | {r['false_positive']:,} |"
        )
    lines += [
        "",
        "These are layers A, D and R only, measured by the current harness against each "
        "release's own binary. Layer C release headlines above are unchanged. The "
        "[committed measurement ledger](agentic-adjacency-v4-history.json) records "
        "binary and scorecard SHA-256 digests, arm and manifest semantics. "
        f"Generator v{GENERATOR_VERSION}, test corpus `{CORPUS_SHA256[:12]}…`, "
        f"setup policy `{POLICY_SHA256[:12]}…`.",
    ]
    return "\n".join(lines)


def apply(doc: str, history: dict) -> str:
    start, stop = doc.find(BEGIN), doc.find(END)
    if start < 0 or stop < start:
        raise HistoryError("benchmark document lacks the agentic adjacency markers")
    return doc[: start + len(BEGIN)] + "\n\n" + render(history) + "\n\n" + doc[stop:]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--record", type=Path, action="append", default=[])
    args = parser.parse_args()
    try:
        if args.record:
            if args.check:
                raise HistoryError("--record and --check are mutually exclusive")
            history = {
                "schema_version": 1, "generator_version": GENERATOR_VERSION,
                "corpus_sha256": CORPUS_SHA256, "contract_sha256": sha256(CONTRACT),
                "policy_sha256": POLICY_SHA256, "rows": [],
            }
            for path in args.record:
                history["rows"].extend(rows_from_scorecard(path))
            HISTORY.write_text(json.dumps(history, indent=2) + "\n", encoding="utf-8")
        history = load_history(HISTORY)
        original = DOC.read_text(encoding="utf-8")
        rendered = apply(original, history)
        if args.check:
            if original != rendered:
                raise HistoryError("agentic adjacency table differs from the committed ledger")
        else:
            DOC.write_text(rendered, encoding="utf-8")
        print("agentic adjacency benchmark table is in sync" if args.check else "wrote agentic adjacency benchmark table")
        return 0
    except (HistoryError, KeyError, OSError, json.JSONDecodeError) as error:
        parser.exit(2, f"render_agentic_adjacency_doc: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
