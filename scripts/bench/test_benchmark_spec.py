"""Immutable specs, exact candidate admission and resumable history without inference."""

import copy
import json
import shutil
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import pytest

import agentic_layers as agentic
import benchmark_certification as cert
import benchmark_history as history
import benchmark_spec as specs
import capture_identity
import gaze_bench_score as score
import render_benchmark_doc as render
import scorecard_record as records
from test_agentic_layers import _scorecard, _success_response

ROOT = Path(__file__).resolve().parents[2]
REVISION = 'a' * 40


def measuring_root(root):
    bench = root / 'scripts/bench'
    bench.mkdir(parents=True)
    for name in ('scorer.py', 'generator.py', 'pyproject.toml', 'uv.lock'):
        (bench / name).write_text('synthetic measuring source')
    shutil.copyfile(ROOT / specs.PROTOCOL, root / specs.PROTOCOL)
    contracts = root / 'docs/reference/benchmarks'
    contracts.mkdir(parents=True)
    shutil.copyfile(ROOT / 'docs/reference/benchmarks/scored-labels-v2.json',
                    contracts / 'scored-labels-v2.json')
    return root


def inputs(root):
    doc = score.Document('synthetic', 'Reserved fixture.', 'en', 'US', 'fixture', ())
    prepared = SimpleNamespace(
        manifest={'generator_version': 1, 'seed': 1, 'corpus_sha256': 'b' * 64},
        contract=SimpleNamespace(sha256='c' * 64), identifiers=[doc], lookalikes=[], repeats=[])
    return dict(root=root, corpus_sha256='d' * 64, seed=20260710, available=[doc], prepared=prepared)


def test_spec_deterministic_across_roots_processes_and_mapping_order(tmp_path):
    left = measuring_root(tmp_path / 'left')
    right = measuring_root(tmp_path / 'right')
    a, b = specs.resolve(**inputs(left)), specs.resolve(**inputs(right))
    assert a == b
    assert specs.manifest(dict(reversed(list(a['inputs'].items())))) == a
    script = 'import benchmark_spec,json,sys; print(benchmark_spec.manifest(json.load(sys.stdin))["id"])'
    for seed in ('1', '9'):
        import os
        result = subprocess.run([sys.executable, '-c', script], cwd=ROOT / 'scripts/bench',
                                input=json.dumps(a['inputs']), capture_output=True, text=True,
                                check=True, env={**os.environ, 'PYTHONHASHSEED': seed})
        assert result.stdout.strip() == a['id']
    assert str(tmp_path) not in json.dumps(a)


@pytest.mark.parametrize('field', ['corpus_sha256', 'generator', 'seed', 'contracts', 'harness',
                                  'record_schema', 'protocol', 'population'])
def test_each_spec_input_changes_id(tmp_path, field):
    original = specs.resolve(**inputs(measuring_root(tmp_path)))
    changed = copy.deepcopy(original['inputs'])
    if field == 'seed':
        changed[field] += 1
    elif field == 'corpus_sha256':
        changed[field] = '0' * 64
    elif field == 'record_schema':
        changed[field] += [99]
    else:
        changed[field]['synthetic_changed_input'] = True
    assert specs.manifest(changed)['id'] != original['id']


@pytest.mark.parametrize('name', ['scripts/bench/scorer.py', 'scripts/bench/generator.py',
                                  'scripts/bench/benchmark-protocol.json',
                                  'scripts/bench/pyproject.toml', 'scripts/bench/uv.lock',
                                  'docs/reference/benchmarks/scored-labels-v2.json'])
def test_resolver_hashes_measuring_source_and_contract_files(tmp_path, name):
    args = inputs(measuring_root(tmp_path))
    original = specs.resolve(**args)['id']
    path = tmp_path / name
    if path.suffix == '.json':
        value = json.loads(path.read_text())
        value['synthetic_changed_input'] = True
        path.write_text(json.dumps(value))
    else:
        path.write_text('changed source')
    assert specs.resolve(**args)['id'] != original


def test_subject_docs_tests_and_output_do_not_change_spec(tmp_path):
    root = measuring_root(tmp_path)
    args = inputs(root)
    original = specs.resolve(**args)
    for name in ('crates/gaze/src/lib.rs', 'target/binary', 'target/policy.toml',
                 'scripts/bench/test_synthetic.py', 'scripts/bench/.venv/lib/module.py',
                 'docs/reference/benchmarks/release-history.json', 'docs/reference/benchmarks/README.md'):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('unrelated subject or retained output')
    assert specs.resolve(**args) == original


def test_gate_refuses_spec_mismatch_with_value_free_error(tmp_path):
    spec = specs.resolve(**inputs(measuring_root(tmp_path)))
    a = _scorecard(dict.fromkeys(('C', 'A', 'D', 'R'), 0))
    a['benchmark_spec'] = spec
    b = copy.deepcopy(a)
    b['benchmark_spec']['inputs']['seed'] += 1
    b['benchmark_spec'] = specs.manifest(b['benchmark_spec']['inputs'])
    with pytest.raises(agentic.LayerError, match='^benchmark_spec_mismatch$'):
        agentic.compare_layers(a, b)
    with pytest.raises(specs.SpecError, match='^benchmark_spec_unavailable$'):
        specs.require_same(a, {})


def make_population():
    email = 'alice@example.invalid'
    docs = {layer: [score.Document(layer.lower(), email, 'en', 'US', 'synthetic',
                                  (score.Span(0, len(email), 'EMAIL'),),
                                  cell=f'{layer}|synthetic|csv|valid' if layer != 'C' else None)]
            for layer in ('C', 'A', 'R')}
    docs['D'] = [score.Document('d-' + family, 'A reserved fixture.', 'en', 'US', 'synthetic', (),
                                cell=f'D|{family}|prose|benign')
                 for family in agentic.guard_families(agentic.GENERATOR_VERSION)]
    return docs


def synthetic_spec(root, docs):
    root = measuring_root(root)
    prepared = SimpleNamespace(manifest={
        'generator_version': agentic.GENERATOR_VERSION, 'seed': 20260710,
        'corpus_sha256': 'd' * 64, 'documents': sum(len(docs[k]) for k in ('A', 'D', 'R')),
        'documents_by_layer': {k: len(docs[k]) for k in ('A', 'D', 'R')}},
        contract=agentic.load_contract(ROOT), identifiers=docs['A'], lookalikes=docs['D'], repeats=docs['R'])
    return specs.resolve(root, corpus_sha256='d' * 64, seed=20260710,
                         available=docs['C'], prepared=prepared)


def capture(folder, docs, spec, protect, policy, revision=REVISION):
    folder.mkdir(parents=True)
    binary = folder / 'synthetic-binary'
    binary.write_bytes(b'synthetic binary')
    all_docs = [doc for group in docs.values() for doc in group]
    measurements = {'validator_kinds_by_class': {}, 'documents': {
        doc.uid: {'gold_validation': [
            {'start': span.start, 'end': span.end, 'label': span.label,
             'applicable': False, 'validator_passed': None} for span in doc.spans],
            'predictions': None} for doc in all_docs}}
    main_measurements = records.filter_measurements(measurements, docs['C'])
    writer = records.RecordWriter(docs['C'], main_measurements, corpus_sha256='d' * 64,
                                  binary=binary, extra_documents=[doc for doc in all_docs if doc not in docs['C']],
                                  layer_contract=agentic.load_contract(ROOT))
    runs = {}
    for layer, documents in docs.items():
        responses = {}
        for doc in documents:
            response = _success_response(doc)
            if protect and doc.spans:
                response['final_protection_trace'] = [{
                    'raw_start': 0, 'raw_end': len(doc.text), 'class': 'email', 'action': 'tokenize',
                    'provenance': {'stage': 'primary_pipeline', 'decision': 'policy', 'source_ids': ['synthetic:email']}}]
                response['manifest_integrity']['spans'] = 1
            responses[doc.uid] = response
        measured = records.filter_measurements(measurements, documents)
        runs[layer] = score.run_config(ROOT, binary, 'policy-file', documents, folder, None, None, None,
                                       0.3, folder, replay_responses=responses,
                                       validator_measurements=measured,
                                       record_document=lambda config, doc, response, measured, layer=layer:
                                           writer.add(layer, config, doc, response, measured))
    layer_contract = agentic.load_contract(ROOT)
    card = {
        'schema_version': 4, 'generated_at': 'synthetic', 'gaze': {'revision': revision, 'dirty': False},
        'dataset': {'integrity': {'sha256': 'd' * 64},
                    'validator_gold_census': score.validator_gold_census(docs['C'], main_measurements)},
        'scoring': {'scored_label_contract': score.scored_label_contract_report(score.SCORED_LABEL_CONTRACT_V1, docs['C'])},
        'parameters': {'profile': 'full', 'max_documents': None, 'configs': ['policy-file'],
                       'ner_threshold': 0.3, 'policy_sha256': capture_identity.file_digest(policy),
                       'sampling_seed': 20260710, 'warmup_count': 1, 'measured_repetitions': 1},
        'runner_provenance': {'policy': {'path': str(policy), 'sha256': capture_identity.file_digest(policy)},
                              'model_bundles': [], 'policy_dependencies': {'files': {}}},
        'runs': [runs['C']], 'benchmark_spec': spec,
        'layers': {'generator': spec['inputs']['generator'],
                   'scored_label_contract': score.scored_label_contract_report(layer_contract, all_docs),
                   'gold_validity': {'C': agentic.gold_validity_digest(docs['C'], main_measurements)},
                   **{layer: {'runs': [runs[layer]], 'population': score.population_summary(docs[layer])}
                      for layer in ('A', 'D', 'R')}},
    }
    record = folder / 'observations-v1.jsonl.gz'
    writer.write(record, card, add_reference=True)
    pins = {'record': {'file': str(record), 'sha256': capture_identity.file_digest(record)}, 'contracts': {}}
    for number in (1, 2):
        contract = score.SCORED_LABEL_CONTRACT_V1 if number == 1 else score.load_scored_label_contract(
            ROOT / 'docs/reference/benchmarks/scored-labels-v2.json')
        path = folder / f'scorecard-v{number}.json'
        path.write_text(json.dumps(records.rescore(record, contract, max_workers=1)))
        pins['contracts'][str(number)] = {'file': str(path), 'sha256': capture_identity.file_digest(path)}
    return pins


@pytest.fixture
def evidence(tmp_path):
    docs = make_population()
    spec = synthetic_spec(tmp_path / 'measuring', docs)
    policy = tmp_path / 'base.toml'
    policy.write_text('[ner]\nthreshold = 0.3\n')
    delta = tmp_path / 'delta.toml'
    delta.write_text('[synthetic_mechanism]\nenabled = true\n')
    candidate_policy = tmp_path / 'candidate.toml'
    candidate_policy.write_text(policy.read_text() + delta.read_text())
    a = capture(tmp_path / 'base', docs, spec, False, policy, revision='b' * 40)
    b = capture(tmp_path / 'candidate', docs, spec, True, policy)
    ma = capture(tmp_path / 'mechanism-base', docs, spec, False, policy)
    mb = capture(tmp_path / 'mechanism-candidate', docs, spec, True, candidate_policy)
    value = {'schema_version': 1, 'kind': 'full-benchmark-certification', 'candidate': REVISION,
             'benchmark_spec': spec, 'gain': {'base': a, 'candidate': b},
             'mechanisms': {'synthetic': {'base': ma, 'candidate': mb,
                                         'policy_delta': {'file': str(delta), 'sha256': capture_identity.file_digest(delta)}}}}
    # Pins are relative to the certification manifest, never machine paths.
    def relative(node):
        if isinstance(node, dict):
            for key, child in node.items():
                if key == 'file':
                    node[key] = str(Path(child).relative_to(tmp_path))
                else:
                    relative(child)
    relative(value['gain'])
    relative(value['mechanisms'])
    return value


def test_certificate_passes_full_replayed_evidence_and_refuses_missing_contract(tmp_path, evidence):
    result = cert.check(evidence, root=tmp_path, harness=ROOT, candidate=REVISION,
                        required_mechanisms=('synthetic',))
    assert result['verdict'] == 'pass'
    assert set(result['contracts']) == {'1', '2'}
    assert set(result['mechanisms']['synthetic']) == {'1', '2'}
    del evidence['gain']['candidate']['contracts']['1']
    with pytest.raises(cert.CertificationError, match='^absent_contract$'):
        cert.check(evidence, root=tmp_path, harness=ROOT, candidate=REVISION, required_mechanisms=('synthetic',))


def test_certificate_refuses_stale_candidate_and_missing_mechanism(tmp_path, evidence):
    with pytest.raises(cert.CertificationError, match='^stale_candidate$'):
        cert.check(evidence, root=tmp_path, harness=ROOT, candidate='c' * 40, required_mechanisms=('synthetic',))
    with pytest.raises(cert.CertificationError, match='^absent_mechanism$'):
        cert.check(evidence, root=tmp_path, harness=ROOT, candidate=REVISION, required_mechanisms=('missing',))
    path = tmp_path / evidence['gain']['candidate']['record']['file']
    with pytest.raises(cert.CertificationError, match='^stale_candidate$'):
        cert.full_record(path, evidence['benchmark_spec'], 'c' * 40)


@pytest.mark.parametrize('mutation', ['missing_observation', 'duplicate_observation', 'missing_document', 'sampled'])
def test_full_population_admission_refuses_partial_record(tmp_path, evidence, mutation):
    path = tmp_path / evidence['gain']['candidate']['record']['file']
    header, observations = records._read(path)
    if mutation == 'missing_observation':
        observations.pop()
    elif mutation == 'duplicate_observation':
        observations.append(observations[-1])
    elif mutation == 'missing_document':
        header['documents'].pop(0)
    else:
        header['scorecard']['parameters']['profile'] = 'sampled'
    records._write_rows(path, [header, *observations])
    with pytest.raises((cert.CertificationError, records.RecordError), match='partial_population|layer document identity'):
        cert.full_record(path, evidence['benchmark_spec'], REVISION)


def test_record_spec_cannot_be_restamped(tmp_path, evidence):
    path = tmp_path / evidence['gain']['candidate']['record']['file']
    header, observations = records._read(path)
    header['benchmark_spec_id'] = 'wrong'
    records._write_rows(path, [header, *observations])
    with pytest.raises(records.RecordError, match='^benchmark_spec_mismatch$'):
        records.rescore(path, score.SCORED_LABEL_CONTRACT_V1, max_workers=1)


def test_legacy_history_and_scorecards_keep_original_metrics():
    ledger = render.load_history(ROOT / 'docs/reference/benchmarks/release-history.json')
    before = copy.deepcopy(ledger)
    render.validate_history(ledger)
    assert ledger == before
    assert all('benchmark_spec' not in row for row in ledger['releases'])
    fixture = ROOT / 'scripts/bench/fixtures/record-once.jsonl.gz'
    replay = records.rescore(fixture, score.SCORED_LABEL_CONTRACT_V1, max_workers=1)
    replay.pop('observation_record')
    assert replay == json.loads((fixture.parent / 'record-once-v1.json').read_text())


def test_history_selection_defaults_to_required_pair(tmp_path):
    spec = specs.resolve(**inputs(measuring_root(tmp_path / 'measuring')))
    plan = {'benchmark_spec': spec, 'current': 'v0.18.0', 'previous': 'v0.17.0',
            'releases': [{'version': v, 'args': []} for v in ('v0.18.0', 'v0.17.0', 'v0.16.0')]}
    state = history.run(plan, tmp_path / 'history')
    assert set(state['releases']) == {'v0.18.0', 'v0.17.0'}
    assert not state['required_complete']
    assert 'not_measured' in (tmp_path / 'history/history.md').read_text()
    assert len(history.selection(plan, True)) == 3
    assert all(row['status'] == 'unavailable' for row in state['releases'].values())


def test_history_resume_verifies_pins_and_older_failure_does_not_block(tmp_path, evidence):
    spec = evidence['benchmark_spec']
    plan = {'benchmark_spec': spec, 'current': 'v0.18.0', 'previous': 'v0.17.0',
            'releases': [{'version': v, 'args': []} for v in ('v0.18.0', 'v0.17.0', 'v0.16.0')]}
    source_header, _ = records._read(tmp_path / 'candidate/observations-v1.jsonl.gz')
    subject = history.subject_identity(source_header)
    for job in plan['releases']:
        job['subject'] = subject
    calls = []
    def measure(job, folder, spec):
        calls.append(job['version'])
        if job['version'] == 'v0.16.0':
            raise RuntimeError('synthetic private diagnostic must never escape')
        source = tmp_path / 'candidate'
        folder.mkdir(parents=True, exist_ok=True)
        artifacts = {}
        for name in ('observations-v1.jsonl.gz', 'scorecard-v1.json', 'scorecard-v2.json'):
            shutil.copyfile(source / name, folder / name)
            artifacts[name] = capture_identity.file_digest(folder / name)
        return {'status': 'available', 'benchmark_spec_id': spec['id'],
                'job_sha256': capture_identity.digest(job), 'artifacts': artifacts}
    # The synthetic spec's measuring files live in its own root.
    with mock.patch.object(history.past, 'HARNESS_ROOT', tmp_path / 'measuring'):
        state = history.run(plan, tmp_path / 'history', execute=True, older=True, measure_job=measure)
        assert state['required_complete']
        assert calls == ['v0.18.0', 'v0.17.0', 'v0.16.0']
        calls.clear()
        history.run(plan, tmp_path / 'history', execute=True, measure_job=measure)
        assert calls == []
        (tmp_path / 'history/v0.17.0/scorecard-v1.json').write_text('tampered')
        # Recompute only the broken required release, leaving the valid one untouched.
        history.run(plan, tmp_path / 'history', execute=True, measure_job=measure)
        assert calls == ['v0.17.0']
    text = (tmp_path / 'history/history.md').read_text()
    assert 'synthetic private diagnostic' not in text
    assert 'measurement_refused' in text
    changed = copy.deepcopy(plan)
    changed['benchmark_spec']['inputs']['seed'] += 1
    changed['benchmark_spec'] = specs.manifest(changed['benchmark_spec']['inputs'])
    with pytest.raises(specs.SpecError, match='^benchmark_spec_mismatch$'):
        history.run(changed, tmp_path / 'history')


def test_history_renderer_refuses_cross_spec_available_row(tmp_path):
    spec = specs.resolve(**inputs(measuring_root(tmp_path)))
    state = {'benchmark_spec': spec, 'releases': {'v0.18.0': {
        'required': True, 'status': 'available', 'benchmark_spec_id': 'different'}}}
    with pytest.raises(render.RenderError, match='^benchmark_spec_mismatch$'):
        render.render_spec_history(state)


def test_resolver_binds_seed_generator_and_record_schema(tmp_path):
    args = inputs(measuring_root(tmp_path))
    original = specs.resolve(**args)
    assert specs.resolve(**{**args, 'seed': args['seed'] + 1})['id'] != original['id']
    args['prepared'].manifest['generator_version'] += 1
    assert specs.resolve(**args)['id'] != original['id']
    args['prepared'].manifest['generator_version'] -= 1
    with mock.patch.object(records, 'READABLE_SCHEMA_VERSIONS', frozenset({1, 2, 99})):
        assert specs.resolve(**args)['id'] != original['id']


@pytest.mark.parametrize('mutation', ['spec', 'artifact', 'mechanism_binary', 'failed_gate'])
def test_certificate_refuses_untrusted_evidence(tmp_path, evidence, mutation):
    arm = evidence['gain']['candidate']
    if mutation == 'artifact':
        path = tmp_path / arm['contracts']['2']['file']
        path.write_text('{}')
    elif mutation == 'spec':
        evidence['benchmark_spec']['inputs']['seed'] += 1
        evidence['benchmark_spec'] = specs.manifest(evidence['benchmark_spec']['inputs'])
    elif mutation == 'mechanism_binary':
        arm = evidence['mechanisms']['synthetic']['candidate']
        path = tmp_path / arm['record']['file']
        header, observations = records._read(path)
        header['binary_sha256'] = 'f' * 64
        records._write_rows(path, [header, *observations])
        arm['record']['sha256'] = capture_identity.file_digest(path)
    else:
        # Reversing a known improving pair must refuse the leak regression.
        evidence['gain']['base'], evidence['gain']['candidate'] = evidence['gain']['candidate'], evidence['gain']['base']
        path = tmp_path / evidence['gain']['candidate']['record']['file']
        header, observations = records._read(path)
        header['scorecard']['gaze']['revision'] = REVISION
        records._write_rows(path, [header, *observations])
        arm = evidence['gain']['candidate']
        arm['record']['sha256'] = capture_identity.file_digest(path)
        for number in (1, 2):
            contract = cert._contract(ROOT, number, evidence['benchmark_spec'])
            card_path = tmp_path / arm['contracts'][str(number)]['file']
            card_path.write_text(json.dumps(records.rescore(path, contract, max_workers=1)))
            arm['contracts'][str(number)]['sha256'] = capture_identity.file_digest(card_path)
    with pytest.raises(cert.CertificationError):
        cert.check(evidence, root=tmp_path, harness=ROOT, candidate=REVISION, required_mechanisms=('synthetic',))


def test_history_bars_and_trends_exclude_other_specs(tmp_path):
    ledger = render.load_history(ROOT / 'docs/reference/benchmarks/release-history.json')
    original = render.comparison_bars(ledger)
    spec = specs.resolve(**inputs(measuring_root(tmp_path)))
    ledger['releases'][-1]['benchmark_spec'] = spec
    bars = render.comparison_bars(ledger)
    assert bars
    assert len(bars) < len(original)
    assert len(render.shipped_default_trend(ledger, 'leak_rate')) == 1
    text = render.render_history(ledger)
    assert 'unavailable: specification unproven' in text
    assert spec['id'] in text
