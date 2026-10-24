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
import benchmark_cache
import stage_timing as stages
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
    rules = root / 'crates/gaze-recognizers/embedded'
    rules.mkdir(parents=True)
    (rules / 'synthetic.toml').write_text('[[recognizers]]\nid = "synthetic:rule"\n')
    (bench / 'no_opf_models.toml').write_text(
        '[builtin_source_ids]\nids = ["synthetic:other"]\n[model]\nid = "synthetic:model"\n')
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
                                  layer_contract=agentic.load_contract(ROOT),
                                  capture_protocol=specs.CANONICAL_CAPTURE,
                                  response_validation=specs.CURRENT_VALIDATION)
    runs = {}
    for layer, documents in docs.items():
        responses = {}
        for doc in documents:
            response = _success_response(doc)
            if protect and doc.spans:
                response['final_protection_trace'] = [{
                    'raw_start': 0, 'raw_end': len(doc.text), 'class': 'email', 'action': 'tokenize',
                    'provenance': {'stage': 'primary_pipeline', 'decision': 'policy', 'source_ids': ['synthetic:email']}}]
                response['manifest_spans'] = [dict(raw_start=0, raw_end=len(doc.text),
                    clean_start=0, clean_end=len(doc.text), **{'class': 'email'})]
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
        'parameters': {'profile': 'full', 'binary_profile': 'debug', 'max_documents': None, 'configs': ['policy-file'],
                       'ner_threshold': 0.3, 'policy_sha256': capture_identity.file_digest(policy),
                       'sampling_seed': 20260710, 'warmup_count': 1, 'measured_repetitions': 1},
        'runner_provenance': {'policy': {'path': str(policy), 'sha256': capture_identity.file_digest(policy)},
                              'model_bundles': [], 'policy_dependencies': {'files': {}}},
        'runs': [runs['C']], 'benchmark_spec': spec,
        'subject_build': {'version': 1, 'mode': 'producer-build',
                          'source': {'revision': revision, 'dirty': False},
                          'binary_sha256': capture_identity.file_digest(binary),
                          'profile': 'debug', 'configs': ['policy-file']},
        'capture_protocol': specs.CANONICAL_CAPTURE,
        'response_validation': specs.CURRENT_VALIDATION,
        'layers': {'generator': spec['inputs']['generator'],
                   'scored_label_contract': score.scored_label_contract_report(layer_contract, [doc for layer in ('A', 'D', 'R') for doc in docs[layer]]),
                   'gold_validity': {'C': agentic.gold_validity_digest(docs['C'], main_measurements)},
                   **{layer: {'runs': [runs[layer]], 'population': score.population_summary(docs[layer])}
                      for layer in ('A', 'D', 'R')}},
    }
    record = folder / 'observations-v1.jsonl.gz'
    card['observation_record'] = writer.write(record, card, add_reference=True)
    capture_path = folder / 'capture.json'
    capture_path.write_text(json.dumps(card))
    pins = {'capture': {'file': str(capture_path), 'sha256': capture_identity.file_digest(capture_path)},'record': {'file': str(record), 'sha256': capture_identity.file_digest(record)}, 'contracts': {}}
    for number in (1, 2):
        contract = score.SCORED_LABEL_CONTRACT_V1 if number == 1 else score.load_scored_label_contract(
            ROOT / 'docs/reference/benchmarks/scored-labels-v2.json')
        path = folder / f'scorecard-v{number}.json'
        path.write_text(json.dumps(records.rescore(record, contract, max_workers=1)))
        pins['contracts'][str(number)] = {'file': str(path), 'sha256': capture_identity.file_digest(path)}
    return pins


@pytest.fixture
def evidence(tmp_path, monkeypatch):
    docs = make_population()
    spec = synthetic_spec(ROOT, docs)
    monkeypatch.setattr(specs, 'full_inputs', lambda root, dataset=None: dict(
        root=root, corpus_sha256=spec['inputs']['corpus_sha256'], available=docs['C'],
        prepared=SimpleNamespace(manifest=spec['inputs']['generator'],
            contract=agentic.load_contract(ROOT), identifiers=docs['A'],
            lookalikes=docs['D'], repeats=docs['R'])))
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
    legacy = [row for row in ledger['releases'] if render.version_sort_key(row['version']) <= render.version_sort_key('v0.17.0')]
    assert legacy
    assert all('benchmark_spec' not in row for row in legacy)
    fixture = ROOT / 'scripts/bench/fixtures/record-once.jsonl.gz'
    replay = records.rescore(fixture, score.SCORED_LABEL_CONTRACT_V1, max_workers=1)
    replay.pop('observation_record')
    assert replay == json.loads((fixture.parent / 'record-once-v1.json').read_text())


def test_history_selection_defaults_to_required_pair(tmp_path):
    spec = specs.resolve(**inputs(measuring_root(tmp_path / 'measuring')))
    plan = {'benchmark_spec': spec, 'current': 'v0.18.0', 'previous': 'v0.17.0',
            'releases': [{'version': v, 'args': [], 'response_validation': specs.CURRENT_VALIDATION} for v in ('v0.18.0', 'v0.17.0', 'v0.16.0')]}
    state = history.run(plan, tmp_path / 'history')
    assert set(state['releases']) == {'v0.18.0', 'v0.17.0'}
    assert not state['required_complete']
    assert 'not_measured' in (tmp_path / 'history/history.md').read_text()
    assert len(history.selection(plan, True)) == 3
    assert all(row['status'] == 'unavailable' for row in state['releases'].values())


def test_history_resume_verifies_pins_and_older_failure_does_not_block(tmp_path, evidence):
    spec = evidence['benchmark_spec']
    plan = {'benchmark_spec': spec, 'current': 'v0.18.0', 'previous': 'v0.17.0',
            'releases': [{'version': v, 'args': [], 'response_validation': specs.CURRENT_VALIDATION} for v in ('v0.18.0', 'v0.17.0', 'v0.16.0')]}
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
        for name in ('capture.json', 'observations-v1.jsonl.gz', 'scorecard-v1.json', 'scorecard-v2.json'):
            shutil.copyfile(source / name, folder / name)
            artifacts[name] = capture_identity.file_digest(folder / name)
        return {'status': 'available', 'benchmark_spec_id': spec['id'],
                'job_sha256': capture_identity.digest(job), 'artifacts': artifacts}
    # The synthetic spec's measuring files live in its own root.
    with mock.patch.object(history.past, 'HARNESS_ROOT', ROOT):
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
        rewrite_arm(tmp_path, evidence['gain']['candidate'],
                    lambda header: (header['scorecard']['gaze'].update(revision=REVISION),
                                    header['scorecard']['subject_build']['source'].update(revision=REVISION)))
    expected = {'spec': 'invalid_certification_evidence', 'artifact': 'evidence_integrity',
                'mechanism_binary': 'mechanism_binary_mismatch', 'failed_gate': 'gate_refused'}
    with pytest.raises(cert.CertificationError, match='^' + expected[mutation] + '$'):
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


def test_certification_admits_original_capture_execution_once_for_both_replays(tmp_path, evidence):
    arm = evidence['gain']['candidate']
    capture_path = tmp_path / arm['capture']['file']
    card = json.loads(capture_path.read_text())
    key = {'synthetic': 'capture-key'}
    card['capture_cache'] = benchmark_cache.provenance(key)
    capture_path.write_text(json.dumps(card))
    arm['capture']['sha256'] = capture_identity.file_digest(capture_path)
    # Contract scorecards differ from capture.json; their integrity is proved
    # by record replay, while execution evidence binds the original artifact.
    execution = stages.Evidence()
    execution.outcome = 'completed'
    execution.cache = {'status': 'miss', 'key_sha256': benchmark_cache.key_digest(key)}
    execution.bind(capture_path, tmp_path / arm['record']['file'])
    receipt = capture_path.parent / 'run-evidence.json'
    execution.write(receipt)
    assert cert.check(evidence, root=tmp_path, harness=ROOT, candidate=REVISION,
                      required_mechanisms=('synthetic',))['verdict'] == 'pass'
    receipt.unlink()
    with pytest.raises(cert.CertificationError):
        cert.check(evidence, root=tmp_path, harness=ROOT, candidate=REVISION,
                   required_mechanisms=('synthetic',))


def test_preserved_specifications_keep_their_content_addressed_names():
    paths = sorted((ROOT / 'docs/reference/benchmarks/specifications').glob('*.json'))
    assert paths
    for path in paths:
        value = specs.validate(json.loads(path.read_text()))
        assert path.stem == value['id'].removeprefix(specs.PREFIX)
        assert '/Users/' not in json.dumps(value)
        assert '/home/' not in json.dumps(value)


@pytest.mark.parametrize('declaration', ['builtin', 'recognizer', 'collision', 'model'])
def test_spec_binds_effective_validation_vocabulary(tmp_path, declaration):
    root = measuring_root(tmp_path)
    args = inputs(root)
    before = specs.resolve(**args)
    if declaration in ('builtin', 'model'):
        path = root / 'scripts/bench/no_opf_models.toml'
        old = 'synthetic:other' if declaration == 'builtin' else 'synthetic:model'
        path.write_text(path.read_text().replace(old, 'synthetic:value'))
    else:
        path = root / 'crates/gaze-recognizers/embedded/synthetic.toml'
        path.write_text(path.read_text() + ('[recognizers.collision]\nfamily = "synthetic"\n'
                                          if declaration == 'collision' else ''))
        if declaration == 'recognizer':
            path.write_text(path.read_text().replace('synthetic:rule', 'synthetic:value'))
    after = specs.resolve(**args)
    assert score.load_committed_source_id_vocabulary(root) != frozenset(before['inputs']['validation_vocabulary'])
    assert after['id'] != before['id']
    assert after['inputs']['harness'] == before['inputs']['harness']


def test_vocabulary_membership_changes_response_acceptance_and_spec(tmp_path):
    root = measuring_root(tmp_path)
    text = 'synthetic:value'
    doc = score.Document('reserved', text, 'en', 'US', 'synthetic',
                         (score.Span(0, len(text), 'EMAIL'),))
    response = _success_response(doc)
    response['final_protection_trace'] = [{
        'raw_start': 0, 'raw_end': len(text), 'class': 'email', 'action': 'tokenize',
        'provenance': {'stage': 'primary_pipeline', 'decision': 'policy', 'source_ids': [text]}}]
    response['manifest_integrity']['spans'] = 1
    before = specs.resolve(**inputs(root))
    with mock.patch.object(score, 'COMMITTED_SOURCE_ID_VOCABULARY', score.load_committed_source_id_vocabulary(root)):
        with pytest.raises(score.ResponseValidationError):
            score._validate_final_protection_trace(doc, response['final_protection_trace'], [{'raw_start': 0, 'raw_end': len(text), 'class': 'email'}])
    path = root / 'scripts/bench/no_opf_models.toml'
    path.write_text(path.read_text().replace('synthetic:other', text))
    with mock.patch.object(score, 'COMMITTED_SOURCE_ID_VOCABULARY', score.load_committed_source_id_vocabulary(root)):
        assert score._validate_final_protection_trace(doc, response['final_protection_trace'], [{'raw_start': 0, 'raw_end': len(text), 'class': 'email'}])
    assert specs.resolve(**inputs(root))['id'] != before['id']


@pytest.mark.parametrize('mutation', ['changed', 'added', 'removed', 'protocol', 'vocabulary', 'population'])
def test_certificate_resolves_executing_spec_before_replay(tmp_path, evidence, mutation):
    spec = copy.deepcopy(evidence['benchmark_spec'])
    if mutation in ('changed', 'added', 'removed'):
        harness = spec['inputs']['harness']
        name = 'scripts/bench/gaze_bench_score.py'
        if mutation == 'removed':
            del harness[name]
        else:
            harness[name if mutation == 'changed' else 'scripts/bench/extra.py'] = 'f' * 64
    elif mutation == 'vocabulary':
        spec['inputs']['validation_vocabulary'].append('synthetic:extra')
    elif mutation == 'population':
        spec['inputs']['population']['C']['documents'] += 1
    else:
        spec['inputs']['protocol']['validation'] = 'different measuring protocol'
    evidence['benchmark_spec'] = specs.manifest(spec['inputs'])
    with mock.patch.object(records, 'rescore', side_effect=AssertionError('replay started')):
        with pytest.raises(cert.CertificationError, match='^harness_spec_mismatch$'):
            cert.check(evidence, root=tmp_path, harness=ROOT, candidate=REVISION,
                       required_mechanisms=('synthetic',))


def rewrite_arm(root, arm, update):
    path = root / arm['record']['file']
    header, observations = records._read(path)
    update(header)
    records._write_rows(path, [header, *observations])
    arm['record']['sha256'] = capture_identity.file_digest(path)
    capture_path = root / arm['capture']['file']
    captured = copy.deepcopy(header['scorecard'])
    captured['observation_record'] = records.rescore(path, score.SCORED_LABEL_CONTRACT_V1, max_workers=1)['observation_record']
    capture_path.write_text(json.dumps(captured))
    arm['capture']['sha256'] = capture_identity.file_digest(capture_path)
    for number in (1, 2):
        target = root / arm['contracts'][str(number)]['file']
        target.write_text(json.dumps(records.rescore(path, cert._contract(ROOT, number, header['scorecard']['benchmark_spec']), max_workers=1)))
        arm['contracts'][str(number)]['sha256'] = capture_identity.file_digest(target)


@pytest.mark.parametrize('pair_name', ['gain', 'mechanism'])
@pytest.mark.parametrize('field', ['policy_dependencies', 'model_bundles'])
def test_certificate_cannot_hide_record_dependency_difference(tmp_path, evidence, pair_name, field):
    pair = evidence['gain'] if pair_name == 'gain' else evidence['mechanisms']['synthetic']
    value = ({'files': {'dictionary:terms': 'a' * 64}} if field == 'policy_dependencies'
             else [{'model_id': 'davlan-mbert-ner-hrl-onnx', 'observed_sha256': 'a' * 64}])
    rewrite_arm(tmp_path, pair['base'], lambda header: header['scorecard']['runner_provenance'].update({field: value}))
    with pytest.raises(cert.CertificationError, match='^gate_refused$'):
        cert.check(evidence, root=tmp_path, harness=ROOT, candidate=REVISION, required_mechanisms=('synthetic',))
    for pin in pair['candidate']['contracts'].values():
        path = tmp_path / pin['file']
        card = json.loads(path.read_text())
        card['runner_provenance'][field] = value
        path.write_text(json.dumps(card))
        pin['sha256'] = capture_identity.file_digest(path)
    with pytest.raises(cert.CertificationError, match='^scorecard_record_mismatch$'):
        cert.check(evidence, root=tmp_path, harness=ROOT, candidate=REVISION, required_mechanisms=('synthetic',))


@pytest.mark.parametrize('artifact', ['capture.json', 'scorecard-v1.json', 'scorecard-v2.json'])
@pytest.mark.parametrize('admission', ['first', 'resume'])
def test_history_mixed_subject_artifacts_are_unavailable(tmp_path, evidence, artifact, admission):
    header, _ = records._read(tmp_path / 'candidate/observations-v1.jsonl.gz')
    job = {'version': 'v0.18.0', 'args': [], 'response_validation': specs.CURRENT_VALIDATION, 'subject': history.subject_identity(header)}
    other = {**job, 'version': 'v0.17.0'}
    plan = {'benchmark_spec': evidence['benchmark_spec'], 'current': job['version'],
            'previous': other['version'], 'releases': [job, other]}
    output = tmp_path / 'mixed-history'
    def measure(job, folder, spec):
        folder.mkdir(parents=True, exist_ok=True)
        artifacts = {}
        for name in ('capture.json', 'observations-v1.jsonl.gz', 'scorecard-v1.json', 'scorecard-v2.json'):
            source = 'base' if name == artifact and admission == 'first' else 'candidate'
            shutil.copyfile(tmp_path / source / name, folder / name)
            artifacts[name] = capture_identity.file_digest(folder / name)
        return {'status': 'available', 'benchmark_spec_id': spec['id'],
                'job_sha256': capture_identity.digest(job), 'artifacts': artifacts}
    state = history.run(plan, output, execute=True, measure_job=measure)
    if admission == 'resume':
        assert state['required_complete']
        folder = output / job['version']
        shutil.copyfile(tmp_path / 'base' / artifact, folder / artifact)
        state['releases'][job['version']]['artifacts'][artifact] = capture_identity.file_digest(folder / artifact)
        history.atomic(output / 'history.json', state)
        state = history.run(plan, output)
    assert not state['required_complete']
    assert state['releases'][job['version']]['status'] == 'unavailable'


def test_detector_toml_bytes_outside_effective_vocabulary_are_subject_inputs(tmp_path):
    root = measuring_root(tmp_path)
    before = specs.resolve(**inputs(root))
    rule = root / 'crates/gaze-recognizers/embedded/synthetic.toml'
    rule.write_text(rule.read_text() + 'pattern = "synthetic detector pattern"\n')
    models = root / 'scripts/bench/no_opf_models.toml'
    models.write_text(models.read_text() + 'bundle_sha = "' + 'f' * 64 + '"\n')
    assert specs.resolve(**inputs(root)) == before


@pytest.mark.parametrize('filename', ['lookalike_cue_vocabulary.json', 'trace-lineage-vocabulary.json'])
def test_consumed_json_vocabulary_changes_spec(tmp_path, filename):
    root = measuring_root(tmp_path)
    path = root / 'scripts/bench' / filename
    shutil.copyfile(ROOT / 'scripts/bench' / filename, path)
    before = specs.resolve(**inputs(root))
    data = json.loads(path.read_text())
    data['schema_version'] = 99
    path.write_text(json.dumps(data))
    assert specs.resolve(**inputs(root))['id'] != before['id']


@pytest.mark.parametrize('field', ['scoring', 'runner_provenance', 'parameters', 'metrics'])
def test_history_rederives_contract_metrics_and_provenance(tmp_path, evidence, field):
    folder = tmp_path / 'candidate'
    path = folder / 'scorecard-v2.json'
    card = json.loads(path.read_text())
    if field == 'scoring':
        card['scoring']['scored_label_contract']['file_sha256'] = 'f' * 64
    elif field == 'runner_provenance':
        card[field]['policy_dependencies']['files']['dictionary:terms'] = 'a' * 64
    elif field == 'parameters':
        card[field]['policy_sha256'] = 'a' * 64
    else:
        card['runs'][0]['metrics']['utf8_bytes']['leaked'] += 1
    path.write_text(json.dumps(card))
    with pytest.raises(cert.CertificationError, match='^scorecard_record_mismatch$'):
        history.scored_rows(folder, evidence['benchmark_spec'])


def test_certificate_refuses_changed_measuring_file_in_supplied_root(tmp_path, evidence):
    root = tmp_path / 'measuring-copy'
    names = set(evidence['benchmark_spec']['inputs']['harness'])
    names.update(path.relative_to(ROOT).as_posix() for path in
                 (ROOT / 'crates/gaze-recognizers/embedded').glob('*.toml'))
    names.update(path.relative_to(ROOT).as_posix() for path in
                 (ROOT / 'docs/reference/benchmarks').glob('scored-labels-v*.json'))
    names.add('scripts/bench/no_opf_models.toml')
    for name in names:
        target = root / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / name, target)
    specs.require_current(evidence['benchmark_spec'], root)
    path = root / 'scripts/bench/gaze_bench_score.py'
    path.write_text(path.read_text() + '\n# changed measuring source\n')
    with mock.patch.object(records, 'rescore', side_effect=AssertionError('replay started')):
        with pytest.raises(cert.CertificationError, match='^harness_spec_mismatch$'):
            cert.check(evidence, root=tmp_path, harness=root, candidate=REVISION,
                       required_mechanisms=('synthetic',))


def test_current_spec_loads_negative_corpus_content(tmp_path):
    import dataiku_en_de_gaze_bench as dataiku
    import run_no_opf_benchmark as runner

    root = measuring_root(tmp_path)
    args = inputs(root)
    negative = root / runner.NEGATIVE_CORPUS
    negative.parent.mkdir(parents=True)
    row = dict(id='reserved', text='A reserved fixture.', language='en',
               category='synthetic', oracle_spans=[])
    negative.write_text(json.dumps(row) + '\n')
    with mock.patch.object(dataiku, 'verify_dataset'), mock.patch.object(
            dataiku, 'load_documents', return_value=(args['available'], {'integrity': {'sha256': 'a' * 64}})), mock.patch.object(
            agentic, 'prepare', return_value=args['prepared']):
        before = specs.resolve(**specs.full_inputs(root), seed=20260710)
        row['text'] = 'A different reserved fixture.'
        negative.write_text(json.dumps(row) + '\n')
        after = specs.resolve(**specs.full_inputs(root), seed=20260710)
    assert after['inputs']['population']['C']['document_ids_sha256'] == before['inputs']['population']['C']['document_ids_sha256']
    assert after['inputs']['population']['C']['descriptors_sha256'] != before['inputs']['population']['C']['descriptors_sha256']
    assert after['inputs']['corpus_sha256'] != before['inputs']['corpus_sha256']
    assert after['id'] != before['id']


@pytest.mark.parametrize('field', ['gaze', 'parameters', 'runner_provenance'])
def test_capture_binds_subject_metadata_to_record(tmp_path, evidence, field):
    arm = evidence['gain']['candidate']
    path = tmp_path / arm['capture']['file']
    card = json.loads(path.read_text())
    if field == 'gaze':
        card[field]['revision'] = 'f' * 40
    elif field == 'parameters':
        card[field]['policy_sha256'] = 'f' * 64
    else:
        card[field]['policy_dependencies']['files']['dictionary:terms'] = 'f' * 64
    path.write_text(json.dumps(card))
    arm['capture']['sha256'] = capture_identity.file_digest(path)
    with pytest.raises(cert.CertificationError, match='^capture_record_mismatch$'):
        cert.admit_capture(path, tmp_path / arm['record']['file'], evidence['benchmark_spec'], ROOT)


def test_capture_metrics_are_reconstructed_even_when_header_and_capture_agree(tmp_path, evidence):
    arm = evidence['gain']['candidate']
    def corrupt(header):
        header['scorecard']['runs'][0]['metrics']['utf8_bytes']['leaked'] += 1
    rewrite_arm(tmp_path, arm, corrupt)
    with pytest.raises(cert.CertificationError, match='^capture_record_mismatch$'):
        cert.admit_capture(tmp_path / arm['capture']['file'], tmp_path / arm['record']['file'],
                           evidence['benchmark_spec'], ROOT)


@pytest.mark.parametrize('number', [1, 2])
def test_history_verification_binds_each_contract_to_subject(tmp_path, evidence, number):
    folder = tmp_path / 'candidate'
    name = f'scorecard-v{number}.json'
    shutil.copyfile(tmp_path / 'base' / name, folder / name)
    header, _ = records._read(folder / 'observations-v1.jsonl.gz')
    job = {'version': 'v0.18.0', 'args': [], 'response_validation': specs.CURRENT_VALIDATION, 'subject': history.subject_identity(header)}
    row = {'status': 'available', 'benchmark_spec_id': evidence['benchmark_spec']['id'],
           'job_sha256': capture_identity.digest(job), 'artifacts': {
               name: capture_identity.file_digest(folder / name) for name in
               ('capture.json', 'observations-v1.jsonl.gz', 'scorecard-v1.json', 'scorecard-v2.json')}}
    assert not history.verified(row, folder, evidence['benchmark_spec'], job)
