"""Model-free checks for the exact labelled-ID mechanism ablation."""
import copy
import hashlib
import json
from pathlib import Path
import tomllib

import pytest

import agentic_layers as agentic
import labelled_record_ids_arm as arm

ROOT = arm.ROOT
SOURCE = '''[policy]
version="0.1.0"
[policy.rulepacks]
bundled=["core", "locale-de", "secrets"]
paths=["existing.toml"]
[ner]
model_dir="reserved-model"
threshold=0.3
[safety_net]
backend="nym"
[safety_net.nym]
labels=["PERSON"]
[[rule]]
kind="default"
action="tokenize"
'''


@pytest.fixture
def project(tmp_path, monkeypatch):
    repo = tmp_path / "repo"
    for name in (arm.CORE, arm.DELTA):
        target = repo / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((ROOT / name).read_bytes())
    monkeypatch.setattr(arm, "ROOT", repo)
    return repo


def test_partition_preserves_all_definitions_and_locale_metadata():
    source = (ROOT / arm.CORE).read_text()
    base_raw, new_raw = arm.split_rulepack(source)
    core, base, new = map(tomllib.loads, (source, base_raw, new_raw))
    assert base['recognizers'] + new['recognizers'] == core['recognizers']
    assert {row['id'] for row in new['recognizers']} == arm.NEW_IDS
    assert not arm.NEW_IDS & {row['id'] for row in base['recognizers']}
    assert base['locale'] == core['locale']
    assert 'locale' not in new
    assert new['rulepack_id'] == 'gaze-labelled-record-ids'
    for row in new['recognizers']:
        assert row['match']['complete_labelled_value']
        assert row['match']['labelled_value_mode'] == 'field_owned'


@pytest.mark.parametrize('mutation', ['missing', 'duplicate_new', 'duplicate_old'])
def test_refuses_missing_or_duplicate_recognizers(mutation):
    source = (ROOT / arm.CORE).read_text()
    if mutation == 'missing':
        source = source.replace('id = "record_id.labelled"', 'id = "unexpected.labelled"')
    else:
        header, *blocks = source.split('\n[[recognizers]]')
        source += '\n[[recognizers]]' + blocks[-1 if mutation == 'duplicate_new' else 0]
    with pytest.raises(ValueError, match='exactly once'):
        arm.split_rulepack(source)


def test_only_mechanism_rulepack_slot_changes(project, tmp_path):
    output = tmp_path / 'arms'
    base, candidate = arm.prepare(SOURCE, output)
    expected = copy.deepcopy(tomllib.loads(SOURCE))
    expected['policy']['rulepacks']['bundled'].remove('core')
    expected['policy']['rulepacks']['paths'].append(str(output / 'base-core.toml'))
    assert base == expected
    expected['policy']['rulepacks']['paths'].append(arm.RULEPACK_PATH.as_posix())
    assert candidate == expected
    for side, parsed in [('base', base), ('candidate', candidate)]:
        assert tomllib.loads((output / f'{side}.toml').read_text()) == parsed
    def card(side):
        path = output / f'{side}.toml'
        sha = hashlib.sha256(path.read_bytes()).hexdigest()
        files = {'policy.rulepacks.paths[2]': 'a' * 64} if side == 'candidate' else {}
        return {'parameters': {'policy_sha256': sha}, 'runner_provenance': {
            'policy': {'path': str(path), 'sha256': sha},
            'policy_dependencies': {'files': files}, 'model_bundles': []}}
    accepted, _, _, allowed = agentic._policy_delta_comparison(card('base'), card('candidate'), output / 'delta.toml')
    assert accepted
    assert allowed == {'policy.rulepacks.paths[2]'}
    receipt = json.loads((output / 'preparation.json').read_text())
    assert receipt['recognizer_ids'] == sorted(arm.NEW_IDS)
    assert receipt['measurement_pending']
    for name, sha in receipt['outputs'].items():
        assert hashlib.sha256(Path(name).read_bytes()).hexdigest() == sha


@pytest.mark.parametrize('bundled', ['["secrets"]', '["core", "core"]'])
def test_refuses_ambiguous_core_policy(project, tmp_path, bundled):
    source = SOURCE.replace('["core", "locale-de", "secrets"]', bundled)
    with pytest.raises(ValueError, match='exactly one core'):
        arm.prepare(source, tmp_path / 'arms')
    assert not (tmp_path / 'arms').exists()


def test_delta_is_validated_before_writes(project, tmp_path):
    (project / arm.DELTA).write_text('[mechanism_delta]\nappend_rulepack_paths=["unexpected.toml"]\n')
    with pytest.raises(ValueError, match='delta differs'):
        arm.prepare(SOURCE, tmp_path / 'arms')
    assert not (tmp_path / 'arms').exists()


@pytest.mark.parametrize('mutation', [None, 'metrics', 'build', 'spec'])
def test_equivalence_binds_build_spec_and_all_layer_metrics(tmp_path, mutation):
    from test_agentic_layers import _scorecard
    import benchmark_spec as specs
    import subject_build

    main, mechanism = tmp_path / 'main', tmp_path / 'mechanism'
    main.mkdir()
    mechanism.mkdir()
    for number in (2, 1, 4):
        card = _scorecard({"C": 1, "A": 1, "D": 0, "R": 1})
        card['gaze'] = {'revision': 'b' * 40, 'dirty': False}
        card['parameters']['binary_profile'] = 'release'
        card['subject_build'] = {'version': 1, 'mode': 'producer-build',
            'source': copy.deepcopy(card['gaze']), 'profile': 'release',
            'configs': ['policy-file'], 'features': subject_build.required_features(['policy-file']),
            'binary_sha256': 'a' * 64}
        card['benchmark_spec'] = specs.manifest({
            'corpus_sha256': 'a' * 64, 'generator': {}, 'seed': 20260710,
            'contracts': {'synthetic': 'b' * 64}, 'harness': {'synthetic.py': 'c' * 64},
            'record_schema': [1], 'protocol': {}, 'population': {}})
        card['scoring']['scored_label_contract']['version'] = number
        (main / f'scorecard-v{number}.json').write_text(json.dumps(card))
        if number == 1:
            if mutation == 'build':
                card['subject_build']['binary_sha256'] = 'c' * 64
            elif mutation == 'spec':
                card['benchmark_spec']['inputs']['seed'] += 1
                card['benchmark_spec'] = specs.manifest(card['benchmark_spec']['inputs'])
            elif mutation == 'metrics':
                card['runs'][0]['metrics']['utf8_bytes']['leaked'] += 1
        (mechanism / f'scorecard-v{number}.json').write_text(json.dumps(card))
    if mutation:
        with pytest.raises(ValueError):
            arm.pair_equivalence(main, mechanism)
    else:
        receipt = arm.pair_equivalence(main, mechanism)
        assert set(receipt['contracts']) == {'1', '2', '4'}
        assert all(row['same_layer_metrics'] for row in receipt['contracts'].values())
        assert receipt['candidate'] == 'b' * 40
