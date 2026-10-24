#!/usr/bin/env python3
"""Record measured adjacency layers and render their benchmark table."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path

import agentic_layers
import benchmark_spec


ROOT = Path(__file__).resolve().parents[2]
DOC = ROOT / "docs/reference/benchmarks/README.md"
# Pin the measured version so a later generator cannot relabel these rows.
# Earlier v4, v8, v9, v10 and v11 ledgers remain committed as historical measurements.
GENERATOR_VERSION = 20
HISTORY = ROOT / f"docs/reference/benchmarks/agentic-adjacency-v{GENERATOR_VERSION}-history.json"
CORPUS_SHA256, CONTRACT_SHA256 = agentic_layers.corpus_identity(ROOT, GENERATOR_VERSION)

# Setup policies retain the model-path identity of their measuring machine.
POLICY_SHA256 = "f909a23aecacc5695388223be5e71bc1e303c845563396d6658448396a0a9ebe"
HISTORICAL_POLICY_SHA256 = {
    **dict.fromkeys((4, 8, 9),
                    "f909a23aecacc5695388223be5e71bc1e303c845563396d6658448396a0a9ebe"),
    10: "6525f0002a6d5bf5f6d9fde6ca35f439f88cf9e6ec90af6d7f0380f805d2fec7",
}


@dataclass(frozen=True)
class HistoryInputs:
    version: int
    corpus_sha256: str
    contract_sha256: str
    path: Path
    policy_sha256: str

    @classmethod
    def for_version(cls, version: int) -> HistoryInputs:
        corpus, contract = agentic_layers.corpus_identity(ROOT, version)
        return cls(version, corpus, contract,
                   ROOT / f'docs/reference/benchmarks/agentic-adjacency-v{version}-history.json',
                   HISTORICAL_POLICY_SHA256.get(version, POLICY_SHA256))


MEASURED_INPUTS = HistoryInputs.for_version(GENERATOR_VERSION)
LEGACY_ROWS = (
    ("v0.15.1", "policy-file"),
    ("v0.15.0", "policy-file"),
    ("v0.14.0", "full-stack-kiji-resolve"),
    ("v0.14.0", "pass2-ner"),
)
CURRENT_ROWS = (("v0.16.0", "policy-file"), *LEGACY_ROWS)
EXPECTED_ROWS = CURRENT_ROWS if GENERATOR_VERSION >= 13 else LEGACY_ROWS
V16_POLICY_SHA256 = "65cd6aa588ce6923f9e2156699893fcffb9a870967d9a0db5018deaa0c22c8cc"


def expected_rows(inputs: HistoryInputs) -> tuple:
    return CURRENT_ROWS if inputs.version >= 13 else LEGACY_ROWS


def release_policy_sha256(version: str, inputs: HistoryInputs) -> str:
    return V16_POLICY_SHA256 if inputs.version >= 13 and version == 'v0.16.0' else inputs.policy_sha256

BEGIN = "<!-- BEGIN GENERATED: agentic-adjacency -->"
END = "<!-- END GENERATED: agentic-adjacency -->"


class HistoryError(ValueError):
    pass


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _hex64(value: object, field: str) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value):
        raise HistoryError(f"{field} must be a SHA-256 digest")
    return value


def unavailable_receipt(path: Path, inputs: HistoryInputs) -> dict:
    """Admit only a pinned, value-free failure receipt for the legacy producer."""
    folder = ROOT / 'docs/reference/benchmarks'
    expected_name = f'unavailable-v0.14.0-generated-v{inputs.version}.json'
    if inputs.version < 20 or path.name != expected_name or path.is_symlink() or path.resolve().parent != folder.resolve():
        raise HistoryError('unavailability receipt must be a regular committed benchmark file')
    value = json.loads(path.read_text(encoding='utf-8'))
    fields = {'schema_version', 'kind', 'version', 'binary_revision', 'binary_sha256',
              'harness_revision', 'driver_sha256', 'generator_version', 'corpus_sha256',
              'contract_sha256', 'configs', 'manifest_replacing_actions', 'attempts'}
    if not isinstance(value, dict) or set(value) != fields:
        raise HistoryError('unavailability receipt has unknown or missing fields')
    if (type(value['schema_version']) is not int or type(value['generator_version']) is not int
            or value['schema_version'] != 1 or value['kind'] != 'historical-producer-unavailable'
            or value['version'] != 'v0.14.0'
            or value['binary_revision'] != '616e8a10b957961f9f7dcb745610afa978783c3a'
            or value['generator_version'] != inputs.version
            or value['corpus_sha256'] != inputs.corpus_sha256
            or value['contract_sha256'] != inputs.contract_sha256):
        raise HistoryError('unavailability receipt source or corpus differs')
    if not isinstance(value['harness_revision'], str) or not re.fullmatch(r'[0-9a-f]{40}', value['harness_revision']):
        raise HistoryError('unavailability receipt needs a harness revision')
    _hex64(value['binary_sha256'], 'binary_sha256')
    _hex64(value['driver_sha256'], 'driver_sha256')
    if value['driver_sha256'] != sha256(ROOT / 'scripts/bench/rescore_past_release.py'):
        try:
            measured_driver = subprocess.check_output(
                ['git', 'show', f"{value['harness_revision']}:scripts/bench/rescore_past_release.py"],
                cwd=ROOT, stderr=subprocess.PIPE,
            )
        except (OSError, subprocess.CalledProcessError) as error:
            raise HistoryError('cannot verify the original measurement driver; fetch its harness commit') from error
        if hashlib.sha256(measured_driver).hexdigest() != value['driver_sha256']:
            raise HistoryError('unavailability receipt differs from its original measurement driver')
    if (value['configs'] != ['rule-floor-extended', 'pass2-ner', 'full-stack-kiji-resolve']
            or value['manifest_replacing_actions'] != ['tokenize']):
        raise HistoryError('unavailability receipt does not describe the legacy release arms')
    attempts = value['attempts']
    if not isinstance(attempts, list) or len(attempts) != 2:
        raise HistoryError('unavailability receipt needs fresh v1 and v2 attempts')
    contracts = []
    for attempt in attempts:
        if not isinstance(attempt, dict) or set(attempt) != {'scored_label_contract', 'exit_code', 'error_phase', 'error_code', 'log_sha256'}:
            raise HistoryError('unavailability attempt has unknown or missing fields')
        if (type(attempt['scored_label_contract']) is not int or type(attempt['exit_code']) is not int
                or attempt['exit_code'] != 1 or attempt['error_phase'] != 'payload_processing'
                or attempt['error_code'] != 'payload'):
            raise HistoryError('unavailability requires a recorded producer protocol failure')
        contracts.append(attempt['scored_label_contract'])
        _hex64(attempt['log_sha256'], 'log_sha256')
    if sorted(contracts) != [1, 2]:
        raise HistoryError('unavailability receipt needs fresh v1 and v2 attempts')
    return value


def rows_from_unavailable_receipt(path: Path, inputs: HistoryInputs) -> list[dict]:
    value = unavailable_receipt(path, inputs)
    return [{'version': version, 'arm': arm, 'unavailable': True,
             'binary_revision': value['binary_revision'], 'binary_sha256': value['binary_sha256'],
             'evidence_file': path.name, 'evidence_sha256': sha256(path)}
            for version, arm in expected_rows(inputs) if version == value['version']]


def validate_unavailable_row(row: dict, inputs: HistoryInputs) -> dict:
    fields = {'version', 'arm', 'unavailable', 'binary_revision', 'binary_sha256', 'evidence_file', 'evidence_sha256'}
    if set(row) != fields or row['unavailable'] is not True:
        raise HistoryError('unavailable rows cannot contain measured metrics')
    name = row['evidence_file']
    if not isinstance(name, str) or Path(name).name != name:
        raise HistoryError('unavailability evidence must be a basename')
    path = ROOT / 'docs/reference/benchmarks' / name
    value = unavailable_receipt(path, inputs)
    if _hex64(row['evidence_sha256'], 'evidence_sha256') != sha256(path):
        raise HistoryError('unavailability evidence digest differs')
    if (row['version'] != value['version'] or row['arm'] not in value['configs']
            or row['binary_revision'] != value['binary_revision'] or row['binary_sha256'] != value['binary_sha256']):
        raise HistoryError('unavailable row differs from its failure receipt')
    return value


def _metric(run: dict, field: str) -> int:
    value = run.get("metrics", {}).get("utf8_bytes", {}).get(field)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise HistoryError(f"invalid utf8_bytes.{field} metric")
    return value


def _document_contract(run: dict) -> dict[str, int]:
    contract = run.get("pipeline_contract", {})
    fields = ("documents", "restore_exact_documents", "restore_success_decisions", "manifest_valid_documents")
    values = {}
    for field in fields:
        value = contract.get(field)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise HistoryError(f"invalid pipeline_contract.{field}")
        values[field] = value
    documents = values["documents"]
    if any(values[field] > documents for field in fields[1:]):
        raise HistoryError("pipeline contract counts exceed completed documents")
    if documents != run.get("pipeline_availability", {}).get("completed_documents"):
        raise HistoryError("pipeline contract population differs from completed documents")
    return {
        "documents": documents,
        "restore_exact": values["restore_exact_documents"],
        "restore_failures": documents - values["restore_exact_documents"],
        "valid_manifests": values["manifest_valid_documents"],
        "invalid_manifests": documents - values["manifest_valid_documents"],
    }


def rows_from_scorecard(path: Path, inputs: HistoryInputs = MEASURED_INPUTS) -> list[dict]:
    payload = path.read_bytes()
    scorecard = json.loads(gzip.decompress(payload) if path.suffix == '.gz' else payload)
    try:
        benchmark_spec.identity(scorecard)
    except benchmark_spec.SpecError as error:
        raise HistoryError(error.code) from error
    version = scorecard.get("measured")
    if version not in {release for release, _ in expected_rows(inputs)}:
        raise HistoryError(f"unsupported measured release {version!r}")
    generator = scorecard.get("layers", {}).get("generator", {})
    if generator.get("generator_version") != inputs.version or generator.get("corpus_sha256") != inputs.corpus_sha256:
        raise HistoryError(f"scorecard did not measure the pinned v{inputs.version} test corpus")
    contract = scorecard["layers"].get("scored_label_contract", {})
    if contract.get("file_sha256") != inputs.contract_sha256:
        raise HistoryError("scorecard agentic contract differs from the committed contract")
    parameters = scorecard.get("parameters", {})
    if parameters.get("policy_sha256") != release_policy_sha256(version, inputs):
        raise HistoryError("scorecard policy differs from the setup policy")
    binary_sha = _hex64(scorecard.get("binary_sha256"), "binary_sha256")
    binary_commit = scorecard.get("binary_commit", {})
    revision = binary_commit.get("revision")
    if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-f]{40}", revision) or binary_commit.get("dirty"):
        raise HistoryError("release binary must have a clean source revision")
    configs = {run["config"] for run in scorecard["layers"]["A"]["runs"]}
    rows = []
    for config in sorted(configs):
        if (version, config) not in expected_rows(inputs):
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
                **(_document_contract(run) if inputs.version >= 16 else {}),
            }
        rows.append({
            **({"benchmark_spec": scorecard["benchmark_spec"]} if "benchmark_spec" in scorecard else {}),
            "version": version, "arm": config, "binary_revision": revision,
            "binary_sha256": binary_sha, "scorecard_sha256": sha256(path),
            "manifest_actions": parameters.get("manifest_replacing_actions"),
            "split_composite_source_ids": parameters.get("split_composite_source_ids"),
            "layers": layers,
            **({'scorecard_file': path.name,
                'policy_sha256': parameters['policy_sha256'],
                'refused': {name: next(run for run in scorecard['layers'][name]['runs']
                                      if run['config'] == config)['pipeline_availability']['failed_closed_documents']
                            for name in ('A', 'D', 'R')}} if inputs.version >= 13 else {}),
        })
    return rows


def load_history(path: Path, inputs: HistoryInputs = MEASURED_INPUTS) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    return validate_history(value, inputs)


def validate_history(value: dict, inputs: HistoryInputs = MEASURED_INPUTS) -> dict:
    if value.get("schema_version") != 1 or value.get("generator_version") != inputs.version:
        raise HistoryError("adjacency history schema or generator version differs")
    if value.get("corpus_sha256") != inputs.corpus_sha256 or value.get("policy_sha256") != inputs.policy_sha256:
        raise HistoryError("adjacency history corpus or policy differs")
    if value.get("contract_sha256") != inputs.contract_sha256:
        raise HistoryError("adjacency history contract differs")
    rows = value.get("rows")
    if not isinstance(rows, list) or len(rows) != len(expected_rows(inputs)) or {(row.get("version"), row.get("arm")) for row in rows} != set(expected_rows(inputs)):
        raise HistoryError("adjacency history needs exactly the displayed release arms")
    for row in rows:
        try:
            if 'unavailable' not in row:
                benchmark_spec.require_same(value, row, required=False)
            else:
                benchmark_spec.identity(value)
        except benchmark_spec.SpecError as error:
            raise HistoryError(error.code) from error
        if 'unavailable' in row:
            validate_unavailable_row(row, inputs)
            continue
        _hex64(row.get("binary_sha256"), "binary_sha256")
        _hex64(row.get("scorecard_sha256"), "scorecard_sha256")
        if inputs.version >= 13:
            if row.get('policy_sha256') != release_policy_sha256(row['version'], inputs):
                raise HistoryError('release row policy differs from its setup policy')
            if row.get('refused') != dict.fromkeys(('A', 'D', 'R'), 0):
                raise HistoryError('release row must record zero refusals for each layer')
        for layer in ("A", "D", "R"):
            for field in (("gold", "leaked", "false_positive", "documents", "restore_exact",
                           "restore_failures", "valid_manifests", "invalid_manifests")
                          if inputs.version >= 16 else ("gold", "leaked", "false_positive")):
                metric = row.get("layers", {}).get(layer, {}).get(field)
                if isinstance(metric, bool) or not isinstance(metric, int) or metric < 0:
                    raise HistoryError(f"invalid {layer}.{field} metric")
    return value


def render(history: dict, inputs: HistoryInputs = MEASURED_INPUTS) -> str:
    by_key = {(row["version"], row["arm"]): row for row in history["rows"]}
    unavailable = {key: validate_unavailable_row(row, inputs)
                   for key, row in by_key.items() if 'unavailable' in row}
    lines = ([f"Benchmark specification: `{benchmark_spec.identity(history, required=True)}`", ""]
             if "benchmark_spec" in history else []) + [
        "| Release and arm | A leaked / gold B | A FP B | D FP B | R leaked / gold B | R FP B |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for key in expected_rows(inputs):
        row = by_key[key]
        if key in unavailable:
            lines.append(f"| `{row['version']}` `{row['arm']}` | N/A | N/A | N/A | N/A | N/A |")
            continue
        a, d, r = (row["layers"][layer] for layer in ("A", "D", "R"))
        lines.append(
            f"| `{row['version']}` `{row['arm']}` | "
            f"{a['leaked']:,} / {a['gold']:,} | {a['false_positive']:,} | "
            f"{d['false_positive']:,} | {r['leaked']:,} / {r['gold']:,} | {r['false_positive']:,} |"
        )
    if inputs.version >= 13:
        lines += ['', '| Release and arm | Refused A / D / R | Leaked on all processed A / D / R B |',
                  '| --- | ---: | ---: |']
        for key in expected_rows(inputs):
            row = by_key[key]
            if key in unavailable:
                lines.append(f"| `{row['version']}` `{row['arm']}` | N/A | N/A |")
                continue
            refusals = ' / '.join(str(row['refused'][layer]) for layer in ('A', 'D', 'R'))
            leaks = ' / '.join(f"{row['layers'][layer]['leaked']:,}" for layer in ('A', 'D', 'R'))
            lines.append(f"| `{row['version']}` `{row['arm']}` | {refusals} | {leaks} |")
    if inputs.version >= 16:
        lines += ["", "| Release and arm | Exact restores A / D / R | Valid manifests A / D / R | Restore failures A / D / R | Invalid manifests A / D / R |",
                  "| --- | ---: | ---: | ---: | ---: |"]
        for key in expected_rows(inputs):
            row = by_key[key]
            if key in unavailable:
                lines.append(f"| `{row['version']}` `{row['arm']}` | N/A | N/A | N/A | N/A |")
                continue
            counts = [" / ".join(f"{row['layers'][layer][field]:,}" for layer in ("A", "D", "R"))
                      for field in ("restore_exact", "valid_manifests", "restore_failures", "invalid_manifests")]
            lines.append(f"| `{row['version']}` `{row['arm']}` | " + " | ".join(counts) + " |")
    lines += [
        "",
        "These are layers A, D and R only, measured by the current harness against each "
        "release's own binary. Layer C release headlines are listed above. The "
        f"[committed measurement ledger]({inputs.path.name}) records "
        "binary and scorecard SHA-256 digests, arm and manifest semantics. "
        f"Generator v{inputs.version}, test corpus `{inputs.corpus_sha256[:12]}…`"
        + (", " if inputs.version < 13 else ". ")
        + (f"setup policy `{inputs.policy_sha256[:12]}…`." if inputs.version < 13 else
         f"Setup policies: v0.15.x `{inputs.policy_sha256[:12]}…`, "
         f"v0.16.0 `{V16_POLICY_SHA256[:12]}…`. v0.14.0 uses its legacy Kiji configurations."),
    ]
    for name in sorted({by_key[key]['evidence_file'] for key in unavailable}):
        lines += ['', 'v0.14.0 generated layers are unavailable: both fresh contract attempts '
                  'stopped at `payload_processing:payload`. No byte, refusal or restore counts '
                  f'are inferred. Layer C was measured separately. [Failure receipt]({name}).']
    return "\n".join(lines)


def apply(doc: str, history: dict, inputs: HistoryInputs = MEASURED_INPUTS) -> str:
    start, stop = doc.find(BEGIN), doc.find(END)
    if start < 0 or stop < start:
        raise HistoryError("benchmark document lacks the agentic adjacency markers")
    return doc[: start + len(BEGIN)] + "\n\n" + render(history, inputs) + "\n\n" + doc[stop:]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--record", type=Path, action="append", default=[])
    parser.add_argument('--unavailable-receipt', type=Path, action='append', default=[],
                        help='record a pinned legacy producer failure instead of numeric rows')
    parser.add_argument('--generator-version', type=int, default=GENERATOR_VERSION,
                        choices=sorted({12, 13, 15, 16, 17, 18, 19, GENERATOR_VERSION, agentic_layers.GENERATOR_VERSION}),
                        help='keep measured history frozen by default; select the current corpus for new measurements')
    args = parser.parse_args()
    try:
        inputs = HistoryInputs.for_version(args.generator_version)
        if args.record or args.unavailable_receipt:
            if args.check:
                raise HistoryError("--record and --check are mutually exclusive")
            history = {
                "schema_version": 1, "generator_version": inputs.version,
                "corpus_sha256": inputs.corpus_sha256, "contract_sha256": inputs.contract_sha256,
                "policy_sha256": inputs.policy_sha256, "rows": [],
            }
            for path in args.record:
                history["rows"].extend(rows_from_scorecard(path, inputs))
            for path in args.unavailable_receipt:
                history['rows'].extend(rows_from_unavailable_receipt(path, inputs))
            proven = [row for row in history['rows'] if 'benchmark_spec' in row]
            if proven:
                history['benchmark_spec'] = proven[0]['benchmark_spec']
            validate_history(history, inputs)
            temporary = inputs.path.with_suffix('.json.tmp')
            temporary.write_text(json.dumps(history, indent=2) + "\n", encoding="utf-8")
            temporary.replace(inputs.path)
        history = load_history(inputs.path, inputs)
        original = DOC.read_text(encoding="utf-8")
        rendered = apply(original, history, inputs)
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
