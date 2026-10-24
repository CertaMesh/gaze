"""Release cache input and admission proofs; no native tools execute."""

import copy
import json
from pathlib import Path
from unittest import mock

import pytest

import benchmark_cache as cache
import past_capture
import scorecard_record as records
import stage_timing as stages
from test_benchmark_spec import ROOT, capture, make_population, synthetic_spec


def bound_capture(tmp_path):
    docs = make_population()
    spec = synthetic_spec(ROOT, docs)
    policy = tmp_path / 'policy.toml'
    policy.write_text('[ner]\nthreshold=0.3\n')
    pins = capture(tmp_path / 'fresh', docs, spec, True, policy)
    record = Path(pins['record']['file'])
    header, rows = records._read(record)
    card = header['scorecard']
    card['runner_provenance']['harness_revision'] = 'b' * 40
    card['generated_at'] = '2026-10-20T12:00:00Z'
    card['dataset']['integrity']['component_sha256'] = {'dataiku': 'd' * 64, 'negative_corpus': 'e' * 64}
    card['dataset']['evaluated_population'] = records.score.population_summary(docs['C'])
    key = {'format': past_capture.FORMAT, 'binary_sha256': header['binary_sha256'],
           'benchmark': {'spec_id': spec['id'], 'harness_revision': 'b' * 40,
                         'validator': {'source': 'independent', 'binary': 'c' * 64}},
           'subject': {'release_tag': 'v0.17.0', 'build': card['subject_build']},
           'execution': {'parameters': card['parameters'],
                         'capture_protocol': card['capture_protocol'],
                         'response_validation': card['response_validation'], 'inputs': {'models': {'nym': {'model': 'd' * 64}}},
                         'policy_dependencies': card['runner_provenance']['policy_dependencies'],
                         'model_bundles': []}}
    card['capture_cache'] = cache.provenance(key)
    card['release_capture'] = past_capture.provenance(key)
    records._write_rows(record, [header, *rows])
    card = records.rescore(record, records.score.SCORED_LABEL_CONTRACT_V1, max_workers=1)
    path = record.parent / 'capture.json'
    past_capture.write_key(path.parent, key)
    path.write_text(json.dumps(card))
    with stages.session(path.parent / 'run-evidence.json', scorecard=path, record=record) as evidence:
        evidence.cache = {'status': 'miss', 'key_sha256': cache.key_digest(key)}
    return path, record, key


def test_fresh_store_replay_preserves_measurement_without_subject_execution(tmp_path):
    path, record, key = bound_capture(tmp_path)
    past_capture.admit(json.loads(path.read_text()), path, version='v0.17.0')
    cache.store_layers(tmp_path / 'cache', key, record)
    destination = tmp_path / 'replayed' / record.name
    destination.parent.mkdir()
    with mock.patch('gaze_bench_score.subprocess.Popen', side_effect=AssertionError('subject executed')):
        assert cache.restore_layers(tmp_path / 'cache', key, destination) == 'hit'
        replay = records.rescore(destination, records.score.SCORED_LABEL_CONTRACT_V1, max_workers=1)
    assert replay == json.loads(path.read_text())
    assert destination.read_bytes() == record.read_bytes()
    assert replay['generated_at'] == '2026-10-20T12:00:00Z'
    assert replay['runner_provenance']['harness_revision'] == 'b' * 40


@pytest.mark.parametrize('field', ['generated_at', 'runner_provenance', 'subject_build', 'parameters',
                                   'release_capture', 'capture_cache'])
def test_changed_provenance_is_rejected(tmp_path, field):
    path, record, key = bound_capture(tmp_path)
    card = json.loads(path.read_text())
    card[field] = 'tampered'
    path.write_text(json.dumps(card))
    with pytest.raises(ValueError):
        past_capture.admit(card, path, version='v0.17.0')


def test_missing_key_legacy_key_and_wrong_tag_are_refused(tmp_path):
    path, _, key = bound_capture(tmp_path)
    card = json.loads(path.read_text())
    with pytest.raises(ValueError):
        past_capture.admit(card, path, version='v0.16.0')
    (path.parent / past_capture.KEY_FILE).write_text('{}')
    with pytest.raises(ValueError):
        past_capture.admit(card, path)
    (path.parent / past_capture.KEY_FILE).unlink()
    with pytest.raises(ValueError):
        past_capture.admit(card, path)


@pytest.fixture
def key_inputs(tmp_path, monkeypatch):
    from types import SimpleNamespace
    import rescore_past_release as past
    import capture_identity as identity
    import gaze_bench_score as score
    root = tmp_path / 'release'
    harness = tmp_path / 'harness'
    root.mkdir()
    (harness / 'scripts/bench/validator_recall_probe/src').mkdir(parents=True)
    (harness / 'scripts/bench/validator_recall_probe/src/main.rs').write_text('synthetic validator')
    paths = {}
    for name in ('binary', 'probe', 'policy', 'dataset', 'negative'):
        paths[name] = root / name
        paths[name].write_text('' if name == 'policy' else 'synthetic:' + name)
    models = root / 'models'
    models.mkdir()
    (models / 'model.onnx').write_bytes(b'synthetic model')
    paths['policy'].write_text(f'[ner]\nmodel_dir="{models}"\n')
    source = {'revision': 'a' * 40, 'dirty': False}
    harness_source = {'revision': 'b' * 40, 'crates_tree_hash': 'c' * 40, 'harness_tree_hash': 'd' * 40}
    monkeypatch.setattr(score, 'git_metadata', lambda _: dict(source))
    monkeypatch.setattr(past.runner, 'source_tree_identity', lambda _: (dict(harness_source), None))
    monkeypatch.setattr(past.runner, 'NEGATIVE_CORPUS', paths['negative'])
    monkeypatch.setattr(past_capture, 'release_tag', lambda root, requested, revision: requested)
    runtime = {'closure': 'synthetic runtime'}
    monkeypatch.setattr(identity, 'native_runtime', lambda _: dict(runtime))
    parameters = {'profile': 'full', 'binary_profile': 'release', 'configs': ['policy-file'],
                  'ner_threshold': 0.3, 'max_documents': None, 'sampling_seed': 20260710,
                  'warmup_count': 0, 'measured_repetitions': 1, 'opf': False,
                  'policy_sha256': identity.file_digest(paths['policy'])}
    receipt = {'version': 1, 'mode': 'producer-build', 'source': dict(source),
               'binary_sha256': identity.file_digest(paths['binary']), 'configs': ['policy-file'],
               'features': ['safety-net-nym'], 'profile': 'release'}
    spec = synthetic_spec(ROOT, make_population())
    args = SimpleNamespace(release_tag='v0.17.0', policy=paths['policy'], model_env=[], model_bundle=[],
                           dataset=paths['dataset'])
    inputs = dict(harness_root=harness, release_root=root, args=args, binary=paths['binary'],
                  probe=paths['probe'], receipt=receipt, spec=spec, parameters=parameters,
                  bundles=[], environment={'PATH': '/synthetic'}, document_ids=['synthetic'])
    return inputs, paths, models, runtime, harness_source


@pytest.mark.parametrize('change', ['model', 'policy', 'corpus', 'negative', 'harness', 'runtime',
                                    'binary', 'features', 'spec', 'validator_binary', 'validator_source',
                                    'environment', 'documents', 'release_tag', 'capture_protocol', 'response_validation'])
def test_live_inputs_invalidate_one_at_a_time(key_inputs, change):
    inputs, paths, models, runtime, harness_source = key_inputs
    original = past_capture.key(**inputs)
    if change == 'model':
        (models / 'model.onnx').write_bytes(b'changed model')
    elif change in ('policy', 'corpus', 'negative', 'validator_binary'):
        path = paths[{'corpus': 'dataset', 'validator_binary': 'probe'}.get(change, change)]
        path.write_text(path.read_text() + '\n# changed\n')
        if change == 'policy':
            inputs['parameters']['policy_sha256'] = past_capture.identity.file_digest(path)
    elif change == 'harness':
        harness_source['revision'] = 'e' * 40
    elif change == 'runtime':
        runtime['closure'] = 'changed'
    elif change == 'binary':
        paths['binary'].write_bytes(b'changed binary')
        inputs['receipt']['binary_sha256'] = past_capture.identity.file_digest(paths['binary'])
    elif change == 'features':
        inputs['receipt']['features'] = []
        with pytest.raises(ValueError, match='unqualified_subject_build'):
            past_capture.key(**inputs)
        return
    elif change == 'spec':
        spec_inputs = copy.deepcopy(inputs['spec']['inputs'])
        spec_inputs['seed'] += 1
        inputs['spec'] = past_capture.specs.manifest(spec_inputs)
    elif change == 'validator_source':
        (inputs['harness_root'] / 'scripts/bench/validator_recall_probe/src/main.rs').write_text('changed')
    elif change == 'environment':
        inputs['environment']['RAYON_NUM_THREADS'] = '2'
    elif change == 'capture_protocol':
        inputs['args'].cross_check = True
    elif change == 'response_validation':
        inputs['args'].manifest_actions = 'tokenize'
    elif change == 'documents':
        inputs['document_ids'] = ['changed']
    else:
        inputs['args'].release_tag = 'v0.16.0'
    assert past_capture.key(**inputs) != original


@pytest.mark.parametrize('field', ['format', 'binary', 'release_marker', 'cache_marker',
    'header_release', 'header_cache', 'spec', 'harness', 'build', 'parameters', 'policy', 'models',
    'tag_format', 'tag_match', 'measurement', 'features', 'capture_protocol', 'response_validation'])
def test_each_binding_guard_rejects_independently(tmp_path, field):
    path, record, key = bound_capture(tmp_path)
    header, _ = records._read(record)
    card = copy.deepcopy(header['scorecard'])
    if field in ('format', 'binary', 'spec', 'harness', 'build', 'parameters', 'policy', 'models', 'tag_format', 'features', 'capture_protocol', 'response_validation'):
        if field == 'format':
            key['format'] = 'legacy'
        elif field == 'binary':
            key['binary_sha256'] = 'e' * 64
        elif field == 'spec':
            key['benchmark']['spec_id'] = 'changed'
        elif field == 'harness':
            key['benchmark']['harness_revision'] = 'c' * 40
        elif field == 'build':
            key['subject']['build'] = {}
        elif field == 'parameters':
            key['execution']['parameters'] = {}
        elif field == 'policy':
            key['execution']['policy_dependencies'] = {'changed': True}
        elif field == 'models':
            key['execution']['model_bundles'] = [{'changed': True}]
        elif field in ('capture_protocol', 'response_validation'):
            key['execution'][field] = {}
        elif field == 'tag_format':
            key['subject']['release_tag'] = 'not-a-tag'
        else:
            card['subject_build']['features'] = []
            header['scorecard']['subject_build']['features'] = []
            key['subject']['build']['features'] = []
        # Re-pin markers so this exercises the semantic guard, not the digest.
        for target in (card, header['scorecard']):
            target['capture_cache'] = cache.provenance(key)
            target['release_capture'] = past_capture.provenance(key)
    elif field in ('release_marker', 'cache_marker'):
        marker = 'release_capture' if field == 'release_marker' else 'capture_cache'
        card[marker] = {}
        header['scorecard'][marker] = {}
    elif field in ('header_release', 'header_cache'):
        header['scorecard']['release_capture' if field == 'header_release' else 'capture_cache'] = {}
    elif field == 'measurement':
        card['generated_at'] = '2099-01-01T00:00:00Z'
    with pytest.raises(ValueError):
        past_capture.validate_binding(card, header, key, version='v0.16.0' if field == 'tag_match' else None)


@pytest.mark.parametrize('tamper', ['part', 'key', 'provenance'])
def test_cache_damage_never_creates_replay_output(tmp_path, tamper):
    _, record, key = bound_capture(tmp_path)
    cache_root = tmp_path / 'cache'
    cache.store_layers(cache_root, key, record)
    entry = cache_root / cache.key_digest(key)
    if tamper == 'part':
        (entry / 'C.jsonl.gz').write_bytes(b'broken')
    elif tamper == 'key':
        metadata = json.loads((entry / 'layers.json').read_text())
        metadata['key']['subject']['release_tag'] = 'v0.16.0'
        (entry / 'layers.json').write_text(json.dumps(metadata))
    else:
        import gzip
        with gzip.open(entry / 'header.jsonl.gz', 'rt') as stream:
            header = json.loads(stream.read())
        header['scorecard']['capture_cache']['key_sha256'] = 'e' * 64
        records._write_rows(entry / 'header.jsonl.gz', [header])
        metadata = json.loads((entry / 'layers.json').read_text())
        metadata['parts']['header'] = past_capture.identity.file_digest(entry / 'header.jsonl.gz')
        (entry / 'layers.json').write_text(json.dumps(metadata))
    output = tmp_path / 'refused.gz'
    assert cache.restore_layers(cache_root, key, output) != 'hit'
    assert not output.exists()


def test_release_driver_fresh_then_hit_executes_no_subject_or_validator(key_inputs, tmp_path, monkeypatch):
    from types import SimpleNamespace
    import rescore_past_release as past
    from test_scorecard_record import RecordReplayTests
    fixture = RecordReplayTests()
    fixture.setUp()
    inputs, paths, _, _, _ = key_inputs
    args = past.parse_args(['--release-root', str(inputs['release_root']), '--build',
        '--release-tag', 'v0.17.0', '--binary-profile', 'release', '--configs', 'policy-file',
        '--policy', str(paths['policy']), '--dataset', str(paths['dataset']), '--warmups', '0',
        '--cache-dir', str(tmp_path / 'cache'), '--output', str(tmp_path / 'fresh/card.json')])
    monkeypatch.setattr(past, 'HARNESS_ROOT', inputs['harness_root'])
    monkeypatch.setattr(past.runner, 'build_no_opf_environment', lambda env: dict(inputs['environment']))
    monkeypatch.setattr(past.subprocess, 'run', lambda cmd, **kw: SimpleNamespace(
        stdout='b' * 40 if cmd[1] == 'rev-parse' else ''))
    monkeypatch.setattr(past.subject_build, 'build', lambda *a, **kw: (paths['binary'], inputs['receipt']))
    monkeypatch.setattr(past.runner, 'build_validator_probe', lambda *a, **kw: paths['probe'])
    monkeypatch.setattr(past.runner, 'validate_required_models', lambda *a: [])
    monkeypatch.setattr(past.dataiku, 'verify_dataset', lambda *a: None)
    monkeypatch.setattr(past.dataiku, 'load_documents', lambda *a: ([fixture.document], {
        'integrity': {'sha256': past._sha256(paths['dataset'])}}))
    monkeypatch.setattr(past.runner, 'load_negative_documents', lambda *a: ([], {'sha256': past._sha256(paths['negative'])}))
    monkeypatch.setattr(past.benchmark_spec, 'resolve', lambda *a, **kw: inputs['spec'])
    executions = []
    run_config = past.score.run_config
    def detect(*a, **kw):
        if 'replay_responses' not in kw:
            executions.append('subject')
            kw['replay_responses'] = {fixture.document.uid: fixture.response}
        return run_config(*a, **kw)
    def validate(*a, **kw):
        executions.append('validator')
        return fixture.measurements
    monkeypatch.setattr(past.score, 'run_config', detect)
    monkeypatch.setattr(past.score, 'collect_validator_measurements', validate)
    first = past.run(args)
    assert executions == ['validator', 'subject']
    fresh = json.loads(first.read_text())
    assert 'release_capture' in fresh
    second = tmp_path / 'hit/card.json'
    args.output = second
    past.run(args)
    assert json.loads((second.parent / 'run-evidence.json').read_text())['cache']['status'] == 'hit'
    assert executions == ['validator', 'subject']
    assert second.read_bytes() == first.read_bytes()
    assert (second.parent / 'observations-v1.jsonl.gz').read_bytes() == (first.parent / 'observations-v1.jsonl.gz').read_bytes()


def test_rebound_execution_evidence_cannot_change_recorded_metrics(tmp_path):
    path, record, _ = bound_capture(tmp_path)
    card = json.loads(path.read_text())
    card['runs'][0]['metrics']['utf8_bytes']['leaked'] += 1
    path.write_text(json.dumps(card))
    with stages.session(path.parent / 'run-evidence.json', scorecard=path, record=record) as evidence:
        evidence.cache = {'status': 'hit', 'key_sha256': card['capture_cache']['key_sha256']}
    with pytest.raises(ValueError, match='unverified_release_capture'):
        past_capture.admit(card, path)


def test_replayed_contract_uses_original_capture_evidence(tmp_path):
    path, record, _ = bound_capture(tmp_path)
    contract = records.score.load_scored_label_contract(ROOT / 'docs/reference/benchmarks/scored-labels-v2.json')
    card = records.rescore(record, contract, max_workers=1)
    derived = path.parent / 'scorecard-v2.json'
    derived.write_text(json.dumps(card))
    past_capture.admit(card, derived, version='v0.17.0')


def test_renderer_admits_verified_replay_and_preserves_original_date(tmp_path):
    import render_benchmark_doc as render
    path, _, _ = bound_capture(tmp_path)
    card = json.loads(path.read_text())
    row = render.history_entry_from_scorecard(card, version='v0.17.0', machine='synthetic',
        scorecard_filename=path.name, scorecard_sha256=past_capture.identity.file_digest(path),
        scorecard_path=path, shipped_arm='policy-file')
    assert row['date'] == '2026-10-20'
    assert row['commit'] == 'a' * 40
    assert row['benchmark_spec'] == card['benchmark_spec']
    (path.parent / past_capture.KEY_FILE).unlink()
    with pytest.raises(render.RenderError, match='verified run evidence'):
        render.history_entry_from_scorecard(card, version='v0.17.0', machine='synthetic',
            scorecard_filename=path.name, scorecard_sha256='e' * 64,
            scorecard_path=path, shipped_arm='policy-file')


def test_history_admits_replay_but_rejects_missing_or_changed_private_proof(tmp_path, monkeypatch):
    import benchmark_history as history
    path, record, _ = bound_capture(tmp_path)
    card = json.loads(path.read_text())
    for number in (1, 2):
        contract = history.certification._contract(ROOT, number, card['benchmark_spec'])
        (path.parent / f'scorecard-v{number}.json').write_text(
            json.dumps(records.rescore(record, contract, max_workers=1)))
    job = {'version': 'v0.17.0', 'response_validation': past_capture.specs.CURRENT_VALIDATION,
           'subject': history.subject_identity(records._read(record)[0])}
    names = ['capture.json', record.name, 'scorecard-v1.json', 'scorecard-v2.json',
             'run-evidence.json', past_capture.KEY_FILE]
    row = {'status': 'available', 'benchmark_spec_id': card['benchmark_spec']['id'],
           'job_sha256': history.digest(job),
           'artifacts': {name: history.file_digest(path.parent / name) for name in names}}
    monkeypatch.setattr(history.specs, 'require_current', lambda *a, **kw: None)
    assert history.verified(row, path.parent, card['benchmark_spec'], job)
    (path.parent / past_capture.KEY_FILE).write_text('{}')
    assert not history.verified(row, path.parent, card['benchmark_spec'], job)


@pytest.mark.parametrize('requested,resolved,revision', [
    (None, 'a' * 40, 'a' * 40), ('legacy', 'a' * 40, 'a' * 40),
    ('v0.17.0', 'b' * 40, 'a' * 40)])
def test_release_tag_requires_an_exact_release_commit(tmp_path, monkeypatch, requested, resolved, revision):
    monkeypatch.setattr(past_capture.subprocess, 'check_output', lambda *a, **kw: resolved)
    with pytest.raises(ValueError):
        past_capture.release_tag(tmp_path, requested, revision)


def test_release_tag_accepts_annotated_or_lightweight_commit_resolution(tmp_path, monkeypatch):
    calls = []
    def resolve(cmd, **kw):
        calls.append(cmd)
        return 'a' * 40 + '\n'
    monkeypatch.setattr(past_capture.subprocess, 'check_output', resolve)
    assert past_capture.release_tag(tmp_path, 'v0.17.0', 'a' * 40) == 'v0.17.0'
    assert calls[0][-1] == 'refs/tags/v0.17.0^{commit}'


def test_certification_requires_release_key_even_when_execution_hashes_match(tmp_path):
    import benchmark_certification as cert
    path, record, _ = bound_capture(tmp_path)
    card = json.loads(path.read_text())
    (path.parent / past_capture.KEY_FILE).unlink()
    with pytest.raises(ValueError):
        cert.admit_capture(path, record, card['benchmark_spec'], ROOT)


def test_history_measure_retains_key_and_execution_proof(tmp_path, monkeypatch):
    from types import SimpleNamespace
    import benchmark_history as history
    path, record, _ = bound_capture(tmp_path)
    card = json.loads(path.read_text())
    job = {'version': 'v0.17.0', 'response_validation': past_capture.specs.CURRENT_VALIDATION,
           'subject': history.subject_identity(records._read(record)[0]), 'args': []}
    monkeypatch.setattr(history.past, 'parse_args', lambda argv: SimpleNamespace(
        agentic_layers=True, max_documents=None, cross_check=False,
        scored_labels=None, agentic_scored_labels=None))
    monkeypatch.setattr(history.past, 'run', lambda args: path)
    result = history.measure(job, path.parent, card['benchmark_spec'])
    assert {'run-evidence.json', past_capture.KEY_FILE} <= set(result['artifacts'])


@pytest.mark.parametrize('damage', ['missing_policy', 'unsupported_config', 'unsupported_backend',
                                   'unqualified_build', 'missing_tag', 'dirty_harness'])
def test_incomplete_input_closure_is_uncacheable(key_inputs, monkeypatch, damage):
    inputs, paths, _, _, _ = key_inputs
    if damage == 'missing_policy':
        inputs['args'].policy = None
    elif damage == 'unsupported_config':
        inputs['parameters']['configs'] = ['rule-floor-extended']
        inputs['receipt']['configs'] = ['rule-floor-extended']
        inputs['receipt']['features'] = []
    elif damage == 'unsupported_backend':
        paths['policy'].write_text('[safety_net]\nbackend="opf"\n')
        inputs['parameters']['policy_sha256'] = past_capture.identity.file_digest(paths['policy'])
    elif damage == 'unqualified_build':
        inputs['receipt']['mode'] = 'unqualified'
    elif damage == 'missing_tag':
        # key_inputs substitutes the Git tag resolver; restore its refusal here.
        monkeypatch.setattr(past_capture, 'release_tag', lambda *a: (_ for _ in ()).throw(ValueError('release_tag_required')))
    else:
        import run_no_opf_benchmark as runner
        monkeypatch.setattr(runner, 'source_tree_identity', lambda root: (None, 'dirty'))
    with pytest.raises((ValueError, KeyError)):
        past_capture.key(**inputs)


def test_derived_card_cannot_switch_execution_record(tmp_path):
    import shutil
    path, record, _ = bound_capture(tmp_path)
    alternate = path.parent / 'alternate.jsonl.gz'
    shutil.copyfile(record, alternate)
    derived = records.rescore(alternate, records.score.SCORED_LABEL_CONTRACT_V1, max_workers=1)
    derived_path = path.parent / 'derived.json'
    derived_path.write_text(json.dumps(derived))
    with pytest.raises(ValueError, match='unverified_release_capture'):
        past_capture.admit(derived, derived_path)


def test_policy_changed_after_loading_is_uncacheable(key_inputs):
    inputs, paths, _, _, _ = key_inputs
    paths['policy'].write_text('[ner]\nthreshold=0.9\n')
    with pytest.raises(ValueError, match='policy_changed_after_loading'):
        past_capture.key(**inputs)


def test_history_subject_rejects_another_tag_at_the_same_revision(tmp_path):
    import benchmark_history as history
    _, record, _ = bound_capture(tmp_path)
    header, _ = records._read(record)
    subject = history.subject_identity(header)
    assert history.subject_matches(record, {'version': 'v0.17.0', 'subject': subject})
    assert not history.subject_matches(record, {'version': 'v0.16.0', 'subject': subject})
