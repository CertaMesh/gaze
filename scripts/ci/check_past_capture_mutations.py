#!/usr/bin/env python3
"""Remove release-cache admission guards and require executed regression failures."""

import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
SOURCE = 'scripts/bench/past_capture.py'
TEST = 'scripts/bench/test_past_capture.py'
BINDINGS = {
    'format': "key.get('format') != FORMAT",
    'binary': "key.get('binary_sha256') != header.get('binary_sha256')",
    'release_marker': "card.get('release_capture') != provenance(key)",
    'cache_marker': "card.get('capture_cache') != cache.provenance(key)",
    'header_release': "original.get('release_capture') != card.get('release_capture')",
    'header_cache': "original.get('capture_cache') != card.get('capture_cache')",
    'spec': "benchmark['spec_id'] != specs.identity(card, required=True)",
    'harness': "benchmark['harness_revision'] != card['runner_provenance']['harness_revision']",
    'build': "subject['build'] != card['subject_build']",
    'parameters': "execution['parameters'] != card['parameters']",
    'capture_protocol': "execution['capture_protocol'] != card['capture_protocol']",
    'response_validation': "execution['response_validation'] != card['response_validation']",
    'policy': "execution['policy_dependencies'] != card['runner_provenance']['policy_dependencies']",
    'models': "execution['model_bundles'] != card['runner_provenance']['model_bundles']",
    'tag_format': "not re.fullmatch(r'v\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?', subject['release_tag'])",
    'tag_match': "(version is not None and subject['release_tag'] != version)",
    'measurement': "card.get(field) != original.get(field)",
}
PROBES = [(name, SOURCE, old, 'False', f'{TEST}::test_each_binding_guard_rejects_independently[{name}]')
          for name, old in BINDINGS.items()]
PROBES += [
    ('history-tag', 'scripts/bench/benchmark_history.py',
     "or header['scorecard']['release_capture'].get('release_tag') == job['version']", 'or True',
     f'{TEST}::test_history_subject_rejects_another_tag_at_the_same_revision'),
    ('loaded-policy', SOURCE, "if identity.file_digest(policy_path) != parameters['policy_sha256']:", 'if False:',
     f'{TEST}::test_policy_changed_after_loading_is_uncacheable'),
    ('configuration', SOURCE, "if parameters['configs'] != ['policy-file'] or args.policy is None:", 'if False:',
     f'{TEST}::test_incomplete_input_closure_is_uncacheable[unsupported_config]'),
    ('backend', SOURCE, "if policy.get('safety_net', {}).get('backend') not in (None, 'nym'):", 'if False:',
     f'{TEST}::test_incomplete_input_closure_is_uncacheable[unsupported_backend]'),
    ('capture-record', SOURCE, "if card.get('observation_record') != captured.get('observation_record'):", 'if False:',
     f'{TEST}::test_derived_card_cannot_switch_execution_record'),
    ('features', SOURCE, "    subject_build.validate(card, header['binary_sha256'])", '    pass',
     f'{TEST}::test_each_binding_guard_rejects_independently[features]'),
    ('metrics', SOURCE, 'if records.rescore(record, contract, max_workers=1) != card:', 'if False:',
     f'{TEST}::test_rebound_execution_evidence_cannot_change_recorded_metrics'),
    ('certification', 'scripts/bench/benchmark_certification.py', '        past_capture.admit(captured, capture_path)', '        pass',
     f'{TEST}::test_certification_requires_release_key_even_when_execution_hashes_match'),
    ('renderer', 'scripts/bench/render_benchmark_doc.py', '            past_capture.admit(scorecard, scorecard_path, version=version)', '            pass',
     f'{TEST}::test_renderer_admits_verified_replay_and_preserves_original_date'),
    ('history-proof', 'scripts/bench/benchmark_history.py', "        names += ['run-evidence.json', past_capture.KEY_FILE]", '        pass',
     f'{TEST}::test_history_measure_retains_key_and_execution_proof'),
    ('tag-required', SOURCE, "if not requested or not re.fullmatch(r'v\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?', requested):", 'if False:',
     f'{TEST}::test_release_tag_requires_an_exact_release_commit[None-{"a" * 40}-{"a" * 40}]'),
    ('tag-commit', SOURCE, 'if resolved != revision:', 'if False:',
     f'{TEST}::test_release_tag_requires_an_exact_release_commit[v0.17.0-{"b" * 40}-{"a" * 40}]'),
]


def main():
    if subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip():
        raise SystemExit('commit source before mutation probes')
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    logs = ROOT / 'target/past-capture-mutations'
    logs.mkdir(parents=True, exist_ok=True)
    summary = {'head': head, 'killed': [], 'survived': []}
    for name, source, old, new, test in PROBES:
        path = ROOT / source
        original = path.read_bytes()
        if original.decode().count(old) != 1:
            raise RuntimeError('mutation target must be unique: ' + name)
        try:
            path.write_text(original.decode().replace(old, new, 1))
            result = subprocess.run([sys.executable, '-B', '-m', 'pytest', '-q', '--color=no', test],
                cwd=ROOT, env={**os.environ, 'PYTHONDONTWRITEBYTECODE': '1'},
                text=True, capture_output=True)
            output = result.stdout + result.stderr
            (logs / (name + '.log')).write_text('head ' + head + '\n' + output)
            killed = result.returncode == 1 and 'FAILED ' in output and 'ERROR collecting' not in output
            summary['killed' if killed else 'survived'].append(name)
            print(name + (': killed' if killed else ': SURVIVED'), flush=True)
        finally:
            path.write_bytes(original)
    (logs / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    if summary['survived']:
        raise SystemExit(1)
    print(str(len(summary['killed'])) + ' admission mutations killed at ' + head)


if __name__ == '__main__':
    main()
