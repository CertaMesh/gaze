#!/usr/bin/env python3
"""Plan or resume current/previous release measurement; older backfills are optional."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

import benchmark_spec as specs
import agentic_layers as agentic
import benchmark_certification as certification
import render_benchmark_doc as render
import rescore_past_release as past
import scorecard_record as records
import gaze_bench_score as score
from capture_identity import digest, file_digest


class HistoryError(ValueError):
    def __init__(self, code: str) -> None:
        self.code = code
        super().__init__(code)


def atomic(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + '.tmp')
    temporary.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')
    temporary.replace(path)


def selection(plan: dict, older: bool) -> list[dict]:
    versions = [plan['current'], plan['previous']]
    if versions[0] == versions[1]:
        raise HistoryError('invalid_history_selection')
    jobs = plan['releases']
    if (not isinstance(jobs, list) or len({row['version'] for row in jobs}) != len(jobs)
            or any(not re.fullmatch(r'v\d+\.\d+\.\d+', row['version']) for row in jobs)):
        raise HistoryError('invalid_history_selection')
    by_version = {row['version']: row for row in jobs}
    if not set(versions) <= set(by_version):
        raise HistoryError('required_release_missing')
    return [by_version[version] for version in versions] + (
        [row for row in jobs if row['version'] not in versions] if older else [])


def release_validation(job: dict) -> dict:
    expected = specs.RELEASE_PROTOCOLS.get(job['version'])
    if expected is None or job.get('response_validation') != expected:
        raise HistoryError('release_protocol_mismatch')
    return expected


def subject_identity(header: dict) -> dict:
    card = header['scorecard']
    return {'revision': card['gaze']['revision'], 'binary_sha256': header.get('binary_sha256'),
            'policy_sha256': card['parameters'].get('policy_sha256'),
            'model_bundles': card.get('runner_provenance', {}).get('model_bundles', [])}


def subject_matches(record: Path, job: dict) -> bool:
    header, _ = records._read(record)
    expected = job.get('subject')
    return (isinstance(expected, dict)
            and re.fullmatch('[0-9a-f]{40}', str(expected.get('revision', ''))) is not None
            and re.fullmatch('[0-9a-f]{64}', str(expected.get('binary_sha256', ''))) is not None
            and subject_identity(header) == expected)


def verified(row: dict, folder: Path, spec: dict, job: dict, *, dataset: Path | None = None) -> bool:
    if (row.get('status') != 'available' or row.get('benchmark_spec_id') != spec['id']
            or row.get('job_sha256') != digest(job)):
        return False
    try:
        validation = release_validation(job)
        artifacts = row['artifacts']
        required = {'capture.json', 'observations-v1.jsonl.gz', 'scorecard-v1.json', 'scorecard-v2.json'}
        if not required <= set(artifacts):
            return False
        for name, sha in artifacts.items():
            if Path(name).name != name or file_digest(folder / name) != sha:
                return False
        specs.require_current(spec, past.HARNESS_ROOT, dataset=dataset)
        record = folder / 'observations-v1.jsonl.gz'
        certification.admit_capture(folder / 'capture.json', record, spec, past.HARNESS_ROOT,
                                    response_validation=validation)
        for number in (2, 1):
            certification.admit_scorecard(folder / f'scorecard-v{number}.json', record,
                                         number, spec, past.HARNESS_ROOT)
        if not subject_matches(folder / 'observations-v1.jsonl.gz', job):
            return False
    except (OSError, ValueError, KeyError, TypeError, score.ResponseValidationError):
        return False
    return True



def scored_rows(folder: Path, spec: dict) -> dict:
    result = {}
    for number in (2, 1):
        card = certification.admit_scorecard(folder / f'scorecard-v{number}.json',
                                            folder / 'observations-v1.jsonl.gz', number,
                                            spec, past.HARNESS_ROOT)
        result[str(number)] = agentic.layer_totals(card, agentic.production_config(card))
    return result

def measure(job: dict, folder: Path, spec: dict) -> dict:
    validation = release_validation(job)
    argv = job['args']
    if (not isinstance(argv, list) or not all(isinstance(arg, str) for arg in argv)
            or any(arg.split('=')[0] in {'--output', '--max-documents', '--cross-check',
                                         '--scored-labels', '--agentic-scored-labels'} for arg in argv)):
        raise HistoryError('invalid_history_job')
    folder.mkdir(parents=True, exist_ok=True)
    output = folder / 'capture.json'
    args = past.parse_args([*argv, '--output', str(output)])
    if (not args.agentic_layers or args.max_documents is not None or args.cross_check
            or args.scored_labels is not None or args.agentic_scored_labels is not None):
        raise HistoryError('partial_population')
    if specs.response_validation(args) != validation:
        raise HistoryError('release_protocol_mismatch')
    past.run(args)
    record = folder / 'observations-v1.jsonl.gz'
    certification.admit_capture(output, record, spec, past.HARNESS_ROOT,
                                response_validation=validation)
    if not subject_matches(record, job):
        raise HistoryError('subject_identity_mismatch')
    # One subject execution; contracts are derived exclusively from its record.
    versions = sorted(int(name.rsplit('v', 1)[1]) for name in spec['inputs']['contracts']
                      if name.startswith('scored-labels-v'))
    names = [record.name, output.name]
    for number in sorted({1, 2, *versions}):
        contract = certification._contract(past.HARNESS_ROOT, number, spec)
        path = folder / f'scorecard-v{number}.json'
        atomic(path, records.rescore(record, contract, max_workers=1))
        names.append(path.name)
    return {'status': 'available', 'benchmark_spec_id': spec['id'],
            'job_sha256': digest(job),
            'artifacts': {name: file_digest(folder / name) for name in names}}


def run(plan: dict, output: Path, *, execute: bool = False, older: bool = False,
        measure_job=None, dataset: Path | None = None) -> dict:
    spec = specs.validate(plan['benchmark_spec'])
    jobs = selection(plan, older)
    state_path = output / 'history.json'
    state = json.loads(state_path.read_text()) if state_path.exists() else {
        'schema_version': 1, 'benchmark_spec': spec, 'releases': {}}
    specs.require_same(state, {'benchmark_spec': spec})
    if execute:
        try:
            specs.require_current(spec, past.HARNESS_ROOT, dataset=dataset)
        except specs.SpecError as error:
            raise HistoryError('harness_spec_mismatch') from error
    for row in state['releases'].values():
        row['required'] = False
    measure_job = measure if measure_job is None else measure_job
    selected = {job['version'] for job in jobs}
    # Retained optional evidence is displayed too; validate it without scheduling a backfill.
    retained = {job['version']: job for job in selection(plan, True)}
    for version in set(state['releases']) - selected:
        job = retained.get(version)
        folder = output / version
        row = state['releases'][version]
        if row.get('status') != 'available':
            continue
        try:
            if job is None or not verified(row, folder, spec, job, dataset=dataset):
                raise HistoryError('evidence_invalid')
            row['contracts'] = scored_rows(folder, spec)
            row['subject'] = job['subject']
            row['response_validation'] = release_validation(job)
        except (OSError, ValueError, KeyError, TypeError, score.ResponseValidationError):
            state['releases'][version] = {'status': 'unavailable', 'reason': 'evidence_invalid', 'required': False}
    for job in jobs:
        version = job['version']
        folder = output / version
        required = version in (plan['current'], plan['previous'])
        row = state['releases'].get(version, {})
        if verified(row, folder, spec, job, dataset=dataset):
            row['required'] = required
        else:
            row = {'status': 'unavailable', 'reason': 'not_measured', 'required': required}
            if execute:
                try:
                    row = {**measure_job(job, folder, spec), 'required': required}
                    if not verified(row, folder, spec, job, dataset=dataset):
                        raise HistoryError('evidence_invalid')
                except (Exception, SystemExit):
                    # A failed optional backfill cannot block required release evidence.
                    # Never put external error text, paths or values into public history.
                    row = {'status': 'unavailable', 'reason': 'measurement_refused', 'required': required}
        if row['status'] == 'available':
            try:
                row['contracts'] = scored_rows(folder, spec)
                row['subject'] = job['subject']
                row['response_validation'] = release_validation(job)
            except (OSError, ValueError, KeyError, TypeError, score.ResponseValidationError):
                row = {'status': 'unavailable', 'reason': 'evidence_invalid', 'required': required}
        state['releases'][version] = row
        atomic(state_path, state)
    state['required_complete'] = all(state['releases'][v]['status'] == 'available'
                                     for v in (plan['current'], plan['previous']))
    atomic(state_path, state)
    (output / 'history.md').write_text(render.render_spec_history(state), encoding='utf-8')
    return state


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('plan', type=Path, help='pinned spec, current/previous versions and rescore_past_release argument lists')
    parser.add_argument('--output', type=Path, required=True, help='separate directory per immutable specification')
    parser.add_argument('--dataset', type=Path, help='canonical Dataiku holdout; defaults to the harness cache')
    parser.add_argument('--execute', action='store_true', help='perform missing measurements; default only writes a plan')
    parser.add_argument('--older', action='store_true', help='include optional older releases off the PR path')
    args = parser.parse_args(argv)
    try:
        state = run(json.loads(args.plan.read_text()), args.output, execute=args.execute, older=args.older, dataset=args.dataset)
        print(json.dumps({'required_complete': state['required_complete']}))
        return 0 if not args.execute or state['required_complete'] else 2
    except (OSError, ValueError, KeyError, TypeError) as error:
        code = error.code if isinstance(error, (HistoryError, specs.SpecError)) else 'invalid_history_plan'
        print(json.dumps({'code': code}))
        return 2


if __name__ == '__main__':
    sys.exit(main())
