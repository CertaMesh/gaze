"""Pure discovery tests; explicitly requested lifecycle/build checks use children."""
import argparse
import copy
import io
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
import tarfile
import time
import unittest
from unittest.mock import patch

import producer_build_binding as binding
from bench_subprocess import ProducerFailure


class OwnerStateTests(unittest.TestCase):
    def test_no_signal_after_reap(self):
        owner = binding.BuildOwner(deadline=time.monotonic()+30)
        owner.process = object()
        owner.reaped = True
        with patch.object(os, 'killpg') as kill:
            with self.assertRaises(ProducerFailure):
                owner.signal_group(signal.SIGKILL)
            kill.assert_not_called()

    def test_nondefault_sigchld_rejected(self):
        with patch.object(signal, 'getsignal', return_value=signal.SIG_IGN):
            with self.assertRaises(ProducerFailure):
                binding.platform_preflight()

    def test_ambiguous_process_metadata_rejected(self):
        for data in (b'1 1 Z', b'1 1 Z\n1 1 Z\n', b'1 bad Z\n', b'1 1 ?\n'):
            with self.subTest(data=data), patch.object(binding, 'metadata', return_value=data):
                with self.assertRaises(ProducerFailure):
                    binding.group_members(1, time.monotonic()+3)

    def test_eperm_requires_known_zombie_only_group(self):
        for members, accepted in (({7: 'Z'}, True), ({7: 'Z', 8: 'S'}, False), ({}, False)):
            owner = binding.BuildOwner(deadline=time.monotonic()+30)
            owner.process = type('Leader', (), {'pid': 7})()
            with patch.object(os, 'killpg', side_effect=PermissionError), \
                    patch.object(owner, 'observe', return_value=object()), \
                    patch.object(binding, 'group_members', return_value=members):
                if accepted:
                    owner.signal_group(signal.SIGTERM)
                else:
                    with self.assertRaises(ProducerFailure):
                        owner.signal_group(signal.SIGTERM)


class CargoSelectionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.binary = self.root/'evidence_bridge'
        self.binary.write_bytes(b'synthetic comparator fixture')
        self.source = self.root/'snapshot'/binding.SOURCE
        self.event = dict(reason='compiler-artifact', package_id='approved-package',
                          target=dict(name='evidence_bridge', kind=['example'],
                                      crate_types=['bin'], src_path=str(self.source)),
                          profile=dict(test=False, opt_level='0', debuginfo=2,
                                       debug_assertions=True, overflow_checks=True),
                          executable=str(self.binary), fresh=False,
                          features=['transport-stdio'])

    def events(self):
        return binding.CargoEvents('approved-package', self.source, self.root)

    def test_split_and_coalesced_stream(self):
        wire = b'\n'.join(json.dumps(v).encode() for v in (
            dict(reason='compiler-message', message={'rendered': 'discarded'}),
            self.event, dict(reason='build-finished', success=True)))+b'\n'
        for width in (1, 7, len(wire)):
            events = self.events()
            for offset in range(0, len(wire), width):
                events.feed(wire[offset:offset+width])
            self.assertEqual(events.finish(0), self.binary)

    def test_closed_selected_fields(self):
        mutations = [('fresh', True), ('fresh', 0), ('features', []),
                     ('features', ['default', 'transport-stdio']), ('executable', None),
                     ('package_id', 'foreign-package')]
        for field, value in mutations:
            with self.subTest(field=field, value=value), self.assertRaises(ProducerFailure):
                event = copy.deepcopy(self.event)
                event[field] = value
                events = self.events()
                events.event(event)
                events.event(dict(reason='build-finished', success=True))
                events.finish(0)

    def test_target_identity_and_profile(self):
        for section, field, value in (
                ('target', 'src_path', '/foreign/evidence_bridge.rs'),
                ('target', 'kind', ['bin']), ('target', 'crate_types', ['lib']),
                ('profile', 'test', True), ('profile', 'test', 0),
                ('profile', 'opt_level', '3'), ('profile', 'debuginfo', 0),
                ('profile', 'debug_assertions', False), ('profile', 'overflow_checks', False)):
            with self.subTest(field=field), self.assertRaises(ProducerFailure):
                event = copy.deepcopy(self.event)
                event[section][field] = value
                self.events().event(event)

    def test_duplicate_candidate_and_completion(self):
        events = self.events()
        events.event(self.event)
        with self.assertRaises(ProducerFailure):
            events.event(self.event)
        for status in (1, -9):
            events = self.events()
            events.event(self.event)
            events.event(dict(reason='build-finished', success=True))
            with self.assertRaises(ProducerFailure):
                events.finish(status)

    def test_partial_noise_depth_and_duplicate_keys(self):
        for raw in (b'noise\n', b'{"reason":1,"reason":2}\n',
                    b'{"x":'+b'['*65+b'0'+b']'*65+b'}\n',
                    b'{"x":NaN}\n', b'a'*(binding.MIB+1)):
            with self.subTest(length=len(raw)), self.assertRaises((ProducerFailure, ValueError)):
                self.events().feed(raw)
        events = self.events()
        events.feed(b'{')
        with self.assertRaises(ProducerFailure):
            events.finish(0)

    def test_symlink_selected_path_refused(self):
        link = self.root/'alias'
        link.symlink_to(self.binary)
        self.event['executable'] = str(link)
        with self.assertRaises(ProducerFailure):
            self.events().event(self.event)


class SnapshotTests(unittest.TestCase):
    def test_artifact_accepted_through_alias_parent(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp).resolve()
            real = base/'real'
            real.mkdir()
            alias = base/'alias'
            alias.symlink_to(real, target_is_directory=True)
            source = real/'snapshot'
            tools = {k: real/k for k in ('cargo', 'rustc', 'rustdoc')}
            package = dict(name='gaze-mcp-rmcp', id='approved-package',
                           manifest_path=str(source/'crates/gaze-mcp-rmcp/Cargo.toml'))
            def build(_, command, **kwargs):
                binary = real/'target'/'evidence_bridge'
                binary.write_bytes(b'synthetic comparator fixture')
                event = dict(reason='compiler-artifact', package_id=package['id'],
                             target=dict(name='evidence_bridge', kind=['example'], crate_types=['bin'],
                                         src_path=str(source/binding.SOURCE)),
                             profile=dict(test=False, opt_level='0', debuginfo=2,
                                          debug_assertions=True, overflow_checks=True),
                             executable=str(binary), fresh=False, features=['transport-stdio'])
                kwargs['consume'](json.dumps(event).encode()+b'\n')
                kwargs['consume'](b'{"reason":"build-finished","success":true}\n')
                return 0
            with patch.object(binding, 'prepare_inputs', return_value=(tools, 'host', {}, {},
                              {k: () for k in tools}, {}, 0)), \
                    patch.object(binding, 'snapshot', return_value=({}, 0)), \
                    patch.object(binding, 'inventory', return_value={}), \
                    patch.object(binding, 'file_digest', return_value=()), \
                    patch.object(binding, 'metadata', return_value=json.dumps({'packages':[package]}).encode()), \
                    patch.object(binding.BuildOwner, 'run', build):
                result = binding.step0(alias, repo=alias, revision='a'*40, registry=alias,
                                       native=alias, toolchain=alias)
                self.assertTrue(result['selected'], 'alias-parent-artifact-accepted')

    def test_parent_roots_canonicalized_before_setup(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            real = root/'real'
            real.mkdir()
            alias = root/'alias'
            alias.symlink_to(real, target_is_directory=True)
            with patch.object(binding, 'prepare_inputs', side_effect=ProducerFailure('invalid_state')) as setup:
                with self.assertRaises(ProducerFailure):
                    binding.step0(alias, repo=alias, revision='a'*40, registry=alias,
                                  native=alias, toolchain=alias)
                self.assertEqual(setup.call_args.args, (real,))
                for key in ('registry', 'native', 'toolchain'):
                    self.assertEqual(setup.call_args.kwargs[key], real)

    def test_exact_git_files_and_ancestor_directories(self):
        files = {'a/Cargo.toml': b'[package]\nname="fixture"\n', 'Cargo.lock': b'version = 4\n'}
        rows = []
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode='w') as tar:
            directory = tarfile.TarInfo('a')
            directory.type = tarfile.DIRTYPE
            tar.addfile(directory)
            for name, content in files.items():
                oid = binding.hashlib.sha1(f'blob {len(content)}\0'.encode()+content).hexdigest()
                rows.append(f'100644 blob {oid} {len(content)}\t{name}'.encode())
                entry = tarfile.TarInfo(name)
                entry.mode, entry.size = 0o644, len(content)
                tar.addfile(entry, io.BytesIO(content))
        def run(_, command, **kwargs):
            self.assertIn('tar.umask=0022', command)
            kwargs['consume'](stream.getvalue())
            return 0
        with tempfile.TemporaryDirectory() as temp, \
                patch.object(binding, 'metadata', side_effect=[b'a'*40+b'\n', b'\0'.join(rows)+b'\0']), \
                patch.object(binding.BuildOwner, 'run', run):
            dest = Path(temp)/'snapshot'
            result, _ = binding.snapshot(Path(temp), 'b'*40, dest, time.monotonic()+30)
            self.assertEqual(set(result), set(files))
            self.assertEqual((dest/'Cargo.lock').stat().st_mode & 0o777, 0o644)

    def test_config_presence_without_reading_contents(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root/'.cargo').mkdir()
            (root/'.cargo/config.toml').touch()
            with self.assertRaises(ProducerFailure):
                binding.no_configs(root)


def lifecycle():
    results = {}
    programs = {
        'normal_leader': 'pass',
        'leader_first_closed_pipes':
            'import os,time\nif os.fork()==0:\n os.close(1);os.close(2);time.sleep(30)\nelse:\n os._exit(0)',
        'term_ignoring_descendant':
            'import os,time,signal\nif os.fork()==0:\n signal.signal(signal.SIGTERM,signal.SIG_IGN);os.close(1);os.close(2);time.sleep(30)\nelse:\n time.sleep(.1);os._exit(0)',
        'timeout': 'import time;time.sleep(30)',
        'cancellation': 'import os,time;os.write(1,b"ready");time.sleep(30)',
    }
    for name, program in programs.items():
        owner = binding.BuildOwner(deadline=time.monotonic()+15,
                                   seconds=.15 if name == 'timeout' else 3)
        def consume(_):
            if name == 'cancellation':
                raise KeyboardInterrupt('private discarded context')
        started = time.monotonic()
        try:
            status = owner.run([sys.executable, '-c', program], consume=consume)
        except ProducerFailure as error:
            assert name != 'normal_leader', 'normal-leader-must-pass'
            assert error.__context__ is None and error.__cause__ is None, 'closed-error'
            results[name] = 'closed'
        else:
            assert name == 'normal_leader' and status == 0, 'live-descendant-must-refuse'
            assert owner.signals == [], 'normal-leader-must-not-be-killed'
            results[name] = 'accepted'
        assert owner.reaped and owner.process.returncode is not None, 'owned-leader-reaped'
        assert time.monotonic()-started < 15, 'finite-cleanup'
        if name == 'term_ignoring_descendant':
            assert signal.SIGKILL in owner.signals, 'term-ignore-needs-kill'
    return results


if __name__ == '__main__':
    if '--step0' in sys.argv:
        parser = argparse.ArgumentParser()
        parser.add_argument('--step0', action='store_true')
        for name in ('repo', 'source-revision', 'registry', 'native', 'toolchain', 'scratch-root'):
            parser.add_argument('--'+name, required=True)
        args = parser.parse_args()
        try:
            result = binding.step0(Path(args.scratch_root), repo=Path(args.repo),
                                   revision=args.source_revision, registry=Path(args.registry),
                                   native=Path(args.native), toolchain=Path(args.toolchain))
            print(json.dumps(result, sort_keys=True))
        except ProducerFailure as error:
            print(json.dumps({'step0': 'blocked', 'code': error.code, 'phase': error.phase}))
            sys.exit(1)
    elif '--lifecycle' in sys.argv:
        print(json.dumps(lifecycle(), sort_keys=True))
    else:
        unittest.main()
