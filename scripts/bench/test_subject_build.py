"""Historical command construction; every executable here is synthetic."""

import copy
import json
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

import subject_build as build

ROOT = Path(__file__).resolve().parents[2]
RELEASES = json.loads((Path(__file__).parent / 'fixtures/release-build-features.json').read_text())


def checkout(root, release):
    package = root / 'crates/gaze-recognizers'
    (package / 'examples').mkdir(parents=True)
    (package / 'Cargo.toml').write_text('[features]\n' + '\n'.join(
        f'{json.dumps(name)} = {json.dumps(values)}' for name, values in release['features'].items()))
    (package / 'examples/clean_for_bench.rs').write_text('\n'.join(
        f'"{config}" => BenchConfig::Synthetic,' for config in release['configs']))


@pytest.mark.parametrize('release', RELEASES, ids=lambda r: r['tag'])
def test_documented_release_build_commands(tmp_path, monkeypatch, release):
    checkout(tmp_path, release)
    configs = ('rule-floor-extended', 'pass2-ner', 'full-stack-kiji-resolve') if release['tag'] == 'v0.14.0' else ('policy-file',)
    feature = 'safety-net-kiji' if release['tag'] == 'v0.14.0' else 'safety-net-nym'
    binary = tmp_path / 'synthetic'
    binary.touch()
    command = Mock(return_value=SimpleNamespace(stdout=json.dumps(dict(reason='compiler-artifact',
        target=dict(name='clean_for_bench', kind=['example']), executable=str(binary)))))
    monkeypatch.setattr(build.subprocess, 'run', command)
    assert build.cargo_build_binary(tmp_path, configs, release=True) == binary
    argv = command.call_args.args[0]
    assert argv[argv.index('--features') + 1] == feature
    assert '--release' in argv and '--locked' in argv
    assert build.build_features(tmp_path, configs) == [feature]
    for config in release['configs']:
        assert set(build.build_features(tmp_path, [config])) <= release['features'].keys()


def test_current_runner_feature_requirements():
    assert build.build_features(ROOT, ['rule-floor-extended', 'pass2-ner']) == []
    assert build.build_features(ROOT, ['policy-file']) == ['safety-net-nym']
    assert build.build_features(ROOT, ['full-stack-nym-resolve', 'full-stack-nym-redact']) == ['safety-net-nym']
    assert build.build_features(ROOT, ['pass3-opf']) == ['safety-net-openai']


@pytest.mark.parametrize('change', ['missing_feature', 'missing_config', 'unknown_config'])
def test_target_checkout_rejects_unavailable_build_requirements(tmp_path, change):
    release = copy.deepcopy(RELEASES[0])
    if change == 'missing_feature':
        release['features'].pop('safety-net-kiji')
    elif change == 'missing_config':
        release['configs'].remove('full-stack-kiji-resolve')
    checkout(tmp_path, release)
    configs = ['unknown-config'] if change == 'unknown_config' else ['full-stack-kiji-resolve']
    with pytest.raises(ValueError, match='unsupported_'):
        build.build_features(tmp_path, configs)


def test_qualified_build_records_selected_features(tmp_path, monkeypatch):
    checkout(tmp_path, RELEASES[0])
    binary = tmp_path / 'synthetic'
    binary.touch()
    monkeypatch.setattr(build.score, 'git_metadata', lambda root: {'revision': 'a' * 40, 'dirty': False})
    configs = ('rule-floor-extended', 'pass2-ner', 'full-stack-kiji-resolve')
    _, receipt = build.build(tmp_path, configs, release=False, builder=lambda *a, **kw: binary)
    assert receipt['features'] == ['safety-net-kiji']


@pytest.mark.parametrize('features', [None, [], ['safety-net-nym'], ['safety-net-kiji', 'safety-net-nym']])
def test_receipt_refuses_features_that_cannot_run_declared_configs(features):
    card = dict(gaze={'revision': 'a' * 40, 'dirty': False},
                parameters=dict(binary_profile='release', configs=['full-stack-kiji-resolve']))
    receipt = dict(version=1, mode='producer-build', source=card['gaze'], binary_sha256='b' * 64,
                   profile='release', configs=['full-stack-kiji-resolve'], features=['safety-net-kiji'])
    card['subject_build'] = receipt
    build.validate(card)
    if features is None:
        receipt.pop('features')
    else:
        receipt['features'] = features
    with pytest.raises(ValueError, match='unqualified_subject_build'):
        build.validate(card)
