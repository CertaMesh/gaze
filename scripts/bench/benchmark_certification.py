#!/usr/bin/env python3
"""Check a full, exact-candidate benchmark certificate from pinned evidence."""

from __future__ import annotations

import argparse
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
        if populations[layer] != {'documents': len(ids), 'document_ids_sha256': digest(sorted(ids))}:
            raise CertificationError('partial_population')
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
    if number == 1:
        return score.SCORED_LABEL_CONTRACT_V1
    path = root / f'docs/reference/benchmarks/scored-labels-v{number}.json'
    if file_digest(path) != spec['inputs']['contracts'][f'scored-labels-v{number}']:
        raise CertificationError('contract_identity')
    return score.load_scored_label_contract(path)


def check_pair(pair: dict, *, root: Path, harness: Path, spec: dict,
               candidate: str, mechanism: bool = False) -> dict:
    cards, record_paths = {}, {}
    for side in ('base', 'candidate'):
        arm = pair[side]
        if set(arm['contracts']) != {'1', '2'}:
            raise CertificationError('absent_contract')
        record_path = pin(root, arm['record'])
        original = full_record(record_path, spec, candidate if mechanism or side == 'candidate' else None)
        capture_path = pin(root, arm['capture'])
        captured = bench_json.load(capture_path)
        specs.require_same(captured, original)
        stages.validate_execution(captured, capture_path, record_path)
        if (captured['gaze'] != original['gaze']
                or agentic._layer_identity(captured) != agentic._layer_identity(original)
                or captured.get('observation_record', {}).get('sha256') != file_digest(record_path)):
            raise CertificationError('capture_record_mismatch')
        record_paths[side] = record_path
        for number in (2, 1):
            path = pin(root, arm['contracts'][str(number)])
            card = bench_json.load(path)
            specs.require_same(card, {'benchmark_spec': spec})
            contract = _contract(harness, number, spec)
            replay = records.rescore(record_path, contract, max_workers=1)
            config = agentic.production_config(card)
            if (agentic._layer_identity(card) != agentic._layer_identity(replay)
                    or agentic.layer_totals(card, config) != agentic.layer_totals(replay, config)
                    or card['gaze'] != replay['gaze']
                    or card.get('observation_record', {}).get('sha256') != file_digest(record_path)):
                raise CertificationError('scorecard_record_mismatch')
            cards[side, number] = card
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
          required_mechanisms: tuple[str, ...]) -> dict:
    try:
        if not re.fullmatch('[0-9a-f]{40}', candidate) or value['candidate'] != candidate:
            raise CertificationError('stale_candidate')
        if value['schema_version'] != 1 or value['kind'] != 'full-benchmark-certification':
            raise CertificationError('invalid_certification')
        spec = specs.validate(value['benchmark_spec'])
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
            TypeError, ValueError) as error:
        if isinstance(error, CertificationError):
            raise
        raise CertificationError('invalid_certification_evidence') from error


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manifest', type=Path)
    parser.add_argument('--candidate', required=True, help='exact integrated Git HEAD')
    parser.add_argument('--required-mechanism', action='append', required=True)
    args = parser.parse_args(argv)
    try:
        result = check(json.loads(args.manifest.read_text()), root=args.manifest.resolve().parent,
                       harness=Path(__file__).resolve().parents[2], candidate=args.candidate,
                       required_mechanisms=tuple(args.required_mechanism))
        print(json.dumps(result, indent=2))
        return 0
    except (CertificationError, OSError, ValueError) as error:
        code = error.code if isinstance(error, CertificationError) else 'invalid_certification'
        print(json.dumps({'verdict': 'refuse', 'code': code}))
        return 2


if __name__ == '__main__':
    sys.exit(main())
