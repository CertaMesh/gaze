"""Private trusted-parent build custody; never a public source attestation.

Tools, SDK, cache and non-detaching build scripts are trusted. The parent must
have exclusive child-wait ownership and exclude concurrent input writers.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tarfile
import time
import tomllib

from bench_subprocess import ProducerFailure, producer_boundary

MIB = 1024 * 1024
CHUNK = 64 * 1024
SOURCE = 'crates/gaze-mcp-rmcp/examples/evidence_bridge.rs'


def require(ok, code='protocol'):
    if not ok:
        raise ProducerFailure(code)


def check_time(deadline):
    require(time.monotonic() < deadline, 'deadline')


def platform_preflight():
    require(sys.platform in ('darwin', 'linux') and sys.version_info >= (3, 13),
            'unsupported_platform')
    require(signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL, 'invalid_state')
    require(all(hasattr(os, key) for key in ('waitid', 'WNOWAIT', 'O_NOFOLLOW')),
            'unsupported_platform')


def metadata(command, *, cwd=None, env=None, deadline=None, cap=MIB):
    """Bounded trusted leaf-tool output. This is not the build-group owner."""
    end = min(deadline or float('inf'), time.monotonic() + 30)
    p = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
    output = bytearray()
    count = 0
    try:
        with selectors.DefaultSelector() as selector:
            for stream in (p.stdout, p.stderr):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ)
            while selector.get_map():
                check_time(end)
                for key, _ in selector.select(min(.05, max(0, end-time.monotonic()))):
                    chunk = os.read(key.fd, CHUNK)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    count += len(chunk)
                    require(count <= cap, 'output_limit')
                    if key.fileobj is p.stdout:
                        output.extend(chunk)
        require(p.wait(timeout=max(.001, end-time.monotonic())) == 0, 'producer_exit')
        return bytes(output)
    finally:
        if p.returncode is None:
            p.kill()
            p.wait(timeout=3)
        p.stdout.close()
        p.stderr.close()


def group_members(pgid, deadline):
    # PID/PGID/state only: no command lines, environment or diagnostic text.
    data = metadata(['/bin/ps', '-axo', 'pid=,pgid=,stat='],
                    env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}, deadline=deadline)
    require(data.endswith(b'\n'), 'cleanup')
    seen, members = set(), {}
    for line in data.splitlines():
        fields = line.split()
        require(len(fields) == 3 and fields[0].isdigit() and fields[1].isdigit(), 'cleanup')
        pid, group = int(fields[0]), int(fields[1])
        state = fields[2].decode('ascii')
        require(pid > 0 and pid not in seen and state and state[0] in 'RSDTtZXIWU', 'cleanup')
        seen.add(pid)
        if group == pgid:
            members[pid] = state
    return members


class BuildOwner:
    """Keep the unreaped session leader pinned until the last signal decision."""

    def __init__(self, *, deadline, seconds=1800, cap=64*MIB):
        self.deadline = deadline
        self.work_end = min(deadline - 10, time.monotonic() + seconds)
        self.cap = cap
        self.process = None
        self.observed = None
        self.reaped = False
        self.signals = []

    def observe(self):
        require(not self.reaped, 'invalid_state')
        if self.observed is None:
            self.observed = os.waitid(os.P_PID, self.process.pid,
                                      os.WEXITED | os.WNOHANG | os.WNOWAIT)
        return self.observed

    def signal_group(self, sig):
        require(self.process is not None and not self.reaped, 'invalid_state')
        self.signals.append(sig)
        try:
            os.killpg(self.process.pid, sig)
        except ProcessLookupError:
            pass
        except PermissionError:
            # An unreaped zombie-only Darwin group can return EPERM.
            members = group_members(self.process.pid, self.deadline)
            require(self.observe() is not None and self.process.pid in members
                    and all(s[0] == 'Z' for s in members.values()), 'cleanup')

    def settled(self):
        members = group_members(self.process.pid, self.deadline)
        require(self.process.pid in members, 'cleanup')
        return self.observe() is not None and all(s[0] == 'Z' for s in members.values())

    def cleanup(self, failure):
        require(not self.reaped, 'invalid_state')
        clean = False
        cleanup_error = False
        try:
            clean = self.settled()
            if not clean:
                failure = True
                self.signal_group(signal.SIGTERM)
                end = min(self.deadline-5, time.monotonic()+2)
                while time.monotonic() < end:
                    if self.settled():
                        clean = True
                        break
                    time.sleep(.02)
                if not clean:
                    self.signal_group(signal.SIGKILL)
                    end = min(self.deadline-2, time.monotonic()+3)
                    while time.monotonic() < end:
                        if self.settled():
                            clean = True
                            break
                        time.sleep(.02)
        except BaseException:
            cleanup_error = True
            # Still pinned. Make the final owned-group signal decision now.
            try:
                self.signal_group(signal.SIGKILL)
            except BaseException:
                pass
        finally:
            # No path beyond here may signal the group, even after wait failure.
            self.reaped = True
            try:
                status = self.process.wait(timeout=max(.001, min(3, self.deadline-time.monotonic())))
            finally:
                self.process.stdout.close()
                self.process.stderr.close()
        require(clean and not cleanup_error, 'cleanup')
        require(not failure, 'producer_exit')
        require(self.observed is not None, 'cleanup')
        expected = (self.observed.si_status if self.observed.si_code == os.CLD_EXITED
                    else -self.observed.si_status)
        require(status == expected, 'cleanup')
        return status

    @producer_boundary
    def run(self, command, *, cwd=None, env=None, consume=None):
        platform_preflight()
        require(self.process is None, 'invalid_state')
        check_time(self.work_end)
        self.process = subprocess.Popen(command, cwd=cwd, env=env,
                                        stdin=subprocess.DEVNULL,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        start_new_session=True, bufsize=0)
        failure = True
        try:
            counts = {'stdout': 0, 'stderr': 0}
            with selectors.DefaultSelector() as selector:
                for name in counts:
                    stream = getattr(self.process, name)
                    os.set_blocking(stream.fileno(), False)
                    selector.register(stream, selectors.EVENT_READ, name)
                while selector.get_map() or self.observe() is None:
                    check_time(self.work_end)
                    for key, _ in selector.select(.02):
                        chunk = os.read(key.fd, CHUNK)
                        if not chunk:
                            selector.unregister(key.fileobj)
                            continue
                        counts[key.data] += len(chunk)
                        require(counts[key.data] <= self.cap, 'output_limit')
                        if key.data == 'stdout' and consume is not None:
                            consume(chunk)
            failure = False
        finally:
            status = self.cleanup(failure)
        return status


def file_digest(path, deadline):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        before = os.fstat(fd)
        require(stat.S_ISREG(before.st_mode), 'io')
        digest = hashlib.sha256()
        while chunk := os.read(fd, CHUNK):
            check_time(deadline)
            digest.update(chunk)
        after = os.fstat(fd)
        require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
                == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns), 'io')
        return (before.st_mode, before.st_size, digest.hexdigest())
    finally:
        os.close(fd)


def inventory(root, deadline):
    entries = {}
    for directory, dirs, files in os.walk(root, followlinks=False):
        check_time(deadline)
        for name in dirs:
            require(not (Path(directory)/name).is_symlink(), 'io')
        for name in files:
            path = Path(directory)/name
            entries[path.relative_to(root).as_posix()] = file_digest(path, deadline)
    return entries


def no_configs(root):
    for directory in (root, *root.parents):
        for name in ('config', 'config.toml'):
            require(not (directory/'.cargo'/name).exists(), 'invalid_state')


def snapshot(repo, revision, destination, deadline):
    """Bound the archive before allocation; extract only exact Git blob entries."""
    end = min(deadline, time.monotonic()+120)
    require(len(revision) == 40 and all(c in '0123456789abcdef' for c in revision))
    env = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'GIT_CONFIG_NOSYSTEM': '1',
           'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_NO_REPLACE_OBJECTS': '1'}
    tree = metadata(['/usr/bin/git', 'rev-parse', revision+'^{tree}'],
                    cwd=repo, env=env, deadline=end).strip().decode('ascii')
    rows = metadata(['/usr/bin/git', 'ls-tree', '-rlz', tree],
                    cwd=repo, env=env, deadline=end, cap=4*MIB)
    require(rows.endswith(b'\0'))
    expected, ancestors = {}, set()
    for row in rows[:-1].split(b'\0'):
        header, raw_path = row.split(b'\t', 1)
        mode, kind, oid, size = header.split()
        path = raw_path.decode('utf-8')
        parts = PurePosixPath(path)
        require(kind == b'blob' and mode in (b'100644', b'100755')
                and not parts.is_absolute() and '..' not in parts.parts
                and parts.as_posix() == path and path not in expected)
        expected[path] = (int(mode, 8) & 0o777, oid.decode('ascii'), int(size))
        ancestors.update(str(p) for p in parts.parents if str(p) != '.')
    bound = min(64*MIB, sum(((v[2]+511)//512+1)*512 for v in expected.values())
                + len(ancestors)*512 + 10240)
    archive = bytearray()
    def collect(chunk):
        require(len(archive)+len(chunk) <= bound, 'output_limit')
        archive.extend(chunk)
    owner = BuildOwner(deadline=end, seconds=100, cap=bound)
    require(owner.run(['/usr/bin/git', '-c', 'tar.umask=0022', 'archive', '--format=tar', tree],
                      cwd=repo, env=env, consume=collect) == 0, 'producer_exit')
    destination.mkdir(mode=0o700)
    seen, seen_dirs = set(), set()
    with tarfile.open(fileobj=io.BytesIO(archive), mode='r:') as tar:
        require(not tar.pax_headers)
        for entry in tar:
            check_time(end)
            name = entry.name
            require(not entry.pax_headers and not entry.linkname)
            if entry.isdir():
                require(name in ancestors and name not in seen_dirs and entry.size == 0)
                seen_dirs.add(name)
                (destination/name).mkdir(mode=0o700, parents=True, exist_ok=True)
                continue
            require(entry.isreg() and name in expected and name not in seen)
            mode, oid, size = expected[name]
            require(entry.mode == mode and entry.size == size)
            path = destination/name
            path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            digest = hashlib.sha1(f'blob {size}\0'.encode('ascii'))
            source = tar.extractfile(entry)
            with path.open('xb') as out:
                remaining = size
                while remaining:
                    check_time(end)
                    data = source.read(min(CHUNK, remaining))
                    require(bool(data))
                    remaining -= len(data)
                    digest.update(data)
                    out.write(data)
            require(digest.hexdigest() == oid)
            path.chmod(mode)
            seen.add(name)
    require(seen == set(expected) and seen_dirs == ancestors)
    no_configs(destination)
    for path in destination.rglob('Cargo.toml'):
        check_time(end)
        value = tomllib.loads(path.read_text())
        def visit(item):
            if isinstance(item, dict):
                if 'path' in item and isinstance(item['path'], str):
                    require((path.parent/item['path']).resolve().is_relative_to(destination.resolve()))
                for child in item.values():
                    visit(child)
            elif isinstance(item, list):
                for child in item:
                    visit(child)
        visit(value)
    require('git+' not in (destination/'Cargo.lock').read_text())
    return inventory(destination, end), len(archive)


def prepare_inputs(root, *, registry, native, toolchain, deadline):
    platform_preflight()
    require(shutil.disk_usage(root).free >= 20*1024**3, 'input_limit')
    native_inventory = inventory(native, min(deadline, time.monotonic()+30))
    require('libonnxruntime.a' in native_inventory, 'invalid_state')
    tools = {name: (toolchain/'bin'/name).resolve(strict=True)
             for name in ('cargo', 'rustc', 'rustdoc')}
    tool_inventory = {k: file_digest(v, deadline) for k, v in tools.items()}
    for name, path in tools.items():
        version = metadata([str(path), '--version'], deadline=deadline)
        require(version.startswith((name+' 1.96.0 ').encode()), 'invalid_state')
    host = metadata([str(tools['rustc']), '-vV'], deadline=deadline)
    hosts = [line[6:].decode('ascii') for line in host.splitlines() if line.startswith(b'host: ')]
    require(len(hosts) == 1)
    home, cargo_home, tmp = (root/x for x in ('home', 'cargo-home', 'tmp'))
    for path in (home, cargo_home, tmp):
        path.mkdir(mode=0o700)
    # Private copies protect the approved cache from Cargo bookkeeping writes.
    # registry/src is deliberately absent and is extracted by this Cargo alone.
    cache_end = min(deadline, time.monotonic()+120)
    cache_size = 0
    cache_inventory = {}
    for name in ('index', 'cache'):
        source = registry/name
        require(source.is_dir() and not source.is_symlink(), 'invalid_state')
        before = inventory(source, cache_end)
        cache_inventory[name] = before
        for rel, (mode, size, digest) in before.items():
            check_time(cache_end)
            dest = cargo_home/'registry'/name/rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            with (source/rel).open('rb') as src, dest.open('xb') as out:
                while data := src.read(CHUNK):
                    check_time(cache_end)
                    out.write(data)
            dest.chmod(0o444)
            require(file_digest(dest, cache_end)[1:] == (size, digest), 'io')
            cache_size += size
    env = {'HOME': str(home), 'CARGO_HOME': str(cargo_home), 'TMPDIR': str(tmp),
           'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'LC_ALL': 'C',
           'RUSTC': str(tools['rustc']), 'RUSTDOC': str(tools['rustdoc']),
           'CARGO_BUILD_JOBS': '2', 'CARGO_NET_OFFLINE': 'true',
           'ORT_SKIP_DOWNLOAD': 'true', 'ORT_LIB_LOCATION': str(native)}
    return tools, hosts[0], env, native_inventory, tool_inventory, cache_inventory, cache_size


def closed_json(raw):
    require(len(raw) <= MIB, 'output_limit')
    depth, quoted, escaped = 0, False, False
    for byte in raw:
        if quoted:
            if escaped:
                escaped = False
            elif byte == 92:
                escaped = True
            elif byte == 34:
                quoted = False
        elif byte == 34:
            quoted = True
        elif byte in (123, 91):
            depth += 1
            require(depth <= 64, 'output_limit')
        elif byte in (125, 93):
            depth -= 1
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result)
            result[key] = value
        return result
    def invalid(_):
        require(False)
    value = json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid)
    require(type(value) is dict)
    return value


class CargoEvents:
    def __init__(self, package, source, target, features=('transport-stdio',)):
        self.package, self.source, self.target = package, source, target
        self.features = sorted(features)
        self.pending = bytearray()
        self.count = 0
        self.selected = None
        self.finished = False

    def feed(self, chunk):
        self.pending.extend(chunk)
        while b'\n' in self.pending:
            raw, _, tail = self.pending.partition(b'\n')
            self.pending = bytearray(tail)
            self.event(closed_json(raw))
        require(len(self.pending) <= MIB, 'output_limit')

    def event(self, event):
        self.count += 1
        require(self.count <= 100000 and not self.finished)
        reason = event.get('reason')
        require(reason in ('compiler-artifact', 'compiler-message', 'build-script-executed', 'build-finished'))
        if reason == 'build-finished':
            require(event.get('success') is True)
            self.finished = True
        elif reason == 'compiler-artifact':
            target = event.get('target')
            require(type(target) is dict)
            if event.get('package_id') != self.package or target.get('name') != 'evidence_bridge':
                return
            require(self.selected is None)
            require(target.get('kind') == ['example'] and target.get('crate_types') == ['bin']
                    and target.get('src_path') == str(self.source))
            require(event.get('features') == self.features and event.get('fresh') is False)
            profile = event.get('profile')
            require(type(profile) is dict and profile.get('test') is False
                    and profile.get('opt_level') == '0' and profile.get('debuginfo') == 2
                    and profile.get('debug_assertions') is True
                    and profile.get('overflow_checks') is True)
            executable = event.get('executable')
            require(type(executable) is str)
            path = Path(executable)
            require(path.is_absolute() and path == path.resolve(strict=True)
                    and path.is_relative_to(self.target))
            require(stat.S_ISREG(path.lstat().st_mode))
            self.selected = path

    def finish(self, status):
        require(not self.pending and self.finished and self.selected is not None and status == 0)
        return self.selected


@producer_boundary
def step0(root, *, repo, revision, registry, native, toolchain):
    # Canonicalize parent-approved roots before Cargo reports canonical paths.
    # On Darwin /tmp is a symlink; mixing its two spellings rejects all artifacts.
    root, repo, registry, native, toolchain = (
        p.resolve(strict=True) for p in (root, repo, registry, native, toolchain))
    started = time.monotonic()
    deadline = started + 2400
    free_before = shutil.disk_usage(root).free
    tools, host, env, native_before, tools_before, cache_before, cache_size = prepare_inputs(
        root, registry=registry, native=native, toolchain=toolchain, deadline=deadline)
    source = root/'snapshot'
    before, archive_size = snapshot(repo, revision, source, deadline)
    target = root/'target'
    target.mkdir(mode=0o700)
    require(not any(target.iterdir()), 'invalid_state')
    planned = closed_json(metadata([str(tools['cargo']), 'metadata', '--format-version', '1',
                                   '--no-deps', '--locked', '--offline',
                                   '--manifest-path', str(source/'Cargo.toml')],
                                  cwd=source, env=env, deadline=deadline))
    packages = [p for p in planned['packages'] if p['name'] == 'gaze-mcp-rmcp'
                and p['manifest_path'] == str(source/'crates/gaze-mcp-rmcp/Cargo.toml')]
    require(len(packages) == 1)
    events = CargoEvents(packages[0]['id'], source/SOURCE, target)
    owner = BuildOwner(deadline=deadline)
    build_start = time.monotonic()
    status = owner.run([str(tools['cargo']), 'build', '--locked', '--offline',
                        '-p', 'gaze-mcp-rmcp', '--example', 'evidence_bridge',
                        '--no-default-features', '--features', 'transport-stdio',
                        '--target', host, '--target-dir', str(target),
                        '--message-format=json', '--manifest-path', str(source/'Cargo.toml')],
                       cwd=source, env=env, consume=events.feed)
    executable = events.finish(status)
    require(inventory(source, deadline) == before, 'io')
    require(inventory(native, deadline) == native_before, 'io')
    require({k: file_digest(v, deadline) for k, v in tools.items()} == tools_before, 'io')
    for name, baseline in cache_before.items():
        require(inventory(registry/name, deadline) == baseline, 'io')
    no_configs(source)
    # This aggregate is a calibration, not a receipt or a dependency-wide claim.
    return dict(build_seconds=round(time.monotonic()-build_start, 3),
                elapsed_seconds=round(time.monotonic()-started, 3),
                target_bytes=sum(p.stat().st_size for p in target.rglob('*') if p.is_file()),
                snapshot_bytes=sum(v[1] for v in before.values()), archive_bytes=archive_size,
                cache_bytes=cache_size, free_delta_bytes=free_before-shutil.disk_usage(root).free,
                selected=True, build_events=events.count, leader_exit=status,
                group_signals=len(owner.signals))
