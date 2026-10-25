#!/usr/bin/env python3
"""Check a full, exact-candidate benchmark certificate from pinned evidence."""

from __future__ import annotations

import argparse
from dataclasses import replace
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

import agentic_layers as agentic
import bench_json
import benchmark_spec as specs
import gaze_bench_score as score
import scorecard_record as records
import subject_build
import stage_timing as stages
from capture_identity import digest, file_digest


class CertificationError(ValueError):
    def __init__(self, code: str) -> None:
        self.code = code
        super().__init__(code)


def pin(root: Path, value: dict) -> Path:
    try:
        name = value['file']
        path = root / name
        if (not isinstance(name, str) or Path(name).is_absolute() or '..' in Path(name).parts
                or path.is_symlink() or file_digest(path) != value['sha256']):
            raise CertificationError('evidence_integrity')
        return path
    except (OSError, KeyError, TypeError) as error:
        raise CertificationError('evidence_integrity') from error


def full_record(path: Path, spec: dict, revision: str | None = None) -> dict:
    header, observations = records._read(path)
    card = header['scorecard']
    specs.require_same(card, {'benchmark_spec': spec})
    if (spec['inputs']['protocol']['execution']['profile'] != 'full'
            or card.get('parameters', {}).get('profile') != 'full'
            or card.get('parameters', {}).get('max_documents') is not None):
        raise CertificationError('partial_population')
    if (card.get('gaze', {}).get('dirty') is not False
            or (revision is not None and card.get('gaze', {}).get('revision') != revision)):
        raise CertificationError('stale_candidate')
    parameters = card['parameters']
    execution = spec['inputs']['protocol']['execution']
    if (header['corpus_sha256'] != spec['inputs']['corpus_sha256']
            or card.get('layers', {}).get('generator') != spec['inputs']['generator']
            or header['schema_version'] not in spec['inputs']['record_schema']
            or parameters.get('sampling_seed') != spec['inputs']['seed']
            or parameters.get('warmup_count') != execution['warmups']
            or parameters.get('measured_repetitions') != execution['repetitions']):
        raise CertificationError('record_spec_mismatch')
    records.binary_sha256(path, required=True)
    populations = spec['inputs']['population']
    if set(populations) != {'C', 'A', 'D', 'R'}:
        raise CertificationError('partial_population')
    inventory = defaultdict(list)
    for row in header['documents']:
        inventory[row['layer']].append(row['id'])
    if set(inventory) != set(populations):
        raise CertificationError('partial_population')
    for layer, ids in inventory.items():
        if ({key: populations[layer][key] for key in ('documents', 'document_ids_sha256')}
                != {'documents': len(ids), 'document_ids_sha256': digest(sorted(ids))}):
            raise CertificationError('partial_population')
        descriptors = [row for row in header['documents'] if row['layer'] == layer]
        if populations[layer].get('descriptors_sha256') != specs.descriptor_digest(descriptors):
            raise CertificationError('canonical_document_mismatch')
    groups = defaultdict(list)
    for row in observations:
        groups[(row['layer'], row['config'])].append(row['document_id'])
    configs = card['parameters']['configs']
    if (not configs or len(configs) != len(set(configs))
            or set(groups) != {(layer, config) for layer in populations for config in configs}):
        raise CertificationError('partial_population')
    if any(sorted(ids) != sorted(inventory[layer]) for (layer, _), ids in groups.items()):
        raise CertificationError('partial_population')
    return card


def _contract(root: Path, number: int, spec: dict):
    if number == 1 and spec['inputs']['contracts']['v1'] == 'all-corpus-labels-v1':
        return score.SCORED_LABEL_CONTRACT_V1
    if type(number) is not int or number < 2:
        raise CertificationError('contract_identity')
    path = root / f'docs/reference/benchmarks/scored-labels-v{number}.json'
    if file_digest(path) != spec['inputs']['contracts'][f'scored-labels-v{number}']:
        raise CertificationError('contract_identity')
    return score.load_scored_label_contract(path)


def admit_capture(capture_path: Path, record_path: Path, spec: dict, harness: Path,
                  revision: str | None = None, *,
                  response_validation: dict = specs.CURRENT_VALIDATION) -> dict:
    """Admit execution once and bind its subject, metadata and metrics to the record."""
    original = full_record(record_path, spec, revision)
    captured = bench_json.load(capture_path)
    specs.require_same(captured, original)
    header, _ = records._read(record_path)
    if (header.get('response_validation') != response_validation
            or original.get('response_validation') != response_validation
            or captured.get('response_validation') != response_validation):
        raise CertificationError('response_validation_mismatch')
    canonical = score.scored_label_contract_report(score.SCORED_LABEL_CONTRACT_V1, ())
    block = original['scoring']['scored_label_contract']
    if (header.get('capture_protocol') != specs.CANONICAL_CAPTURE
            or original.get('capture_protocol') != specs.CANONICAL_CAPTURE
            or captured.get('capture_protocol') != specs.CANONICAL_CAPTURE
            or any(block.get(key) != canonical[key] for key in
                   ('id', 'version', 'file_sha256', 'excluded_labels', 'neutral_prediction_classes'))):
        raise CertificationError('noncanonical_capture')
    for key in ('gaze', 'subject_build', 'parameters', 'runner_provenance', 'dataset', 'scoring', 'layers', 'runs'):
        if captured.get(key) != original.get(key):
            raise CertificationError('capture_record_mismatch')
    try:
        stages.validate_execution(captured, capture_path, record_path, require_build=True)
        subject_build.validate(original, header_digest := records.binary_sha256(record_path, required=True))
        subject_build.validate(captured, header_digest)
    except ValueError as error:
        if str(error) == 'unqualified_subject_build':
            raise CertificationError('unqualified_subject_build') from error
        raise
    if 'release_capture' in captured:
        import past_capture
        past_capture.admit(captured, capture_path)
    block = captured['scoring']['scored_label_contract']
    contract = _contract(harness, block['version'], spec)
    # Display paths do not change contract semantics; all other fields are rederived.
    contract = replace(contract, path=block['file'])
    layer_contract = agentic.load_contract(harness)
    if layer_contract.sha256 != spec['inputs']['contracts']['generated']:
        raise CertificationError('contract_identity')
    header, _ = records._read(record_path)
    if header['layer_contract'] != records._contract_row(layer_contract):
        raise CertificationError('contract_identity')
    replay = records.rescore(record_path, contract, max_workers=1)
    if (captured.get('observation_record') != replay.get('observation_record')
            or captured['runs'] != replay['runs']
            or captured['layers'] != replay['layers']
            or captured['scoring'] != replay['scoring']
            or captured['dataset'] != replay['dataset']):
        raise CertificationError('capture_record_mismatch')
    return captured


def admit_scorecard(path: Path, record_path: Path, number: int,
                    spec: dict, harness: Path) -> dict:
    """Only reconstructed evidence may feed a gate or a history row."""
    card = bench_json.load(path)
    specs.require_same(card, {'benchmark_spec': spec})
    replay = records.rescore(record_path, _contract(harness, number, spec), max_workers=1)
    if card != replay:
        raise CertificationError('scorecard_record_mismatch')
    return replay


def check_pair(pair: dict, *, root: Path, harness: Path, spec: dict,
               candidate: str, mechanism: bool = False) -> dict:
    cards, record_paths = {}, {}
    for side in ('base', 'candidate'):
        arm = pair[side]
        if set(arm['contracts']) != {'1', '2'}:
            raise CertificationError('absent_contract')
        record_path = pin(root, arm['record'])
        capture_path = pin(root, arm['capture'])
        admit_capture(capture_path, record_path, spec, harness,
                      candidate if mechanism or side == 'candidate' else None)
        record_paths[side] = record_path
        for number in (2, 1):
            path = pin(root, arm['contracts'][str(number)])
            cards[side, number] = admit_scorecard(path, record_path, number, spec, harness)
    if mechanism and records.binary_sha256(record_paths['base'], required=True) != records.binary_sha256(record_paths['candidate'], required=True):
        raise CertificationError('mechanism_binary_mismatch')
    delta = pin(root, pair['policy_delta']) if 'policy_delta' in pair else None
    if mechanism and delta is None:
        raise CertificationError('absent_mechanism_delta')
    results = {}
    for number in (2, 1):
        # Execution was admitted once from the original capture. Each contract
        # was independently reconstructed from its pinned observation record.
        result = agentic.compare_layers(cards['base', number], cards['candidate', number],
                                         policy_delta=delta)
        if result['verdict'] != 'pass':
            raise CertificationError('gate_refused')
        results[str(number)] = result
    return results


def check(value: dict, *, root: Path, harness: Path, candidate: str,
          required_mechanisms: tuple[str, ...], dataset: Path | None = None) -> dict:
    try:
        if not re.fullmatch('[0-9a-f]{40}', candidate) or value['candidate'] != candidate:
            raise CertificationError('stale_candidate')
        if value['schema_version'] != 1 or value['kind'] != 'full-benchmark-certification':
            raise CertificationError('invalid_certification')
        spec = specs.validate(value['benchmark_spec'])
        try:
            specs.require_current(spec, harness, dataset=dataset)
        except specs.SpecError as error:
            raise CertificationError('harness_spec_mismatch') from error
        if not required_mechanisms or set(value['mechanisms']) != set(required_mechanisms):
            raise CertificationError('absent_mechanism')
        gates = check_pair(value['gain'], root=root, harness=harness, spec=spec, candidate=candidate)
        integrated_binary = records.binary_sha256(pin(root, value['gain']['candidate']['record']), required=True)
        for pair in value['mechanisms'].values():
            if records.binary_sha256(pin(root, pair['candidate']['record']), required=True) != integrated_binary:
                raise CertificationError('mechanism_binary_mismatch')
        mechanisms = {name: check_pair(value['mechanisms'][name], root=root, harness=harness,
                                       spec=spec, candidate=candidate, mechanism=True)
                      for name in required_mechanisms}
        return {'verdict': 'pass', 'candidate': candidate, 'benchmark_spec_id': spec['id'],
                'manifest_sha256': digest(value), 'contracts': gates, 'mechanisms': mechanisms}
    except (specs.SpecError, records.RecordError, agentic.LayerError, OSError, KeyError,
            TypeError, ValueError, score.ResponseValidationError) as error:
        if isinstance(error, CertificationError):
            raise
        raise CertificationError('invalid_certification_evidence') from error


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manifest', type=Path)
    parser.add_argument('--dataset', type=Path, help='canonical Dataiku holdout; defaults to the harness cache')
    parser.add_argument('--candidate', required=True, help='exact integrated Git HEAD')
    parser.add_argument('--required-mechanism', action='append', required=True)
    args = parser.parse_args(argv)
    try:
        result = check(json.loads(args.manifest.read_text()), root=args.manifest.resolve().parent,
                       harness=Path(__file__).resolve().parents[2], candidate=args.candidate,
                       required_mechanisms=tuple(args.required_mechanism), dataset=args.dataset)
        print(json.dumps(result, indent=2))
        return 0
    except (CertificationError, OSError, ValueError) as error:
        code = error.code if isinstance(error, CertificationError) else 'invalid_certification'
        print(json.dumps({'verdict': 'refuse', 'code': code}))
        return 2


if __name__ == '__main__':
    sys.exit(main())
