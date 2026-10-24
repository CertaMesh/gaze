"""Semantic bindings at the runtime, producer, canonical gold and history boundaries."""

import copy
import json
import shutil
from types import SimpleNamespace
from unittest import mock

import pytest

import benchmark_spec as specs
import benchmark_certification as cert
import benchmark_history as history
import capture_identity as identity
import gaze_bench_score as score
import scorecard_record as records
import run_no_opf_benchmark as runner
import rescore_past_release as past
import test_benchmark_spec as fixtures

ROOT = fixtures.ROOT
REVISION = fixtures.REVISION
evidence = fixtures.evidence


def check(value, root):
    return cert.check(value, root=root, harness=ROOT, candidate=REVISION,
                      required_mechanisms=('synthetic',))


@pytest.mark.parametrize('field,value', [('python', '3.12'), ('unicode', '14.0.0'),
                                         ('implementation', 'pypy'), ('pyarrow', '0.0.0')])
def test_runtime_drift_refuses_resolve_capture_replay_and_admission(tmp_path, evidence, monkeypatch, field, value):
    import importlib.metadata
    import sys
    import unicodedata
    plan, measure = plan_and_measure(tmp_path, evidence)
    folder = tmp_path / 'runtime-history'
    job = plan['releases'][0]
    row = measure(job, folder, evidence['benchmark_spec'])
    assert history.verified(row, folder, evidence['benchmark_spec'], job)
    if field == 'python':
        monkeypatch.setattr(sys, 'version_info', (3, 12, 13))
    elif field == 'unicode':
        monkeypatch.setattr(unicodedata, 'unidata_version', value)
    elif field == 'implementation':
        monkeypatch.setattr(sys, 'implementation', SimpleNamespace(name=value))
    else:
        monkeypatch.setattr(importlib.metadata, 'version', lambda name: value)
    with pytest.raises(specs.SpecError, match='^benchmark_runtime_mismatch$'):
        specs.resolve(**fixtures.inputs(ROOT))
    # Both real producer entry points refuse before any subject/build side effect.
    monkeypatch.setattr(runner, 'validate_cli_guards', mock.Mock(side_effect=AssertionError('subject capture started')))
    monkeypatch.setattr(past.subprocess, 'run', mock.Mock(side_effect=AssertionError('subject capture started')))
    for entry, args in ((runner._run, runner.parse_args(['full'])),
                        (past.run, past.parse_args(['--release-root', str(tmp_path), '--binary', 'unused',
                            '--binary-profile', 'debug', '--configs', 'rule-floor-extended',
                            '--output', str(tmp_path / 'unused.json')]))):
        with pytest.raises(specs.SpecError, match='^benchmark_runtime_mismatch$'):
            entry(args)
    with pytest.raises(records.RecordError, match='^benchmark_runtime_mismatch$'):
        records.rescore(tmp_path / 'candidate/observations-v1.jsonl.gz', score.SCORED_LABEL_CONTRACT_V1, max_workers=1)
    with pytest.raises(cert.CertificationError):
        check(evidence, tmp_path)
    spec = evidence['benchmark_spec']
    assert not history.verified(row, folder, spec, job)


def test_unicode_reviewer_boundary_is_bound_to_runtime(tmp_path, monkeypatch):
    import unicodedata
    # U+1FAE8 changed from unassigned to symbol between Unicode 14 and 15.
    assert unicodedata.category('\U0001fae8') == 'So'
    args = fixtures.inputs(fixtures.measuring_root(tmp_path))
    spec = specs.resolve(**args)
    assert spec['inputs']['measuring_runtime']['python'] == '3.13'
    assert spec['inputs']['measuring_runtime']['unicode'] == unicodedata.unidata_version
    changed = copy.deepcopy(spec['inputs'])
    changed['measuring_runtime']['unicode'] = '14.0.0'
    assert specs.manifest(changed)['id'] != spec['id']
    with pytest.raises(specs.SpecError, match='^benchmark_runtime_mismatch$'):
        specs.require_runtime(specs.manifest(changed))


def rebuild(root, arm, number=1):
    """Review reproduction: fully repin self-consistent evidence, never bypass admission."""
    rp = root / arm['record']['file']
    cp = root / arm['capture']['file']
    header, observations = records._read(rp)
    spec = header['scorecard']['benchmark_spec']
    fresh = records.rescore(rp, cert._contract(ROOT, number, spec), max_workers=1)
    header['scorecard'] = copy.deepcopy(fresh)
    header['scorecard'].pop('observation_record', None)
    records._write_rows(rp, [header, *observations])
    cp.write_text(json.dumps(records.rescore(rp, cert._contract(ROOT, number, spec), max_workers=1)))
    arm['capture']['sha256'] = identity.file_digest(cp)
    arm['record']['sha256'] = identity.file_digest(rp)
    for n, pin in arm['contracts'].items():
        path = root / pin['file']
        path.write_text(json.dumps(records.rescore(rp, cert._contract(ROOT, int(n), spec), max_workers=1)))
        pin['sha256'] = identity.file_digest(path)


@pytest.mark.parametrize('pair_name', ['gain', 'mechanism'])
def test_reviewer_contract_specific_capture_refuses_even_if_repinned(tmp_path, evidence, pair_name):
    pair = evidence['gain'] if pair_name == 'gain' else evidence['mechanisms']['synthetic']
    for arm in pair.values():
        if 'record' in arm:
            rebuild(tmp_path, arm, 2)
    with pytest.raises(cert.CertificationError, match='^noncanonical_capture$'):
        check(evidence, tmp_path)


@pytest.mark.parametrize('pair_name', ['gain', 'mechanism'])
@pytest.mark.parametrize('sides', [('candidate',), ('base', 'candidate')])
@pytest.mark.parametrize('field', ['gold', 'utf8_bytes', 'language', 'region', 'source_dataset', 'negative_category', 'cell'])
def test_canonical_descriptors_bind_every_arm(tmp_path, evidence, pair_name, sides, field):
    pair = evidence['gain'] if pair_name == 'gain' else evidence['mechanisms']['synthetic']
    for side in sides:
        arm = pair[side]
        rp = tmp_path / arm['record']['file']
        header, observations = records._read(rp)
        for row in header['documents']:
            if row['layer'] != 'A':
                continue
            if field == 'gold':
                row['gold'][0][1] -= 1
                row['validator']['gold_validation'][0]['end'] -= 1
                for observation in observations:
                    if observation['layer'] == 'A':
                        for gap in observation['response'].get('gold_gap_evidence', []):
                            gap[6] = []
            elif field == 'utf8_bytes':
                row[field] += 1
            else:
                row[field] = 'synthetic changed metadata'
        header['layer_identity'] = records._layer_identity(header['scorecard']['layers']['generator'], header['documents'])
        records._write_rows(rp, [header, *observations])
        rebuild(tmp_path, arm)
    with pytest.raises(cert.CertificationError, match='^canonical_document_mismatch$'):
        check(evidence, tmp_path)


def plan_and_measure(root, evidence):
    header, _ = records._read(root / 'candidate/observations-v1.jsonl.gz')
    jobs = [dict(version=v, args=[], subject=history.subject_identity(header),
                 response_validation=specs.CURRENT_VALIDATION)
            for v in ('v0.18.0', 'v0.17.0', 'v0.16.0')]
    plan = dict(benchmark_spec=evidence['benchmark_spec'], current='v0.18.0', previous='v0.17.0', releases=jobs)
    def measure(job, folder, spec):
        folder.mkdir(parents=True, exist_ok=True)
        artifacts = {}
        for name in ('capture.json', 'observations-v1.jsonl.gz', 'scorecard-v1.json', 'scorecard-v2.json'):
            shutil.copyfile(root / 'candidate' / name, folder / name)
            artifacts[name] = identity.file_digest(folder / name)
        return dict(status='available', benchmark_spec_id=spec['id'], job_sha256=identity.digest(job), artifacts=artifacts)
    return plan, measure


@pytest.mark.parametrize('tamper', ['artifact', 'saved_metrics', 'removed_job'])
def test_default_resume_revalidates_optional_rows_without_backfill(tmp_path, evidence, tamper):
    plan, measure = plan_and_measure(tmp_path, evidence)
    out = tmp_path / 'history'
    state = history.run(plan, out, execute=True, older=True, measure_job=measure)
    assert all(row['status'] == 'available' for row in state['releases'].values())
    row = state['releases']['v0.16.0']
    row['contracts']['2']['C']['leaked'] = 123456
    if tamper == 'artifact':
        (out / 'v0.16.0/scorecard-v2.json').write_text('{}')
    if tamper == 'removed_job':
        plan['releases'].pop()
    history.atomic(out / 'history.json', state)
    with mock.patch.object(history, 'measure', side_effect=AssertionError('optional backfill executed')):
        state = history.run(plan, out, execute=True)
    assert state['required_complete']
    assert '123456' not in (out / 'history.md').read_text()
    row = state['releases']['v0.16.0']
    if tamper == 'saved_metrics':
        assert row['status'] == 'available'
        assert row['contracts']['2']['C']['leaked'] != 123456
    else:
        assert row['status'] == 'unavailable'


def test_history_shares_canonical_gold_admission(tmp_path, evidence):
    arm = evidence['gain']['candidate']
    rp = tmp_path / arm['record']['file']
    header, obs = records._read(rp)
    header['documents'][0]['source_dataset'] = 'synthetic changed source'
    records._write_rows(rp, [header, *obs])
    rebuild(tmp_path, arm)
    plan, measure = plan_and_measure(tmp_path, evidence)
    state = history.run(plan, tmp_path / 'history', execute=True, older=True, measure_job=measure)
    assert not state['required_complete']
    assert all(row['status'] == 'unavailable' for row in state['releases'].values())


def produced_capture(tmp_path, monkeypatch, evidence, producer, flags, response_factory=None):
    """Run the actual producer and writer; replace only dataset/subject execution."""
    from test_agentic_layers import _success_response
    source = tmp_path / 'candidate'
    folder = tmp_path / 'produced'
    folder.mkdir()
    dataset = folder / 'synthetic.parquet'
    dataset.write_bytes(b'synthetic canonical loader substitute')
    negative = folder / 'negative.jsonl'
    negative.write_text('')
    binary = source / 'synthetic-binary'
    policy = tmp_path / 'base.toml'
    canonical = specs.full_inputs(ROOT)
    prepared = canonical['prepared']
    documents = {'C': canonical['available'], 'A': prepared.identifiers,
                 'D': prepared.lookalikes, 'R': prepared.repeats}
    def measurements(_probe, docs, *_args):
        return {'schema_version': 1, 'validator_recognizers': [], 'validator_kinds_by_class': {},
                'documents': {doc.uid: {'gold_validation': [dict(start=span.start, end=span.end, label=span.label,
                     applicable=False, validator_passed=None) for span in (*doc.spans, *doc.excluded_spans)],
                     'predictions': None} for doc in docs}}
    real_run = score.run_config
    def synthetic_responses(*args, **kwargs):
        kwargs.setdefault('replay_responses', {doc.uid: _success_response(doc) for doc in args[3]})
        return real_run(*args, **kwargs)
    if response_factory is None:
        monkeypatch.setattr(score, 'run_config', synthetic_responses)
    else:
        by_id = {doc.uid: doc for docs in documents.values() for doc in docs}
        process = mock.MagicMock()
        process.__enter__.return_value = process
        process.message_deadline = 0
        process.exchange.side_effect = lambda request: response_factory(by_id[request['fixture_id']])
        monkeypatch.setattr(score, 'BenchSubprocess', lambda *a, **k: process)
    monkeypatch.setattr(score, 'collect_validator_measurements', measurements)
    monkeypatch.setattr(score, 'git_metadata', lambda root: {'revision': REVISION, 'dirty': False})
    monkeypatch.setattr(past.subprocess, 'run', lambda *a, **k: SimpleNamespace(stdout=''))
    monkeypatch.setattr(runner.dataiku, 'verify_dataset', lambda p: None)
    monkeypatch.setattr(runner.dataiku, 'load_documents', lambda p: (documents['C'], {}))
    monkeypatch.setattr(runner, 'load_negative_documents', lambda p: ([], {}))
    monkeypatch.setattr(runner, 'composite_dataset_report', lambda *a: ({}, {'integrity': {'sha256': 'd' * 64}}))
    monkeypatch.setattr(runner.dataiku, 'build_binary', lambda *a, **k: binary)
    monkeypatch.setattr(runner, 'build_validator_probe', lambda *a, **k: binary)
    monkeypatch.setattr(runner, 'validate_required_models', lambda *a: [])
    monkeypatch.setattr(runner, 'policy_ner_provenance', lambda *a: (0.3, []))
    monkeypatch.setattr(runner, 'policy_ner_settings', lambda data, policy, model, threshold: (model, threshold))
    monkeypatch.setattr(fixtures.agentic, 'prepare', lambda *a: prepared)
    generated = [SimpleNamespace(to_document=lambda doc=doc: doc)
                 for layer in ('A', 'D', 'R') for doc in documents[layer]]
    monkeypatch.setattr(fixtures.agentic, 'generate', lambda *a: generated)
    if producer == 'current':
        def finish(args, output, card):
            runner.write_json(output / 'capture.json', card)
            return 0
        monkeypatch.setattr(runner, 'finish_run', finish)
        args = runner.parse_args(['full', '--dataset', str(dataset), '--negative-corpus', str(negative),
            '--policy', str(policy), '--output-dir', str(folder), '--no-download', '--no-cache', *flags])
        assert runner._run(args) == 0
        folder = folder / 'full'
    else:
        args = past.parse_args(['--release-root', str(tmp_path), '--binary', str(binary), '--binary-profile', 'debug',
            '--configs', 'policy-file', '--policy', str(policy), '--agentic-layers', '--dataset', str(dataset),
            '--output', str(folder / 'capture.json'), *flags])
        past.run(args)
    return folder


@pytest.mark.parametrize('producer', ['current', 'release'])
@pytest.mark.parametrize('pair_name', ['gain', 'mechanism'])
@pytest.mark.parametrize('flags', [[], ['--cross-check'], ['--cross-check', '--scored-labels',
    'docs/reference/benchmarks/scored-labels-v2.json']])
def test_actual_producers_persist_and_enforce_capture_mode(tmp_path, evidence, monkeypatch, producer, flags, pair_name):
    folder = produced_capture(tmp_path, monkeypatch, evidence, producer, flags)
    capture = folder / 'capture.json'
    record = folder / 'observations-v1.jsonl.gz'
    header, _ = records._read(record)
    expected = {'version': 1, 'mode': 'cross-check' if flags else 'canonical-once'}
    assert header['capture_protocol'] == expected
    assert json.loads(capture.read_text()).get('capture_protocol') == expected
    # This exercises the common admission used by both gain and mechanism pairs and history.
    if flags:
        pair = evidence['gain'] if pair_name == 'gain' else evidence['mechanisms']['synthetic']
        arm = pair['candidate']
        arm['record'] = {'file': str(record.relative_to(tmp_path)), 'sha256': identity.file_digest(record)}
        arm['capture'] = {'file': str(capture.relative_to(tmp_path)), 'sha256': identity.file_digest(capture)}
        with pytest.raises(cert.CertificationError, match='^noncanonical_capture$'):
            check(evidence, tmp_path)
    else:
        assert cert.admit_capture(capture, record, evidence['benchmark_spec'], ROOT, REVISION)


@pytest.mark.parametrize('layer', ['C', 'A', 'D', 'R'])
def test_canonical_descriptor_digest_covers_every_layer(tmp_path, evidence, layer):
    arm = evidence['gain']['candidate']
    rp = tmp_path / arm['record']['file']
    header, obs = records._read(rp)
    next(row for row in header['documents'] if row['layer'] == layer)['region'] = 'synthetic-region'
    records._write_rows(rp, [header, *obs])
    rebuild(tmp_path, arm)
    with pytest.raises(cert.CertificationError, match='^canonical_document_mismatch$'):
        check(evidence, tmp_path)


@pytest.mark.parametrize('location', ['header', 'original', 'capture'])
def test_missing_producer_mode_cannot_be_inferred_from_v1(tmp_path, evidence, location):
    arm = evidence['gain']['candidate']
    rp = tmp_path / arm['record']['file']
    cp = tmp_path / arm['capture']['file']
    header, obs = records._read(rp)
    card = json.loads(cp.read_text())
    if location == 'header':
        header.pop('capture_protocol')
    elif location == 'original':
        header['scorecard'].pop('capture_protocol')
    else:
        card.pop('capture_protocol')
    if location == 'capture':
        cp.write_text(json.dumps(card))
    else:
        records._write_rows(rp, [header, *obs])
        rebuild(tmp_path, arm)
        if location == 'original':
            captured = json.loads(cp.read_text())
            captured['capture_protocol'] = specs.CANONICAL_CAPTURE
            cp.write_text(json.dumps(captured))
    # Admission requires the producer discriminator even with a canonical scoring contract.
    with pytest.raises(cert.CertificationError, match='^noncanonical_capture$'):
        cert.admit_capture(cp, rp, evidence['benchmark_spec'], ROOT)


def test_runtime_patch_release_is_not_a_spec_input(monkeypatch):
    import sys
    before = specs.require_runtime()
    monkeypatch.setattr(sys, 'version_info', (3, 13, 99))
    assert specs.require_runtime() == before


def test_writer_refuses_spec_runtime_drift(tmp_path, evidence):
    spec = copy.deepcopy(evidence['benchmark_spec'])
    spec['inputs']['measuring_runtime']['unicode'] = '14.0.0'
    spec = specs.manifest(spec['inputs'])
    writer = records.RecordWriter([], {'documents': {}}, corpus_sha256='d' * 64)
    with pytest.raises(specs.SpecError, match='^benchmark_runtime_mismatch$'):
        writer.write(tmp_path / 'drift.jsonl.gz', {'benchmark_spec': spec}, add_reference=True)


@pytest.mark.parametrize('python_version,credit', [('3.11', 0), ('3.12', 11)])
def test_reviewer_unicode_probe_refuses_unqualified_interpreters(python_version, credit):
    import subprocess
    result = subprocess.run(['uv', 'python', 'find', '--no-python-downloads', python_version],
                            capture_output=True, text=True)
    if result.returncode:
        pytest.skip('optional alternate interpreter is not installed')
    probe = r'''
import json,unicodedata
from pathlib import Path
import benchmark_spec as specs
import gaze_bench_score as score
text='Dr. Schmidt / Dr. Schmidt\U0001fae8'
gold=score.Span(0,11,'PERSON'); pred=score.Span(14,25,'person')
doc=score.Document('synthetic',text,'en','US','synthetic',(gold,),gold_gap=score.GoldGapRule(frozenset({('person','PERSON')})))
credits=score.gold_gap_credits(doc,[pred],[(0,11)],[])
try:
    specs.resolve(Path.cwd().parents[1],corpus_sha256='a'*64,seed=1,available=[doc],prepared=None)
    code='accepted'
except specs.SpecError as error:
    code=error.code
print(json.dumps({'credit':sum(c.end-c.start for c in credits),'code':code}))
'''
    completed = subprocess.run([result.stdout.strip(), '-c', probe], cwd=ROOT / 'scripts/bench',
                               capture_output=True, text=True, check=True)
    value = json.loads(completed.stdout)
    assert value == {'credit': credit, 'code': 'benchmark_runtime_mismatch'}


def test_canonical_gold_order_survives_scored_excluded_partition():
    from dataclasses import replace
    doc = score.Document('synthetic-mixed', 'XY', 'en', 'US', 'synthetic',
                         (score.Span(0, 1, 'X'), score.Span(1, 2, 'Y')))
    contract = replace(score.SCORED_LABEL_CONTRACT_V1, version=2,
                       scored_labels=frozenset({'Y'}), excluded_labels=frozenset({'X'}))
    filtered = score.apply_scored_label_contract([doc], contract)[0]
    assert specs.population([doc], 'A') == specs.population([filtered], 'A')


def attach_produced(root, evidence, folder, pair_name, side):
    pair = evidence['gain'] if pair_name == 'gain' else evidence['mechanisms']['synthetic']
    arm = pair[side]
    rp, cp = folder / 'observations-v1.jsonl.gz', folder / 'capture.json'
    arm['record'] = {'file': str(rp.relative_to(root)), 'sha256': identity.file_digest(rp)}
    arm['capture'] = {'file': str(cp.relative_to(root)), 'sha256': identity.file_digest(cp)}
    for n, pin in arm['contracts'].items():
        path = root / pin['file']
        path.write_text(json.dumps(records.rescore(rp, cert._contract(ROOT, int(n), evidence['benchmark_spec']), max_workers=1)))
        pin['sha256'] = identity.file_digest(path)


@pytest.mark.parametrize('pair_name,side', [('gain', 'base'), ('gain', 'candidate'),
                                         ('mechanism', 'base'), ('mechanism', 'candidate')])
def test_reviewer_legacy_manifest_runner_cannot_certify(tmp_path, evidence, monkeypatch, pair_name, side):
    from test_agentic_layers import _success_response
    from bench_subprocess import ProducerFailure
    def response(doc):
        value = _success_response(doc)
        if doc.spans:
            value['clean_text'] = '[REDACTED:email]'
            value['final_protection_trace'] = [dict(raw_start=0, raw_end=len(doc.text.encode()),
                **{'class': 'email'}, action='redact', provenance=dict(stage='safety_net',
                decision='redact', source_ids=['synthetic:email']))]
            value['restore']['exact'] = False
        return value
    # Same synthetic subject is refused by the current production validator.
    with monkeypatch.context() as mp:
        with pytest.raises(ProducerFailure):
            produced_capture(tmp_path, mp, evidence, 'release', [], response)
    shutil.rmtree(tmp_path / 'produced')
    folder = produced_capture(tmp_path, monkeypatch, evidence, 'release', ['--manifest-actions', 'tokenize'], response)
    header, _ = records._read(folder / 'observations-v1.jsonl.gz')
    assert header['capture_protocol'] == specs.CANONICAL_CAPTURE
    assert header['response_validation'] == specs.LEGACY_VALIDATION
    assert header['scorecard'].get('response_validation') == specs.LEGACY_VALIDATION
    assert json.loads((folder / 'capture.json').read_text()).get('response_validation') == specs.LEGACY_VALIDATION
    attach_produced(tmp_path, evidence, folder, pair_name, side)
    with pytest.raises(cert.CertificationError, match='^response_validation_mismatch$'):
        check(evidence, tmp_path)


@pytest.mark.parametrize('producer', ['current', 'release'])
@pytest.mark.parametrize('flag', ['canonical', 'cross-check', 'scored-labels', 'agentic-scored-labels', 'manifest-actions'])
def test_actual_producers_bind_every_validation_flag(tmp_path, evidence, monkeypatch, producer, flag):
    if producer == 'current' and flag == 'manifest-actions':
        return  # The current runner deliberately exposes no legacy manifest adapter.
    flags = {'canonical': [], 'cross-check': ['--cross-check'],
             'scored-labels': ['--cross-check', '--scored-labels', 'docs/reference/benchmarks/scored-labels-v2.json'],
             'agentic-scored-labels': ['--cross-check', '--agentic-scored-labels', 'docs/reference/benchmarks/scored-labels-v2.json'],
             'manifest-actions': ['--manifest-actions', 'tokenize']}[flag]
    folder = produced_capture(tmp_path, monkeypatch, evidence, producer, flags)
    rp, cp = folder / 'observations-v1.jsonl.gz', folder / 'capture.json'
    header, _ = records._read(rp)
    expected = specs.LEGACY_VALIDATION if flag == 'manifest-actions' else specs.CURRENT_VALIDATION
    assert header['response_validation'] == expected
    assert header['scorecard'].get('response_validation') == expected
    assert json.loads(cp.read_text()).get('response_validation') == expected
    if flag == 'canonical':
        assert cert.admit_capture(cp, rp, evidence['benchmark_spec'], ROOT)
    else:
        code = 'response_validation_mismatch' if flag == 'manifest-actions' else 'noncanonical_capture'
        with pytest.raises(cert.CertificationError, match=f'^{code}$'):
            cert.admit_capture(cp, rp, evidence['benchmark_spec'], ROOT)


@pytest.mark.parametrize('location', ['header', 'original', 'capture'])
def test_validation_adapter_required_at_every_boundary(tmp_path, evidence, location):
    arm = evidence['gain']['candidate']
    rp, cp = tmp_path / arm['record']['file'], tmp_path / arm['capture']['file']
    header, obs = records._read(rp)
    if location == 'capture':
        card = json.loads(cp.read_text())
        card.pop('response_validation')
        cp.write_text(json.dumps(card))
    else:
        (header if location == 'header' else header['scorecard']).pop('response_validation')
        records._write_rows(rp, [header, *obs])
    with pytest.raises(cert.CertificationError, match='^response_validation_mismatch$'):
        cert.admit_capture(cp, rp, evidence['benchmark_spec'], ROOT)


@pytest.mark.parametrize('version,declared,allowed', [
    ('v0.14.0', specs.LEGACY_VALIDATION, True),
    ('v0.14.0', specs.CURRENT_VALIDATION, False),
    ('v0.17.0', specs.CURRENT_VALIDATION, True),
    ('v0.18.0', specs.CURRENT_VALIDATION, True),
    ('v0.17.0', specs.LEGACY_VALIDATION, False),
    ('v0.18.0', specs.LEGACY_VALIDATION, False),
    ('v0.18.0', None, False),
    ('v9.0.0', specs.CURRENT_VALIDATION, False)])
def test_history_declares_release_protocol(version, declared, allowed):
    job = dict(version=version, response_validation=declared)
    if allowed:
        assert history.release_validation(job) == declared
    else:
        with pytest.raises(history.HistoryError, match='^release_protocol_mismatch$'):
            history.release_validation(job)


@pytest.mark.parametrize('version', ['v0.18.0', 'v0.17.0', 'v0.14.0'])
def test_history_binds_declared_adapter_to_actual_record(tmp_path, evidence, monkeypatch, version):
    folder = produced_capture(tmp_path, monkeypatch, evidence, 'release', ['--manifest-actions', 'tokenize'])
    rp = folder / 'observations-v1.jsonl.gz'
    for n in (1, 2):
        (folder / f'scorecard-v{n}.json').write_text(json.dumps(records.rescore(rp,
            cert._contract(ROOT, n, evidence['benchmark_spec']), max_workers=1)))
    header, _ = records._read(rp)
    validation = specs.LEGACY_VALIDATION if version == 'v0.14.0' else specs.CURRENT_VALIDATION
    job = dict(version=version, subject=history.subject_identity(header), response_validation=validation)
    row = dict(status='available', benchmark_spec_id=evidence['benchmark_spec']['id'], job_sha256=identity.digest(job),
        artifacts={p.name: identity.file_digest(p) for p in folder.iterdir() if p.name in
                   ('capture.json', 'observations-v1.jsonl.gz', 'scorecard-v1.json', 'scorecard-v2.json')})
    assert history.verified(row, folder, evidence['benchmark_spec'], job) == (version == 'v0.14.0')


def test_history_measure_refuses_adapter_before_subject(tmp_path, monkeypatch):
    monkeypatch.setattr(past, 'run', mock.Mock(side_effect=AssertionError('subject capture started')))
    job = dict(version='v0.18.0', response_validation=specs.CURRENT_VALIDATION,
        args=['--release-root', str(tmp_path), '--binary', 'unused', '--binary-profile', 'debug',
              '--configs', 'policy-file', '--agentic-layers', '--manifest-actions', 'tokenize'])
    with pytest.raises(history.HistoryError, match='^release_protocol_mismatch$'):
        history.measure(job, tmp_path / 'history', {})


@pytest.mark.parametrize('flag', ['manifest-actions', 'split-composite-source-ids'])
def test_generated_runner_records_validation_flags(tmp_path, evidence, monkeypatch, flag):
    import agentic_layers as agentic
    from test_agentic_layers import _success_response
    prepared = specs.full_inputs(ROOT)['prepared']
    docs = {doc.uid: doc for doc in (*prepared.identifiers, *prepared.lookalikes, *prepared.repeats)}
    process = mock.MagicMock()
    process.__enter__.return_value = process
    process.message_deadline = 0
    def response(request):
        doc = docs[request['fixture_id']]
        value = _success_response(doc)
        if flag == 'split-composite-source-ids' and doc.spans:
            value['clean_text'] = '[REDACTED:email]'
            value['manifest_spans'] = [dict(raw_start=0, raw_end=len(doc.text.encode()),
                clean_start=0, clean_end=len(value['clean_text']), **{'class': 'email'})]
            value['manifest_integrity']['spans'] = 1
            value['final_protection_trace'] = [dict(raw_start=0, raw_end=len(doc.text.encode()),
                **{'class': 'email'}, action='redact', provenance=dict(stage='safety_net',
                decision='redact', source_ids=['synthetic:email+synthetic:email']))]
            value['restore']['exact'] = False
        return value
    process.exchange.side_effect = response
    monkeypatch.setattr(score, 'BenchSubprocess', lambda *a, **k: process)
    monkeypatch.setattr(score, 'validator_probe_binary', lambda root: tmp_path / 'candidate/synthetic-binary')
    monkeypatch.setattr(score, 'collect_validator_measurements', lambda probe, documents, ids: {
        'schema_version': 1, 'validator_recognizers': [], 'validator_kinds_by_class': {}, 'documents': {
            doc.uid: {'gold_validation': [dict(start=s.start, end=s.end, label=s.label, applicable=False,
                validator_passed=None) for s in (*doc.spans, *doc.excluded_spans)], 'predictions': None} for doc in documents}})
    monkeypatch.setattr(score, 'git_metadata', lambda root: {'revision': REVISION, 'dirty': False})
    monkeypatch.setattr(agentic, 'prepare', lambda *a: prepared)
    monkeypatch.setattr(agentic, 'generate', lambda *a: [SimpleNamespace(to_document=lambda doc=doc: doc) for doc in docs.values()])
    output = tmp_path / 'generated/capture.json'
    flags = ['--manifest-actions', 'tokenize'] if flag == 'manifest-actions' else ['--split-composite-source-ids']
    argv = ['measure', '--binary', str(tmp_path / 'candidate/synthetic-binary'),
        '--config', 'policy-file', '--label', 'synthetic', '--output', str(output)]
    if flag == 'split-composite-source-ids':
        from bench_subprocess import ProducerFailure
        with pytest.raises(ProducerFailure):
            agentic.main(argv)
    assert agentic.main([*argv, *flags]) == 0
    header, _ = records._read(output.with_name('observations-v1.jsonl.gz'))
    expected = specs.LEGACY_VALIDATION if flag == 'manifest-actions' else {**specs.CURRENT_VALIDATION,
                                                                       'split_composite_source_ids': True}
    assert header['response_validation'] == expected
    assert header['scorecard'].get('response_validation') == expected
    assert json.loads(output.read_text()).get('response_validation') == expected


@pytest.mark.parametrize('retained', [False, True])
def test_history_rows_rederive_validation_adapter(tmp_path, evidence, monkeypatch, retained):
    plan, measure = plan_and_measure(tmp_path, evidence)
    monkeypatch.setattr(history.render, 'render_spec_history', lambda state: '')
    out = tmp_path / 'history'
    state = history.run(plan, out, execute=True, older=True, measure_job=measure)
    version = 'v0.16.0' if retained else plan['current']
    state['releases'][version]['response_validation'] = specs.LEGACY_VALIDATION
    history.atomic(out / 'history.json', state)
    state = history.run(plan, out)
    assert state['releases'][version].get('response_validation') == specs.CURRENT_VALIDATION


def test_history_renderer_refuses_wrong_adapter(tmp_path, evidence):
    plan, measure = plan_and_measure(tmp_path, evidence)
    state = history.run(plan, tmp_path / 'history', execute=True, measure_job=measure)
    state['releases'][plan['current']]['response_validation'] = specs.LEGACY_VALIDATION
    with pytest.raises(history.render.RenderError, match='^release_protocol_mismatch$'):
        history.render.render_spec_history(state)
