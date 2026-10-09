"""Cache identity, custody and release evidence regression contracts."""
import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import benchmark_cache as cache
import gaze_bench_score as score
import run_no_opf_benchmark as runner
import render_benchmark_doc as render
import scorecard_record as records


class CacheReviewTests(unittest.TestCase):
    def test_real_key_contains_and_invalidates_each_measured_input(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'Cargo.lock').write_text('synthetic lock')
            cargo_home = root / 'cargo-home'
            cargo_home.mkdir()
            (root / '.cargo').mkdir()
            (root / '.cargo/config.toml').write_text('[build]\njobs=2\n')
            (cargo_home / 'config.toml').write_text('[net]\noffline=true\n')
            identity = {'revision': '1'*40, 'crates_tree_hash': '2'*40, 'harness_tree_hash': '3'*40}
            inputs = dict(repo_root=root, args=runner.parse_args(['full', '--release']),
                          policy_path=root/'policy.toml', policy_sha256='4'*64,
                          corpus_sha256='5'*64, contract=SimpleNamespace(sha256='6'*64),
                          model_provenance=[{'model_id':'synthetic', 'observed_sha256':'7'*64}],
                          policy_dependencies={}, agentic_prepared=SimpleNamespace(
                              contract=SimpleNamespace(sha256='8'*64), manifest={'corpus_sha256':'9'*64}),
                          document_ids=['synthetic-one'], threshold=0.5,
                          source_environment={'RUSTFLAGS':'synthetic-flags', 'CARGO_HOME':str(cargo_home)})
            with mock.patch.object(runner, 'source_tree_identity', side_effect=lambda _: (dict(identity), None)):
                key, reason = runner.observation_cache_key(**inputs)
                self.assertIsNone(reason)
                expected = dict(identity, policy_sha256='4'*64, seed=inputs['args'].seed,
                                corpus_sha256='5'*64, scored_labels_sha256='6'*64,
                                agentic_scored_labels_sha256='8'*64,
                                model_bundle_sha256={'synthetic':'7'*64}, release=True,
                                ner_threshold=0.5,
                                environment=runner.observation_environment_identity(inputs['source_environment']))
                expected['document_ids_sha256'] = runner.hashlib.sha256(b'["synthetic-one"]').hexdigest()
                expected['cargo_config_sha256'] = {'repository':score.sha256_file(root/'.cargo/config.toml'),
                                                   'cargo_home':score.sha256_file(cargo_home/'config.toml')}
                for name, value in expected.items():
                    self.assertEqual(key[name], value, name)
                record = root/'record'
                record.write_bytes(b'synthetic observation')
                cache.store(root/'cache', key, record)
                mutations = {
                    'policy_sha256': lambda x: x.update(policy_sha256='a'*64),
                    'seed': lambda x: setattr(x['args'], 'seed', x['args'].seed+1),
                    'corpus_sha256': lambda x: x.update(corpus_sha256='a'*64),
                    'scored_labels_sha256': lambda x: setattr(x['contract'], 'sha256','a'*64),
                    'agentic_scored_labels_sha256': lambda x: setattr(x['agentic_prepared'].contract,'sha256','a'*64),
                    'model_bundle_sha256': lambda x: x['model_provenance'][0].update(observed_sha256='a'*64),
                    'environment': lambda x: x['source_environment'].update(RUSTFLAGS='changed'),
                    'release': lambda x: setattr(x['args'],'release',False),
                    'ner_threshold': lambda x: x.update(threshold=0.75),
                    'document_ids_sha256': lambda x: x.update(document_ids=['synthetic-two']),
                }
                for name, mutate in mutations.items():
                    changed = copy.deepcopy(inputs)
                    mutate(changed)
                    candidate, _ = runner.observation_cache_key(**changed)
                    self.assertNotEqual(key[name], candidate[name], name)
                    self.assertIsNone(cache.lookup(root/'cache', candidate)[0], name)
                for name in identity:
                    before = identity[name]
                    identity[name] = 'a'*40
                    candidate, _ = runner.observation_cache_key(**inputs)
                    self.assertNotEqual(key[name],candidate[name])
                    self.assertIsNone(cache.lookup(root/'cache',candidate)[0])
                    identity[name] = before
                for path in (root/'.cargo/config.toml', cargo_home/'config.toml'):
                    path.write_text('changed config')
                    candidate, _ = runner.observation_cache_key(**inputs)
                    self.assertNotEqual(key['cargo_config_sha256'],candidate['cargo_config_sha256'])
                    self.assertIsNone(cache.lookup(root/'cache',candidate)[0])

    def test_dirty_or_changed_source_is_never_stored(self):
        key = {'revision':'original','crates_tree_hash':'tree','harness_tree_hash':'harness'}
        for identity, reason in [(None,'measured source inputs are dirty or untracked'),
                                  ({**key,'revision':'changed'},None)]:
            with mock.patch.object(runner,'source_tree_identity',return_value=(identity,reason)), \
                 mock.patch.object(cache,'store') as store:
                self.assertFalse(runner.store_observations_if_unchanged(Path('.'),Path('cache'),key,Path('record')))
                store.assert_not_called()

    def test_copy_rejects_record_changed_after_lookup(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            source=root/'record'
            source.write_bytes(b'synthetic original')
            cached=cache.store(root/'cache',{'synthetic':True},source)
            hit,_=cache.lookup(root/'cache',{'synthetic':True})
            cached.write_bytes(b'changed after lookup')
            with self.assertRaisesRegex(ValueError,'sha256 mismatch'):
                cache.copy_verified(hit,root/'copy',{'synthetic':True})
            self.assertFalse((root/'copy').exists())

    def test_release_history_refuses_replay(self):
        with self.assertRaisesRegex(render.RenderError,'must be fresh'):
            render.history_entry_from_scorecard({'cache_replay':True},version='v0.17.0',
                machine='synthetic',scorecard_filename='scorecard-v0.17.0.json',scorecard_sha256='0'*64)

    def test_replay_provenance_survives_rescoring(self):
        source=Path(__file__).resolve().parents[2]/'docs/reference/benchmarks/observations-v0.15.1.jsonl.gz'
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/source.name
            path.write_bytes(source.read_bytes())
            records.mark_cache_replay(path,'a'*64)
            header,_=records._read(path)
            self.assertTrue(header['scorecard']['cache_replay'])
            card=records.rescore(path,score.SCORED_LABEL_CONTRACT_V1,max_workers=1)
            self.assertTrue(card['cache_replay'])
            self.assertEqual(card['cache_key_sha256'],'a'*64)
