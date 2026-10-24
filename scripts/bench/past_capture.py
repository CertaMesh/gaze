"""Exact release-subject identity and offline admission for retained captures.

The producer/build tools and local cache are trusted, as for subject_build.
Hashes detect changed inputs and damaged artifacts; they are not attestations
against a party that fabricates the entire evidence set consistently.
"""

from __future__ import annotations

import json
from pathlib import Path
import re
import subprocess
import tomllib

import bench_json
import agentic_layers as agentic
import benchmark_cache as cache
import benchmark_spec as specs
import capture_identity as identity
import scorecard_record as records
import stage_timing as stages
import subject_build

FORMAT = 'past-release-capture-v1'
KEY_FILE = 'release-capture-key.json'


def provenance(key):
    return {'format': FORMAT, 'key_file': KEY_FILE, 'key_sha256': cache.key_digest(key)}


def write_key(directory, key):
    path = directory / KEY_FILE
    temporary = path.with_suffix('.json.tmp')
    temporary.write_text(json.dumps(key, sort_keys=True, allow_nan=False) + '\n')
    temporary.replace(path)


def release_tag(root, requested, revision):
    if not requested or not re.fullmatch(r'v\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?', requested):
        raise ValueError('release_tag_required')
    resolved = subprocess.check_output(['git', 'rev-parse', '--verify', f'refs/tags/{requested}^{{commit}}'],
                                       cwd=root, text=True, stderr=subprocess.PIPE).strip()
    if resolved != revision:
        raise ValueError('release_tag_mismatch')
    return requested


def key(*, harness_root, release_root, args, binary, probe, receipt, spec,
        parameters, bundles, environment, document_ids):
    """Read live inputs again on every call; never trust a prior inventory."""
    import run_no_opf_benchmark as runner
    source = records.score.git_metadata(release_root)
    subject_build.validate({'subject_build': receipt, 'gaze': source, 'parameters': parameters},
                           identity.file_digest(binary))
    tag = release_tag(release_root, args.release_tag, source['revision'])
    harness, reason = runner.source_tree_identity(harness_root)
    if harness is None:
        raise ValueError(reason)
    # Initially qualify the same policy-driven native producer as the base cache.
    # Older config-specific model defaults do not have a complete input closure.
    if parameters['configs'] != ['policy-file'] or args.policy is None:
        raise ValueError('unsupported_release_cache_configuration')
    policy_path = args.policy.resolve()
    if identity.file_digest(policy_path) != parameters['policy_sha256']:
        raise ValueError('policy_changed_after_loading')
    policy = tomllib.loads(policy_path.read_text())
    if policy.get('safety_net', {}).get('backend') not in (None, 'nym'):
        raise ValueError('unsupported_release_cache_backend')
    files = {'detector_binary': binary, 'validator_binary': probe, 'policy': policy_path,
             'dataset': runner.repo_path(harness_root, args.dataset),
             'negative_corpus': harness_root / runner.NEGATIVE_CORPUS}
    models = {}
    for section in ('ner', 'dob_judge'):
        config = policy.get(section, {})
        if section == 'dob_judge' and config.get('enabled') is not True:
            continue
        if config.get('model_dir'):
            path = Path(config['model_dir']).expanduser()
            models[section] = path if path.is_absolute() else release_root / path
    if policy.get('safety_net', {}).get('backend') == 'nym':
        path = Path(policy['safety_net']['nym']['model_dir']).expanduser()
        models['nym'] = path if path.is_absolute() else release_root / path
    for value in args.model_env:
        name, path = value.split('=', 1)
        model_path = Path(path).expanduser()
        models['env:' + name] = model_path if model_path.is_absolute() else release_root / model_path
    for value in args.model_bundle:
        name, path, _ = value.split('=')
        models['bundle:' + name] = Path(path).expanduser()
    # The detached validator belongs to the measuring checkout, never the release.
    validator_sources = harness_root / 'scripts/bench/validator_recall_probe'
    validator = {'source': identity.tree(validator_sources),
                 'gaze': harness, 'binary': identity.file_digest(probe)}
    dependencies = agentic.policy_dependency_provenance(policy_path, release_root, policy)
    inputs = identity.snapshot(files=files, models=models, environment=environment,
                               runtime=identity.native_runtime({'detector': binary, 'validator': probe}))
    return {'format': FORMAT, 'binary_sha256': inputs['files']['detector_binary'],
            'benchmark': {'spec_id': specs.validate(spec)['id'],
                          'harness_revision': harness['revision'], 'validator': validator},
            'subject': {'release_tag': tag, 'build': receipt},
            'execution': {'parameters': parameters, 'inputs': inputs,
                          'capture_protocol': specs.capture_protocol(args),
                          'response_validation': specs.response_validation(args),
                          'document_ids_sha256': identity.digest(list(document_ids)),
                          'policy_dependencies': dependencies, 'model_bundles': bundles}}


def validate_binding(card, header, key, *, version=None):
    """Bind the full key, original measurement and producer to the retained header."""
    original = header['scorecard']
    if (key.get('format') != FORMAT
            or card.get('release_capture') != provenance(key)
            or card.get('capture_cache') != cache.provenance(key)
            or original.get('release_capture') != card.get('release_capture')
            or original.get('capture_cache') != card.get('capture_cache')
            or key.get('binary_sha256') != header.get('binary_sha256')):
        raise ValueError('release_capture_binding')
    benchmark, subject, execution = key['benchmark'], key['subject'], key['execution']
    if (benchmark['spec_id'] != specs.identity(card, required=True)
            or benchmark['harness_revision'] != card['runner_provenance']['harness_revision']
            or subject['build'] != card['subject_build']
            or execution['parameters'] != card['parameters']
            or execution['capture_protocol'] != card['capture_protocol']
            or execution['response_validation'] != card['response_validation']
            or execution['policy_dependencies'] != card['runner_provenance']['policy_dependencies']
            or execution['model_bundles'] != card['runner_provenance']['model_bundles']
            or not re.fullmatch(r'v\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?', subject['release_tag'])
            or (version is not None and subject['release_tag'] != version)):
        raise ValueError('release_capture_subject')
    for field in ('generated_at', 'gaze', 'subject_build', 'runner_provenance', 'parameters',
                  'benchmark_spec', 'capture_protocol', 'response_validation'):
        if card.get(field) != original.get(field):
            raise ValueError('release_capture_measurement')
    subject_build.validate(card, header['binary_sha256'])


def admit(card, path, *, version=None):
    """Offline admission uses frozen measurement inputs, never the publication tree."""
    if path is None:
        raise ValueError('release capture requires run evidence and a scorecard path')
    try:
        evidence = json.loads((path.parent / 'run-evidence.json').read_text())
        captured_name = evidence['artifacts']['scorecard']['file']
        if not isinstance(captured_name, str) or Path(captured_name).name != captured_name:
            raise ValueError('invalid_release_capture_path')
        captured_path = path.parent / captured_name
        captured = json.loads(captured_path.read_text())
        stages.validate_execution(captured, captured_path, require_build=True)
        if bench_json.load(path) != card:
            raise ValueError('release_capture_artifact')
        if card.get('observation_record') != captured.get('observation_record'):
            raise ValueError('release_capture_record')
        key = json.loads((path.parent / KEY_FILE).read_text())
        record = path.parent / card['observation_record']['file']
        header, _ = records._read(record)
        validate_binding(card, header, key, version=version)
        # Reconstruct metrics as well as checking hashes; changing a card and its
        # timing sidecar together cannot change the recorded observations.
        contract_row = card['scoring']['scored_label_contract']
        from dataclasses import replace
        import benchmark_certification as certification
        root = Path(__file__).resolve().parents[2]
        contract = certification._contract(root, contract_row['version'], card['benchmark_spec'])
        contract = replace(contract, path=contract_row['file'])
        if records.rescore(record, contract, max_workers=1) != card:
            raise ValueError('release_capture_replay')
    except (OSError, KeyError, TypeError, ValueError, AttributeError) as error:
        raise ValueError('unverified_release_capture') from error
