"""Pure discovery tests; explicitly requested lifecycle/build checks use children."""
import argparse
import copy
from contextlib import ExitStack, contextmanager
import io
import json
import os
from pathlib import Path
import signal
import shutil
import sys
import tempfile
import tarfile
import time
import types
import unittest
from unittest.mock import patch, MagicMock

import producer_build_binding as binding
from bench_subprocess import ProducerFailure


class OwnerStateTests(unittest.TestCase):
    def test_no_signal_after_reap(self):
        for state in (binding.OwnerState.WAIT_ONLY, binding.OwnerState.REAPED):
            owner = binding.BuildOwner(deadline=time.monotonic()+30)
            owner.process = types.SimpleNamespace(pid=7)
            owner.state = state
            with patch.object(os, 'killpg') as kill:
                with self.assertRaises(ProducerFailure):
                    owner.signal_group(signal.SIGKILL)
                kill.assert_not_called()

    def test_nondefault_sigchld_rejected(self):
        with patch.object(sys, 'version_info', (3, 13)), \
                patch.object(signal, 'getsignal', return_value=signal.SIG_IGN):
            with self.assertRaises(ProducerFailure) as caught:
                binding.platform_preflight()
            self.assertEqual(caught.exception.code, 'invalid_state')

    def test_ambiguous_process_metadata_rejected(self):
        for data in (b'1 1 Z', b'1 1 Z\n1 1 Z\n', b'1 bad Z\n', b'1 1 ?\n'):
            with self.subTest(data=data), patch.object(binding, 'metadata', return_value=data):
                with self.assertRaises(ProducerFailure):
                    binding.group_members(1, time.monotonic()+3)

    def test_eperm_requires_known_zombie_only_group(self):
        for members, accepted in (({7: 'Z'}, True), ({7: 'Z', 8: 'S'}, False), ({}, False)):
            owner = binding.BuildOwner(deadline=time.monotonic()+30)
            owner.process = type('Leader', (), {'pid': 7})()
            owner.state = binding.OwnerState.RUNNING
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
                ('profile', 'debuginfo', 2.0), ('profile', 'debuginfo', True),
                ('profile', 'debuginfo', '2'), ('profile', 'debuginfo', None),
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
            with self.subTest(length=len(raw)), self.assertRaises(ProducerFailure):
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

    def test_foreign_regular_artifact_path_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            foreign = Path(temp).resolve()/'foreign'
            foreign.write_bytes(b'synthetic-foreign-artifact')
            self.event['executable'] = str(foreign)
            with self.assertRaises(ProducerFailure):
                self.events().event(self.event)


class SnapshotTests(unittest.TestCase):
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


def artifact_event(source, executable, features=('transport-stdio',)):
    return dict(reason='compiler-artifact', package_id='approved-package',
                target=dict(name='evidence_bridge', kind=['example'], crate_types=['bin'],
                            src_path=str(source/binding.SOURCE)),
                profile=dict(test=False, opt_level='0', debuginfo=2,
                             debug_assertions=True, overflow_checks=True),
                executable=str(executable), fresh=False, features=list(features))


@contextmanager
def fake_build_inputs(alias_parent=False):
    """File-backed ownership tests; no process is launched by this fixture."""
    with tempfile.TemporaryDirectory() as temp, ExitStack() as stack:
        parent = Path(temp).resolve()
        alias = parent/'alias'
        real = parent/'real'
        real.mkdir()
        alias.symlink_to(real, target_is_directory=True)
        root = alias if alias_parent else real
        inputs = binding.Inputs(root, 'a'*40, root, root, root, root)
        def prepare(root, **kwargs):
            tools = {k: root/k for k in ('cargo', 'rustc', 'rustdoc')}
            home = root/'cargo-home'
            home.mkdir()
            return tools, 'host', {'CARGO_HOME':str(home)}, {}, {}, {}, 0
        def snapshot(repo, revision, source, deadline):
            (source/binding.SOURCE).parent.mkdir(parents=True)
            (source/binding.SOURCE).write_bytes(b'prefix(PHONE, 8)')
            (source/'Cargo.lock').write_text('version = 4\n[[package]]\nname = "ort-sys"\nversion = "2.0.0-rc.12"\n')
            return binding.inventory(source, deadline), 0
        def metadata(command, **kwargs):
            source = kwargs['cwd']
            return json.dumps({'packages':[dict(name='gaze-mcp-rmcp', id='approved-package',
                       manifest_path=str(source/'crates/gaze-mcp-rmcp/Cargo.toml'))]}).encode()
        def build(owner, command, **kwargs):
            source = kwargs['cwd']
            target = Path(command[command.index('--target-dir')+1])
            executable = target/'returned-artifact'
            executable.write_bytes(b'synthetic-executable-fixture')
            event = artifact_event(source, executable,
                                   () if '--features' not in command else ('transport-stdio',))
            kwargs['consume'](json.dumps(event).encode()+b'\n')
            kwargs['consume'](b'{"reason":"build-finished","success":true}\n')
            owner.state = binding.OwnerState.REAPED
            return 0
        stack.enter_context(patch.object(binding.shutil, 'disk_usage', return_value=types.SimpleNamespace(free=30*1024**3)))
        stack.enter_context(patch.object(binding, 'prepare_inputs', side_effect=prepare))
        stack.enter_context(patch.object(binding, 'snapshot', side_effect=snapshot))
        stack.enter_context(patch.object(binding, 'metadata', side_effect=metadata))
        stack.enter_context(patch.object(binding.BuildOwner, 'run', build))
        stack.enter_context(patch.object(binding.BindingSession, 'check_inputs'))
        yield inputs


class SessionOwnershipTests(unittest.TestCase):
    def test_alias_parent_accepts_actual_selection_and_capture(self):
        with fake_build_inputs(alias_parent=True) as inputs:
            with binding.BindingSession(inputs, suite_deadline=time.monotonic()+300) as session:
                record = session.build()
                self.assertEqual(Path(record['executable']), session.executable,
                                 'returned-record-path-accepted')
                self.assertEqual(session.root, session.root.resolve(), 'alias-parent-canonical')
                session.capture()
                session.captured.compare()

                self.assertIsNotNone(session.captured.fd, 'retained-fd-after-capture')
            self.assertIsNone(session.captured.fd, 'retained-fd-closed')
            self.assertFalse(session.root.exists(), 'owned-root-retired')

    def test_one_build_and_capture_per_session(self):
        with fake_build_inputs() as inputs:
            with binding.BindingSession(inputs, suite_deadline=time.monotonic()+300) as session:
                session.build()
                with self.assertRaises(ProducerFailure):
                    session.build()
                session.capture()
                with self.assertRaises(ProducerFailure):
                    session.capture()

    def test_occupied_target_fails_before_cargo(self):
        with fake_build_inputs() as inputs:
            with self.assertRaises(ProducerFailure):
                with binding.BindingSession(inputs, suite_deadline=time.monotonic()+300) as session:
                    (session.target/'occupied').touch()
                    with patch.object(binding.BuildOwner, 'run') as launch:
                        try:
                            session.build()
                        finally:
                            launch.assert_not_called()
            self.assertFalse(session.root.exists(), 'failed-build-root-retired')

    def test_live_build_owner_cannot_authorize_capture(self):
        with fake_build_inputs() as inputs:
            original = binding.BuildOwner.run
            def unreaped(owner, *args, **kwargs):
                status = original(owner, *args, **kwargs)
                owner.state = binding.OwnerState.WAIT_ONLY
                return status
            with patch.object(binding.BuildOwner, 'run', unreaped):
                session = binding.BindingSession(inputs, suite_deadline=time.monotonic()+300).__enter__()
                try:
                    with self.assertRaises(ProducerFailure):
                        session.build()
                finally:
                    session.close()
            self.assertIsNone(session.captured, 'no-capture-before-reap')

    def test_bridge_uses_returned_artifact_with_retained_fd_and_remaining_deadline(self):
        with fake_build_inputs() as inputs:
            with binding.BindingSession(inputs, suite_deadline=time.monotonic()+300) as session:
                session.build()
                session.capture()
                observed = []
                def bridge(path, *, _owner):
                    observed.append((path, _owner.command, _owner.limits.invocation_seconds,
                                     session.captured.fd is not None))
                    return binding.evidence_bridge.BridgeSuccess(True, True)
                with patch.object(binding.evidence_bridge, 'run', side_effect=bridge):
                    self.assertTrue(session.run_bridge().numeric_verified)
                self.assertEqual(observed[0][0], session.executable, 'execute-returned-artifact')
                self.assertEqual(observed[0][1], [str(session.executable)], 'selected-command')
                self.assertLess(observed[0][2], 300, 'remaining-deadline')
                self.assertTrue(observed[0][3], 'fd-held-through-bridge')

    def test_actual_substitution_refused_before_bridge(self):
        with fake_build_inputs() as inputs:
            with self.assertRaises(ProducerFailure):
                with binding.BindingSession(inputs, suite_deadline=time.monotonic()+300) as session:
                    session.build()
                    session.capture()
                    other = session.target/'replacement'
                    other.write_bytes(b'synthetic-alternate')
                    other.replace(session.executable)
                    with patch.object(binding.evidence_bridge, 'run') as launch:
                        try:
                            session.run_bridge()
                        finally:
                            launch.assert_not_called()

    def test_post_bridge_byte_change_with_restored_metadata_refused(self):
        with fake_build_inputs() as inputs:
            with binding.BindingSession(inputs, suite_deadline=time.monotonic()+300) as session:
                session.build()
                session.capture()
                old = session.executable.stat()
                data = session.executable.read_bytes()
                def bridge(*args, **kwargs):
                    session.executable.write_bytes(b'x'*len(data))
                    os.utime(session.executable, ns=(old.st_atime_ns, old.st_mtime_ns))
                    return binding.evidence_bridge.BridgeSuccess(True, True)
                try:
                    with patch.object(binding.evidence_bridge, 'run', side_effect=bridge):
                        with self.assertRaises(ProducerFailure) as caught:
                            session.run_bridge()
                        self.assertEqual(caught.exception.code, 'io', 'post-bridge-byte-comparison')
                finally:
                    session.executable.write_bytes(data)
                    os.utime(session.executable, ns=(old.st_atime_ns, old.st_mtime_ns))

    def test_input_refusal_occurs_before_bridge_launch(self):
        with fake_build_inputs() as inputs:
            with binding.BindingSession(inputs, suite_deadline=time.monotonic()+300) as session:
                session.build()
                session.capture()
                with patch.object(session, 'check_inputs', side_effect=ProducerFailure('io')), \
                        patch.object(binding.evidence_bridge, 'run') as launch:
                    with self.assertRaises(ProducerFailure):
                        session.run_bridge()
                    launch.assert_not_called()

    def test_unused_conventional_path_is_positive_control(self):
        with fake_build_inputs() as inputs:
            with binding.BindingSession(inputs, suite_deadline=time.monotonic()+300) as session:
                session.build()
                session.capture()
                conventional = session.target/'debug/examples/evidence_bridge'
                conventional.parent.mkdir(parents=True)
                conventional.write_bytes(b'synthetic-unused-path')
                session.captured.compare()
                with patch.object(binding.evidence_bridge, 'run',
                                  return_value=binding.evidence_bridge.BridgeSuccess(True, True)):
                    self.assertTrue(session.run_bridge().numeric_verified)

    def test_whole_entry_preflight_error_is_closed(self):
        with patch.object(binding.BindingSession, '__enter__', side_effect=ValueError('private setup')):
            with self.assertRaises(ProducerFailure) as caught:
                binding.run_binding(None, suite_deadline=time.monotonic()+300)
        self.assertIsNone(caught.exception.__context__, 'no-ambient-context')
        self.assertNotIn('private setup', str(caught.exception), 'no-private-error')

    def test_cleanup_failure_invalidates_success(self):
        with fake_build_inputs() as inputs, \
                patch.object(binding.evidence_bridge, 'run',
                             return_value=binding.evidence_bridge.BridgeSuccess(True, True)), \
                patch.object(binding, 'remove_owned', side_effect=OSError('private cleanup')):
            with self.assertRaises(ProducerFailure) as caught:
                binding.run_binding(inputs, suite_deadline=time.monotonic()+300)
            self.assertIsNone(caught.exception.__context__, 'cleanup-error-closed')

    def test_retiring_products_keeps_only_selected_binary(self):
        with fake_build_inputs() as inputs:
            with binding.BindingSession(inputs, suite_deadline=time.monotonic()+300) as session:
                session.build()
                session.capture()
                (session.target/'unused').write_bytes(b'compiled-fixture')
                session.retire_build_products()
                self.assertEqual(list(session.target.iterdir()), [session.executable])
                session.captured.compare()


class InputContextTests(unittest.TestCase):
    def test_private_cache_and_minimal_native_environment(self):
        with tempfile.TemporaryDirectory() as temp:
            parent = Path(temp).resolve()
            root, registry, native, tools = (parent/x for x in ('owned', 'registry', 'native', 'toolchain'))
            for path in (root, registry, native, tools/'bin'):
                path.mkdir(parents=True)
            for name in ('index', 'cache', 'src'):
                (registry/name).mkdir()
                (registry/name/'fixture').write_bytes(b'synthetic-cache')
            (parent/'config.toml').write_bytes(b'not-an-input')
            (native/'libonnxruntime.a').write_bytes(b'synthetic-archive')
            for name in ('cargo', 'rustc', 'rustdoc'):
                (tools/'bin'/name).write_bytes(b'synthetic-tool')
            def versions(command, **kwargs):
                env = kwargs['env']
                self.assertNotIn('RUSTFLAGS', env, 'tool-check-no-inherited-flags')
                self.assertNotIn('DYLD_LIBRARY_PATH', env, 'tool-check-no-loader')
                return (b'host: synthetic-host\n' if command[-1] == '-vV'
                        else (Path(command[0]).name+' 1.96.0 (fixture)\n').encode())
            with patch.object(binding, 'platform_preflight'), \
                    patch.object(binding.shutil, 'disk_usage', return_value=types.SimpleNamespace(free=20*1024**3)), \
                    patch.object(binding, 'metadata', side_effect=versions), \
                    patch.object(binding, 'verify_native_archive'), \
                    patch.dict(os.environ, {'RUSTFLAGS':'--cfg synthetic', 'DYLD_LIBRARY_PATH':'private'}):
                result = binding.prepare_inputs(root, registry=registry, native=native,
                                                toolchain=tools, deadline=time.monotonic()+30)
            env = result[2]
            self.assertEqual(env['CARGO_NET_OFFLINE'], 'true', 'offline-environment')
            self.assertEqual(env['ORT_SKIP_DOWNLOAD'], 'true', 'skip-native-download')
            self.assertEqual(env['ORT_LIB_LOCATION'], str(native), 'explicit-native')
            self.assertEqual(env['RUSTC'], str(tools/'bin/rustc'), 'explicit-rustc')
            self.assertEqual(env['RUSTDOC'], str(tools/'bin/rustdoc'), 'explicit-rustdoc')
            for key in ('ORT_LIB_PATH', 'ORT_PREFER_DYNAMIC_LINK', 'ORT_LIB_PROFILE',
                        'ORT_VCPKG_TARGET', 'ORT_CXX_STDLIB', 'RUSTFLAGS', 'RUSTC_WRAPPER',
                        'DYLD_LIBRARY_PATH', 'LD_PRELOAD', 'HTTPS_PROXY'):
                self.assertNotIn(key, env, 'no-inherited-injection')
            cargo_home = Path(env['CARGO_HOME'])
            self.assertFalse((cargo_home/'registry/src').exists(), 'no-shared-extracted-source')
            for name in ('index', 'cache'):
                path = cargo_home/'registry'/name/'fixture'
                self.assertEqual(path.read_bytes(), b'synthetic-cache')
                self.assertNotEqual(path.stat().st_ino, (registry/name/'fixture').stat().st_ino,
                                    'cache-not-shared-inode')
                self.assertEqual(path.stat().st_mode & 0o777, 0o444, 'private-cache-read-only')
            for name in ('config', 'config.toml', 'credentials', 'credentials.toml'):
                self.assertFalse((cargo_home/name).exists(), 'no-cargo-configuration')

    def test_missing_or_symlink_native_archive_refused(self):
        for symlink, code in ((False, 'invalid_state'), (True, 'io')):
            with self.subTest(symlink=symlink), tempfile.TemporaryDirectory() as temp:
                parent = Path(temp).resolve()
                root, native, tools, registry = (parent/x for x in ('root', 'native', 'tools', 'registry'))
                for path in (root, native, tools/'bin', registry/'index', registry/'cache'):
                    path.mkdir(parents=True)
                for name in ('cargo', 'rustc', 'rustdoc'):
                    (tools/'bin'/name).write_bytes(b'synthetic-tool')
                if symlink:
                    (parent/'outside').write_bytes(b'synthetic-archive')
                    (native/'libonnxruntime.a').symlink_to(parent/'outside')
                def versions(command, **kwargs):
                    return (b'host: synthetic-host\n' if command[-1] == '-vV'
                            else (Path(command[0]).name+' 1.96.0 (fixture)\n').encode())
                with patch.object(binding, 'platform_preflight'), \
                        patch.object(binding.shutil, 'disk_usage', return_value=types.SimpleNamespace(free=20*1024**3)), \
                        patch.object(binding, 'metadata', side_effect=versions), \
                        patch.object(binding, 'verify_native_archive'):
                    with self.assertRaises(ProducerFailure) as caught:
                        binding.prepare_inputs(root, registry=registry, native=native,
                                               toolchain=tools, deadline=time.monotonic()+30)
                    self.assertEqual(caught.exception.code, code)

    def test_insufficient_disk_refused_before_setup(self):
        with patch.object(binding, 'platform_preflight'), \
                patch.object(binding.shutil, 'disk_usage', return_value=types.SimpleNamespace(free=20*1024**3-1)), \
                patch.object(Path, 'mkdir') as mkdir:
            with self.assertRaises(ProducerFailure) as caught:
                binding.prepare_inputs(Path('/synthetic'), registry=Path('/synthetic'),
                                       native=Path('/synthetic'), toolchain=Path('/synthetic'),
                                       deadline=time.monotonic()+30)
            self.assertEqual(caught.exception.code, 'input_limit')
            mkdir.assert_not_called()

    def test_native_archive_host_mismatch_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)/'libonnxruntime.a'
            payload = b'\x7fELF\x02\x01'+b'\0'*12+binding.struct.pack('<H', 62)
            header = b'fixture.o/      '+b'0           '+b'0     '+b'0     '+b'100644  '+b'20        '+b'`\n'
            self.assertEqual(len(header), 60)
            path.write_bytes(b'!<arch>\n'+header+payload)
            binding.verify_native_archive(path, 'x86_64-unknown-linux-gnu', time.monotonic()+3)
            with self.assertRaises(ProducerFailure):
                binding.verify_native_archive(path, 'aarch64-unknown-linux-gnu', time.monotonic()+3)


class InputRecheckTests(unittest.TestCase):
    def test_external_suite_budget_cannot_expand_ceiling(self):
        for budget in (0, 10, 7201, float('inf'), float('nan')):
            with self.subTest(budget=budget), self.assertRaises(ProducerFailure) as caught:
                integration(None, suite_seconds=budget)
            self.assertEqual(caught.exception.code, 'invalid_limits')

    def test_source_and_private_cache_rechecked(self):
        check_inputs = binding.BindingSession.check_inputs
        with fake_build_inputs() as inputs:
            with binding.BindingSession(inputs, suite_deadline=time.monotonic()+300) as session:
                session.build()
                session.capture()
                session.native = session.root/'native'
                session.native.mkdir()
                session.native_before, session.tools, session.tools_before = {}, {}, {}
                check_inputs(session)
                lock = session.source/'Cargo.lock'
                saved = lock.read_bytes()
                try:
                    lock.write_bytes(saved+b'\n')
                    with self.assertRaises(ProducerFailure):
                        check_inputs(session)
                finally:
                    lock.write_bytes(saved)
                session.registry = session.root/'approved-registry'
                (session.registry/'cache').mkdir(parents=True)
                (session.registry/'cache/fixture').write_bytes(b'synthetic-cache')
                session.cache_before = {'cache':binding.inventory(session.registry/'cache', session.work_end)}
                private = Path(session.env['CARGO_HOME'])/'registry/cache/fixture'
                private.parent.mkdir(parents=True)
                private.write_bytes(b'synthetic-cache')
                private.chmod(0o444)
                check_inputs(session)
                private.chmod(0o644)
                private.write_bytes(b'changed-private-cache')
                with self.assertRaises(ProducerFailure):
                    check_inputs(session)

    def test_unsupported_tar_extensions_refused(self):
        for kind in (tarfile.XHDTYPE, tarfile.XGLTYPE, tarfile.GNUTYPE_LONGNAME,
                     tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE):
            header = tarfile.TarInfo('fixture')
            header.type = kind
            raw = header.tobuf(format=tarfile.USTAR_FORMAT)+bytes(1024)
            with self.subTest(kind=kind), self.assertRaises(ProducerFailure):
                binding.validate_tar_headers(raw)

    def test_false_or_missing_completion_cannot_capture(self):
        for success in (False, 1, None):
            events = binding.CargoEvents('fixture', Path('/source'), Path('/target'))
            with self.subTest(success=success), self.assertRaises(ProducerFailure):
                events.event({'reason':'build-finished', 'success':success})

    def test_full_suite_preserves_control_across_three_builds_without_reconstruction(self):
        with fake_build_inputs() as inputs:
            build = binding.BuildOwner.run
            builds = []
            def owner_run(owner, command, **kwargs):
                if '--target-dir' not in command:
                    owner.state = binding.OwnerState.REAPED
                    return 2
                builds.append(Path(command[command.index('--target-dir')+1]))
                return build(owner, command, **kwargs)
            def check_inputs(session):
                binding.require(binding.inventory(session.source, session.work_end)
                                == session.source_before, 'io')
            def bridge(path, *, _owner):
                source = Path(_owner.cwd)
                if b'prefix(PHONE, 9)' in (source/binding.SOURCE).read_bytes():
                    binding.evidence_bridge.validate_numeric(None, {})
                return binding.evidence_bridge.BridgeSuccess(True, True)
            with patch.object(binding.BuildOwner, 'run', owner_run), \
                    patch.object(binding.BindingSession, 'check_inputs', check_inputs), \
                    patch.object(binding.evidence_bridge, 'run', side_effect=bridge), \
                    patch.dict(globals(), {'lifecycle':lambda end: {}}):
                result = integration(inputs)
            self.assertEqual(result['genuine_builds'], 3, 'suite-three-builds')
            self.assertEqual(len(set(builds)), 3, 'three-distinct-fresh-targets')
            self.assertTrue(all(not path.exists() for path in builds), 'all-targets-retired')



class BindingRegressionTests(unittest.TestCase):
    def test_spawn_bookkeeping_cancellation_and_normal_control(self):
        import inspect
        lines, first = inspect.getsourcelines(binding.BuildOwner.run.__wrapped__)
        body = {first+i: line for i, line in enumerate(lines)}
        for transition in ('prestate', 'poststate', 'normal'):
            with self.subTest(transition=transition):
                owner = binding.BuildOwner(deadline=time.monotonic()+30)
                log = []
                process = MagicMock(pid=7, returncode=None)
                def wait(**kwargs):
                    log.append(('wait', owner.state))
                    process.returncode = 0
                    return 0
                process.wait.side_effect = wait
                observed = types.SimpleNamespace(si_status=0, si_code=os.CLD_EXITED)
                selector = MagicMock()
                selector.__enter__.return_value = selector
                selector.get_map.return_value = {}
                settled_calls = 0
                def settled(*args):
                    nonlocal settled_calls
                    settled_calls += 1
                    clean = transition == 'normal' or settled_calls > 1
                    if clean:
                        owner.observed = observed
                    return clean
                fired = False
                def trace(frame, event, arg):
                    nonlocal fired
                    if (event == 'line' and frame.f_code.co_name == 'run'
                            and frame.f_lineno in body and owner.process is process and not fired):
                        ready = (transition == 'prestate' and owner.state == binding.OwnerState.NEW
                                 or transition == 'poststate' and owner.state == binding.OwnerState.RUNNING)
                        if ready:
                            fired = True
                            raise KeyboardInterrupt('discarded synthetic context')
                    return trace
                with patch.object(binding, 'platform_preflight'), \
                        patch.object(binding.subprocess, 'Popen', return_value=process), \
                        patch.object(binding.selectors, 'DefaultSelector', return_value=selector), \
                        patch.object(os, 'set_blocking'), \
                        patch.object(os, 'waitid', return_value=observed), \
                        patch.object(owner, 'settled', side_effect=settled), \
                        patch.object(os, 'killpg', side_effect=lambda *args: log.append(('signal', owner.state))):
                    sys.settrace(trace)
                    try:
                        if transition == 'normal':
                            self.assertEqual(owner.run(['synthetic']), 0)
                        else:
                            with self.assertRaises(ProducerFailure) as caught:
                                owner.run(['synthetic'])
                            self.assertEqual(caught.exception.code, 'cancelled')
                    finally:
                        sys.settrace(None)
                self.assertTrue(owner.reaped, 'returned-process-must-be-reaped')
                self.assertEqual(log[-1], ('wait', binding.OwnerState.WAIT_ONLY))
                self.assertEqual(owner.signals, [] if transition == 'normal' else [signal.SIGTERM])
                process.stdout.close.assert_called_once()
                process.stderr.close.assert_called_once()

    def test_metadata_bookkeeping_cancellation(self):
        process = MagicMock(returncode=None)
        fired = False
        def trace(frame, event, arg):
            nonlocal fired
            if event == 'line' and frame.f_code.co_name == 'metadata' and frame.f_locals.get('p') is process and not fired:
                fired = True
                raise KeyboardInterrupt()
            return trace
        with patch.object(binding.subprocess, 'Popen', return_value=process):
            sys.settrace(trace)
            try:
                with self.assertRaises(KeyboardInterrupt):
                    binding.metadata(['synthetic'])
            finally:
                sys.settrace(None)
        process.kill.assert_called_once()
        process.wait.assert_called_once()
        process.stdout.close.assert_called_once()
        process.stderr.close.assert_called_once()

    def test_nonregular_opens_are_nonblocking_and_never_read(self):
        for mode in (binding.stat.S_IFIFO, binding.stat.S_IFCHR, binding.stat.S_IFSOCK):
            with self.subTest(mode=mode), patch.object(os, 'open', return_value=7) as opened, \
                    patch.object(os, 'fstat', return_value=types.SimpleNamespace(st_mode=mode)), \
                    patch.object(os, 'close') as close, patch.object(os, 'read') as read:
                with self.assertRaises(ProducerFailure) as caught:
                    binding.file_digest(Path('/synthetic'), time.monotonic()+3)
                self.assertEqual(caught.exception.code, 'io')
                flags = opened.call_args.args[1]
                self.assertEqual(flags & (os.O_NONBLOCK | os.O_NOFOLLOW), os.O_NONBLOCK | os.O_NOFOLLOW)
                read.assert_not_called()
                close.assert_called_once_with(7)

    def test_directory_cleanup_reservation_and_expired_final_deadline(self):
        with fake_build_inputs() as inputs:
            session = binding.BindingSession(inputs, suite_deadline=time.monotonic()+300).__enter__()
            self.assertGreaterEqual(session.deadline-session.work_end, 130, 'separate-process-and-directory-reserves')
            with patch.object(binding.time, 'monotonic', return_value=session.work_end+1):
                session.close()
            self.assertFalse(session.root.exists(), 'cleanup-survives-work-expiry')
            session = binding.BindingSession(inputs, suite_deadline=time.monotonic()+300).__enter__()
            with patch.object(binding.time, 'monotonic', return_value=session.deadline):
                with self.assertRaises(ProducerFailure) as caught:
                    session.close()
                self.assertEqual(caught.exception.code, 'deadline')
                self.assertNotEqual(session.state, binding.BindingState.CLOSED)
                self.assertTrue(session.root.exists())
            session.close()

    def test_free_delta_uses_canonical_scratch_filesystem(self):
        with fake_build_inputs(alias_parent=True) as inputs:
            calls = []
            def usage(path):
                calls.append(path)
                return types.SimpleNamespace(free=30*1024**3-len(calls))
            with patch.object(binding.shutil, 'disk_usage', side_effect=usage):
                session = binding.BindingSession(inputs, suite_deadline=time.monotonic()+300).__enter__()
                session.repo = Path('/different-filesystem')
                session.close()
            self.assertEqual(calls, [inputs.scratch_parent.resolve()]*2)
            self.assertEqual(session.measurements['free_delta_bytes'], 1)

    def test_suite_lifecycle_cannot_launch_after_budget_exhaustion(self):
        now = [100.0]
        launches = []
        def run(owner, *args, **kwargs):
            launches.append(owner.deadline)
            now[0] = 111.0
            owner.state = binding.OwnerState.REAPED
            owner.process = types.SimpleNamespace(returncode=0)
            return 0
        with patch.object(binding.time, 'monotonic', side_effect=lambda: now[0]), \
                patch.object(binding.BuildOwner, 'run', run):
            with self.assertRaises(ProducerFailure) as caught:
                integration(None, suite_seconds=11)
            self.assertEqual(caught.exception.code, 'deadline')
        self.assertEqual(len(launches), 1, 'no-second-launch')
        self.assertLessEqual(launches[0], 111)

    def test_metadata_has_its_own_four_mib_parse_budget(self):
        raw = b'{"padding":"'+b'x'*(2*binding.MIB)+b'"}'
        self.assertEqual(len(binding.closed_json(raw, cap=4*binding.MIB)['padding']), 2*binding.MIB)
        with self.assertRaises(ProducerFailure) as caught:
            binding.closed_json(raw)
        self.assertEqual(caught.exception.code, 'output_limit')
        with self.assertRaises(ProducerFailure):
            binding.closed_json(b' '*(4*binding.MIB)+b'{}', cap=4*binding.MIB)
        with fake_build_inputs() as inputs:
            original = binding.metadata
            with patch.object(binding, 'metadata', wraps=original) as metadata:
                with binding.BindingSession(inputs, suite_deadline=time.monotonic()+300) as session:
                    session.build()
                    session.capture()
            self.assertEqual(metadata.call_args.kwargs['cap'], 4*binding.MIB)

    def test_workflow_native_cache_has_no_legacy_restore(self):
        workflow = (Path(__file__).resolve().parents[2]/'.github/workflows/test.yml').read_text()
        cache = workflow.split('  test:\n', 1)[1].split('      - name: Cache cargo registry + target', 1)[1].split('      - name:', 1)[0]
        native = '~/.cache/ort.pyke.io/dfbin/x86_64-unknown-linux-gnu/acc1cba79c337594ead1d88ca72516147aa60054c84217b53399a31caa5ba671'
        self.assertIn(native, cache, 'warm-target-restores-exact-native')
        self.assertIn('-cargo-test-native-v2-', cache)
        self.assertNotIn('-cargo-test-${{', cache, 'legacy-exact-key-excluded')
        self.assertNotIn('-cargo-test-\n', cache, 'legacy-prefix-excluded')
        self.assertLess(workflow.index('      - name: cargo test\n'), workflow.index('      - name: Test private fresh-build'))


def lifecycle(suite_deadline):
    results = {}
    programs = {
        'normal_leader': 'pass',
        'leader_first_closed_pipes':
            'import os,time\nif os.fork()==0:\n os.close(1);os.close(2);time.sleep(30)\nelse:\n os._exit(0)',
        'term_ignoring_descendant':
            'import os,time,signal\nif os.fork()==0:\n signal.signal(signal.SIGTERM,signal.SIG_IGN);os.close(1);os.close(2);time.sleep(30)\nelse:\n time.sleep(.1);os._exit(0)',
        'timeout': 'import time;time.sleep(30)',
        'cancellation': 'import os,time;os.write(1,b"ready");time.sleep(30)',
        'leader_first_open_pipes':
            'import os,time\nif os.fork()==0:\n time.sleep(30)\nelse:\n os._exit(0)',
        'stdout_overflow': 'import os;os.write(1,b"x"*2048)',
        'stderr_overflow': 'import os;os.write(2,b"x"*2048)',
        'non_json_stdout': 'print("synthetic noise")',
    }
    expected_codes = dict(leader_first_closed_pipes='cleanup', term_ignoring_descendant='cleanup',
                          timeout='deadline', cancellation='cancelled', leader_first_open_pipes='cleanup',
                          stdout_overflow='output_limit', stderr_overflow='output_limit', non_json_stdout='protocol')
    def owner_for(**kwargs):
        binding.check_time(suite_deadline-binding.PROCESS_RESERVE)
        return binding.BuildOwner(deadline=min(suite_deadline, time.monotonic()+15), **kwargs)
    for name, program in programs.items():
        owner = owner_for(
                                   seconds=.15 if name == 'timeout' else 3,
                                   cap=1024 if name.endswith('overflow') else 64*binding.MIB)
        parser = binding.CargoEvents('fixture', Path('/source'), Path('/target'))
        def consume(chunk):
            if name == 'cancellation':
                raise KeyboardInterrupt('private discarded context')
            if name == 'non_json_stdout':
                parser.feed(chunk)
        started = time.monotonic()
        try:
            status = owner.run([sys.executable, '-c', program], consume=consume)
        except ProducerFailure as error:
            assert name != 'normal_leader', 'normal-leader-must-pass'
            assert error.__context__ is None and error.__cause__ is None, 'closed-error'
            assert error.code == expected_codes[name], 'lifecycle-exact-code'
            results[name] = error.code
        else:
            assert name == 'normal_leader' and status == 0, 'live-descendant-must-refuse'
            assert owner.signals == [], 'normal-leader-must-not-be-killed'
            results[name] = 'accepted'
        assert owner.reaped and owner.process.returncode is not None, 'owned-leader-reaped'
        assert time.monotonic()-started < 15, 'finite-cleanup'
        if name == 'leader_first_open_pipes':
            assert time.monotonic()-started < 3, 'open-pipe-grace-before-build-timeout'
        if name == 'term_ignoring_descendant':
            assert signal.SIGKILL in owner.signals, 'term-ignore-needs-kill'
    for status in (0, 7):
        parser = binding.CargoEvents('fixture', Path('/source'), Path('/target'))
        program = ('import os,sys\nos.write(1,b\'{"reason":"compiler-message"}\\n\')\n'
                   'os.write(1,b\'{"reason":"build-\')\n'
                   'os.write(1,b\'finished","success":true}\\n\')\n'
                   f'sys.exit({status})')
        owner = owner_for(seconds=3)
        actual = owner.run([sys.executable, '-c', program], consume=parser.feed)
        assert actual == status and parser.finished and not parser.pending, 'stream-exit-independent'
        assert owner.reaped and not owner.signals, 'stream-clean-exit'
        results['split_stream_exit_'+str(status)] = 'observed'
    for operation in ('digest', 'inventory', 'native', 'capture', 'compare'):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            fifo = root/'fifo'
            os.mkfifo(fifo)
            program = """import sys,time
from pathlib import Path
import producer_build_binding as b
root = Path(sys.argv[1])
operation = sys.argv[2]
end = time.monotonic()+2
captured = None
try:
    if operation == 'compare':
        regular = root/'regular'
        regular.write_bytes(b'synthetic')
        captured = b.CapturedArtifact(regular, end)
        regular.unlink()
        (root/'fifo').rename(regular)
        captured.compare()
    elif operation == 'capture':
        b.CapturedArtifact(root/'fifo', end)
    elif operation == 'native':
        b.verify_native_archive(root/'fifo', 'x86_64-unknown-linux-gnu', end)
    elif operation == 'inventory':
        b.inventory(root, end)
    else:
        b.file_digest(root/'fifo', end)
except b.ProducerFailure as error:
    sys.exit(0 if error.code == 'io' else 3)
else:
    sys.exit(4)
finally:
    if captured is not None:
        captured.close()
"""
            owner = owner_for(seconds=3)
            started = time.monotonic()
            status = owner.run([sys.executable, '-B', '-c', program, str(root), operation],
                               cwd=Path(binding.__file__).resolve().parent)
            assert status == 0 and owner.reaped and not owner.signals, 'fifo-prompt-closed-refusal'
            assert time.monotonic()-started < 3, 'fifo-no-blocking-open'
            results['fifo_'+operation] = 'io'
    return results


def refused(call, label, *, code=None):
    try:
        call()
    except ProducerFailure as error:
        assert code is None or error.code == code, label+'-wrong-refusal'
        assert error.__context__ is None and error.__cause__ is None, label+'-private-context'
    else:
        raise AssertionError(label+'-survived')


def foreign_record_refusal(control, foreign, label):
    # This is the same operation capture() calls before creating its baseline fd.
    with patch.object(binding, 'CapturedArtifact') as capture:
        try:
            control.capture_record(copy.deepcopy(foreign.events.selected_record))
        except ProducerFailure:
            pass
        else:
            raise AssertionError(label+'-foreign-record-accepted')
        capture.assert_not_called()
    # Isolate path and source checks using the real foreign build's fields.
    # These deliberate injections are not claimed to be authentic Cargo testimony.
    for field in ('executable', 'src_path'):
        record = copy.deepcopy(control.events.selected_record)
        if field == 'executable':
            record[field] = foreign.events.selected_record[field]
        else:
            record['target'][field] = foreign.events.selected_record['target'][field]
        with patch.object(binding, 'CapturedArtifact') as capture:
            try:
                control.capture_record(record)
            except ProducerFailure:
                pass
            else:
                raise AssertionError(label+'-'+field+'-accepted')
            capture.assert_not_called()
    # Isolate the feature predicate with a genuine no-transport feature record.
    if foreign.variant == 'no_transport':
        record = copy.deepcopy(control.events.selected_record)
        record['features'] = foreign.events.selected_record['features']
        with patch.object(binding, 'CapturedArtifact') as capture:
            try:
                control.capture_record(record)
            except ProducerFailure:
                pass
            else:
                raise AssertionError(label+'-wrong-features-accepted')
            capture.assert_not_called()


def substitution_refusal(control, foreign, label):
    backup = control.executable.with_name('retained-original')
    control.executable.rename(backup)
    try:
        shutil.copyfile(foreign.executable, control.executable)
        control.executable.chmod(0o755)
        with patch.object(binding.evidence_bridge, 'run', wraps=binding.evidence_bridge.run) as launch:
            refused(control.run_bridge, label+'-after-capture', code='io')
            launch.assert_not_called()
    finally:
        control.executable.unlink(missing_ok=True)
        backup.rename(control.executable)
    control.captured.compare()


@binding.producer_boundary
def integration(inputs, *, suite_seconds=7200):
    """Exactly three fresh builds; there is no existing-binary input or skip path."""
    start = time.monotonic()
    binding.require(type(suite_seconds) in (int, float) and binding.math.isfinite(suite_seconds)
                    and 10 < suite_seconds <= 7200, 'invalid_limits')
    end = start+suite_seconds
    checks = lifecycle(end)
    with binding.BindingSession(inputs, suite_deadline=end) as control:
        control.build()
        lock = control.source/'Cargo.lock'
        original = lock.read_bytes()
        try:
            lock.write_bytes(original+b'\n')
            refused(control.capture, 'dirty-snapshot-before-capture', code='io')
        finally:
            lock.write_bytes(original)
        control.capture()
        assert control.run_bridge().numeric_verified, 'genuine-control-bridge'
        checks['genuine_control'] = 'passed'
        checks['dirty_snapshot_before_capture'] = 'refused'
        control.retire_build_products()
        # Each alternate retires before the next build; only the comparator
        # control's returned executable is held alongside the current target.
        with binding.BindingSession(inputs, suite_deadline=end, variant='no_transport') as no_transport:
            no_transport.build()
            no_transport.capture()
            owner = binding.BuildOwner(deadline=no_transport.work_end, seconds=30)
            assert owner.run([str(no_transport.executable)], cwd=no_transport.source,
                             env=no_transport.env) == 2, 'no-transport-valid-setup-exit-two'
            no_transport.captured.compare()
            foreign_record_refusal(control, no_transport, 'no-transport')
            substitution_refusal(control, no_transport, 'no-transport')
            conventional = control.target/'debug/examples/evidence_bridge'
            assert conventional != control.executable, 'unused-path-distinct'
            conventional.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(no_transport.executable, conventional)
            conventional.chmod(0o755)
            assert control.run_bridge().numeric_verified, 'unused-path-positive-control'
            checks['no_transport_before_and_after_capture'] = 'refused'
            checks['unused_conventional_path'] = 'positive_control'
        with binding.BindingSession(inputs, suite_deadline=end, variant='alternate') as alternate:
            alternate.build()
            alternate.capture()
            assert alternate.inventory_identity != control.inventory_identity, 'alternate-inventory-distinct'
            assert alternate.snapshot_identity == 'parent-mutation:partial-prefix-nine', 'named-snapshot-mutation'
            with patch.object(binding.evidence_bridge, 'validate_numeric',
                              wraps=binding.evidence_bridge.validate_numeric) as numeric:
                refused(alternate.run_bridge, 'alternate-numeric-behavior', code='protocol')
                assert numeric.call_count == 1, 'alternate-valid-protocol-reaches-numeric-oracle'
            foreign_record_refusal(control, alternate, 'alternate-source')
            substitution_refusal(control, alternate, 'alternate-source')
            checks['alternate_source_before_and_after_capture'] = 'refused'
            checks['alternate_numeric_oracle'] = 'behavior_only'
        config = control.source/'.cargo/config.toml'
        config.parent.mkdir(exist_ok=True)
        try:
            config.write_bytes(b'# synthetic unexpected configuration\n')
            with patch.object(binding.evidence_bridge, 'run') as launch:
                refused(control.run_bridge, 'dirty-config-before-launch', code='io')
                launch.assert_not_called()
        finally:
            config.unlink()
            config.parent.rmdir()
        checks['dirty_config_before_launch'] = 'refused'
        assert control.run_bridge().numeric_verified, 'restored-control-still-valid'
    binding.check_time(end)
    return dict(checks=checks, builds=[control.measurements, no_transport.measurements, alternate.measurements],
                genuine_builds=3, suite_seconds=round(time.monotonic()-start, 3),
                suite_budget_seconds=suite_seconds)


def mutation_proof():
    """Execute actual changed helper code; errors and timeouts never count as kills."""
    global binding
    original = binding
    source = Path(original.__file__).read_text()
    classes = (OwnerStateTests, CargoSelectionTests, SnapshotTests,
               SessionOwnershipTests, InputContextTests, InputRecheckTests, BindingRegressionTests)
    def run_tests(only=None):
        if only is None:
            suite = unittest.TestSuite(unittest.defaultTestLoader.loadTestsFromTestCase(c) for c in classes)
        else:
            class_name, method = only.split('.')
            case = next(c for c in classes if c.__name__ == class_name)
            suite = unittest.TestSuite([case(method)])
        result = unittest.TestResult()
        suite.run(result)
        return result
    baseline = run_tests()
    assert not baseline.errors and not baseline.failures, 'mutation-baseline-must-pass'
    roster = [
        ('fresh', "and event.get('fresh') is False", '', 'CargoSelectionTests.test_closed_selected_fields'),
        ('source', "and target.get('src_path') == str(self.source)", '', 'CargoSelectionTests.test_target_identity_and_profile'),
        ('profile', "and profile.get('test') is False", '', 'CargoSelectionTests.test_target_identity_and_profile'),
        ('features', "event.get('features') == self.features and", '', 'CargoSelectionTests.test_closed_selected_fields'),
        ('unique', 'require(self.selected is None)', 'pass', 'CargoSelectionTests.test_duplicate_candidate_and_completion'),
        ('exit', 'and status == 0)', ')', 'CargoSelectionTests.test_duplicate_candidate_and_completion'),
        ('finished', "require(event.get('success') is True)", 'pass', 'InputRecheckTests.test_false_or_missing_completion_cannot_capture'),
        ('owned_path', 'and path.is_relative_to(self.target)', '', 'CargoSelectionTests.test_foreign_regular_artifact_path_refused'),
        ('signal_order', "require(self.process is not None and self.state == OwnerState.RUNNING, 'invalid_state')",
         "require(self.process is not None, 'invalid_state')", 'OwnerStateTests.test_no_signal_after_reap'),
        ('reap_before_capture', "require(self.owner.reaped, 'cleanup')", 'pass', 'SessionOwnershipTests.test_live_build_owner_cannot_authorize_capture'),
        ('empty_target', "require(not any(self.target.iterdir()), 'invalid_state')", 'pass', 'SessionOwnershipTests.test_occupied_target_fails_before_cargo'),
        ('bytes', 'return identity(before), digest.digest()', "return identity(before), b''", 'SessionOwnershipTests.test_post_bridge_byte_change_with_restored_metadata_refused'),
        ('pre_compare', 'self.check_inputs()\n        self.captured.compare()\n        # B1',
         'self.check_inputs()\n        # B1', 'SessionOwnershipTests.test_actual_substitution_refused_before_bridge'),
        ('post_compare', 'finally:\n            self.captured.compare()\n            self.check_inputs()\n        return result',
         'finally:\n            self.check_inputs()\n        return result', 'SessionOwnershipTests.test_post_bridge_byte_change_with_restored_metadata_refused'),
        ('source_recheck', "require(inventory(self.source, self.work_end) == self.source_before, 'io')", 'pass',
         'InputRecheckTests.test_source_and_private_cache_rechecked'),
        ('offline', "'CARGO_NET_OFFLINE': 'true'", "'CARGO_NET_OFFLINE': 'false'", 'InputContextTests.test_private_cache_and_minimal_native_environment'),
        ('skip_download', "'ORT_SKIP_DOWNLOAD': 'true'", "'ORT_SKIP_DOWNLOAD': 'false'", 'InputContextTests.test_private_cache_and_minimal_native_environment'),
        ('config_absence', "require(not (directory/'.cargo'/name).exists(), 'invalid_state')", 'pass',
         'SnapshotTests.test_config_presence_without_reading_contents'),
        ('cleanup', 'remove_owned(self.root, self.deadline)', 'pass', 'SessionOwnershipTests.test_cleanup_failure_invalidates_success'),
        ('selected_command', '[str(self.executable)], cwd=self.source',
         "[str(self.target/'debug/examples/evidence_bridge')], cwd=self.source",
         'SessionOwnershipTests.test_bridge_uses_returned_artifact_with_retained_fd_and_remaining_deadline'),
    ]
    roster.extend([
        ('returned_process_cleanup', 'if self.process is not None:\n                # A returned process',
         'if self.process is not None and not failure:\n                # A returned process',
         'BindingRegressionTests.test_spawn_bookkeeping_cancellation_and_normal_control'),
        ('metadata_returned_process_cleanup', 'if p is not None:\n            try:',
         'if False:\n            try:', 'BindingRegressionTests.test_metadata_bookkeeping_cancellation'),
        ('nonblocking_open', 'os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK',
         'os.O_RDONLY | os.O_NOFOLLOW', 'BindingRegressionTests.test_nonregular_opens_are_nonblocking_and_never_read'),
        ('native_presence', "require('libonnxruntime.a' in native_inventory, 'invalid_state')", 'pass',
         'InputContextTests.test_missing_or_symlink_native_archive_refused'),
        ('disk_bound', "require(shutil.disk_usage(root).free >= 20*1024**3, 'input_limit')",
         "require(shutil.disk_usage(root).free >= 19*1024**3, 'input_limit')",
         'InputContextTests.test_insufficient_disk_refused_before_setup'),
        ('typed_debug', "and type(profile.get('debuginfo')) is int", '',
         'CargoSelectionTests.test_target_identity_and_profile'),
        ('directory_reserve', 'DIRECTORY_RESERVE = 120', 'DIRECTORY_RESERVE = 0',
         'BindingRegressionTests.test_directory_cleanup_reservation_and_expired_final_deadline'),
        ('scratch_filesystem', 'shutil.disk_usage(self.scratch_parent).free', 'shutil.disk_usage(self.repo).free',
         'BindingRegressionTests.test_free_delta_uses_canonical_scratch_filesystem'),
    ])
    killed = {}
    try:
        for name, old, new, expected in roster:
            assert source.count(old) == 1, name+'-unique-site'
            module = types.ModuleType('binding_mutant')
            module.__file__ = original.__file__
            sys.modules[module.__name__] = module
            exec(compile(source.replace(old, new), original.__file__, 'exec'), module.__dict__)
            binding = module
            started = time.monotonic()
            result = run_tests(expected)
            failures = sorted({'.'.join(case.id().split(' (', 1)[0].split('.')[-2:])
                               for case, _ in result.failures})
            assert time.monotonic()-started < 30, name+'-timeout-not-kill'
            assert not result.errors, name+'-setup-error-not-kill'
            assert expected in failures, name+'-named-assertion-not-killed'
            killed[name] = failures
    finally:
        binding = original
        sys.modules.pop('binding_mutant', None)
    restored = run_tests()
    assert not restored.errors and not restored.failures, 'mutation-restored-baseline'
    return dict(mutants=len(killed), named_assertion_kills=killed,
                baseline_tests=baseline.testsRun, restored_tests=restored.testsRun)


if __name__ == '__main__':
    if '--integration' in sys.argv:
        parser = argparse.ArgumentParser()
        parser.add_argument('--integration', action='store_true')
        parser.add_argument('--suite-seconds', type=float, default=7200)
        for name in ('repo', 'source-revision', 'registry', 'native', 'toolchain', 'scratch-parent'):
            parser.add_argument('--'+name, required=True)
        args = parser.parse_args()
        try:
            result = integration(binding.Inputs(Path(args.repo), args.source_revision, Path(args.registry),
                                                Path(args.native), Path(args.toolchain), Path(args.scratch_parent)),
                                 suite_seconds=args.suite_seconds)
            print(json.dumps(result, sort_keys=True))
        except ProducerFailure as error:
            print(json.dumps({'integration': 'failed', 'code': error.code, 'phase': error.phase}))
            sys.exit(1)
    elif '--mutation-proof' in sys.argv:
        print(json.dumps(mutation_proof(), sort_keys=True))
    elif '--lifecycle' in sys.argv:
        print(json.dumps(lifecycle(time.monotonic()+120), sort_keys=True))
    else:
        unittest.main()
