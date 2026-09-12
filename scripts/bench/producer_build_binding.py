"""Private trusted-parent build custody; never a public source attestation.

Tools, SDK, cache and non-detaching build scripts are trusted. The parent must
have exclusive child-wait ownership and exclude concurrent input writers.
"""
from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
import hashlib
import io
import json
import math
import os
from pathlib import Path, PurePosixPath
import selectors
import shutil
import signal
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib

from bench_subprocess import BenchSubprocess, ProducerFailure, TransportLimits, producer_boundary
import evidence_bridge

MIB = 1024 * 1024
CHUNK = 64 * 1024
PROCESS_RESERVE = 10
DIRECTORY_RESERVE = 120
SOURCE = 'crates/gaze-mcp-rmcp/examples/evidence_bridge.rs'


class OwnerState(Enum):
    NEW = 'new'
    RUNNING = 'running'
    WAIT_ONLY = 'wait_only'
    REAPED = 'reaped'


def require(ok, code='protocol'):
    if not ok:
        raise ProducerFailure(code)


def check_time(deadline):
    require(time.monotonic() < deadline, 'deadline')


def platform_preflight():
    require(sys.platform in ('darwin', 'linux') and sys.version_info >= (3, 13),
            'unsupported_platform')
    require(signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL, 'invalid_state')
    require(all(hasattr(os, key) for key in ('waitid', 'WNOWAIT', 'O_NOFOLLOW', 'O_NONBLOCK')),
            'unsupported_platform')


def metadata(command, *, cwd=None, env=None, deadline=None, cap=MIB):
    """Bounded trusted leaf-tool output. This is not the build-group owner."""
    end = min(deadline or float('inf'), time.monotonic() + 30)
    p = None
    try:
        p = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
        output = bytearray()
        count = 0
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
        if p is not None:
            try:
                if p.returncode is None:
                    p.kill()
                    p.wait(timeout=max(.001, min(3, end-time.monotonic())))
            finally:
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
        require(type(deadline) in (int, float) and math.isfinite(deadline)
                and type(seconds) in (int, float) and 0 < seconds <= 1800
                and type(cap) is int and 0 < cap <= 64*MIB, 'invalid_limits')
        self.deadline = deadline
        self.work_end = min(deadline - PROCESS_RESERVE, time.monotonic() + seconds)
        self.cap = cap
        self.process = None
        self.observed = None
        self.state = OwnerState.NEW
        self.signals = []
        self.cleanup_end = deadline

    @property
    def reaped(self):
        return self.state == OwnerState.REAPED

    def observe(self):
        require(self.state == OwnerState.RUNNING, 'invalid_state')
        if self.observed is None:
            self.observed = os.waitid(os.P_PID, self.process.pid,
                                      os.WEXITED | os.WNOHANG | os.WNOWAIT)
        return self.observed

    def signal_group(self, sig):
        require(self.process is not None and self.state == OwnerState.RUNNING, 'invalid_state')
        self.signals.append(sig)
        try:
            os.killpg(self.process.pid, sig)
        except ProcessLookupError:
            pass
        except PermissionError:
            # An unreaped zombie-only Darwin group can return EPERM.
            members = group_members(self.process.pid, self.cleanup_end)
            require(self.observe() is not None and self.process.pid in members
                    and all(s[0] == 'Z' for s in members.values()), 'cleanup')

    def settled(self, deadline=None):
        members = group_members(self.process.pid, deadline or self.cleanup_end)
        require(self.process.pid in members, 'cleanup')
        return self.observe() is not None and all(s[0] == 'Z' for s in members.values())

    def cleanup(self, failure):
        require(self.state == OwnerState.RUNNING, 'invalid_state')
        clean = False
        cleanup_error = False
        live_after_exit = False
        self.cleanup_end = min(self.deadline, time.monotonic()+7)
        try:
            clean = self.settled()
            if not clean:
                live_after_exit = not failure
                self.signal_group(signal.SIGTERM)
                end = min(self.cleanup_end-5, time.monotonic()+2)
                while time.monotonic() < end:
                    if self.settled(end):
                        clean = True
                        break
                    time.sleep(.02)
                if not clean:
                    self.signal_group(signal.SIGKILL)
                    end = min(self.cleanup_end-2, time.monotonic()+3)
                    while time.monotonic() < end:
                        if self.settled(end):
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
            self.state = OwnerState.WAIT_ONLY
            try:
                status = self.process.wait(timeout=max(.001, min(3, self.cleanup_end-time.monotonic())))
                self.state = OwnerState.REAPED
            finally:
                self.process.stdout.close()
                self.process.stderr.close()
        require(clean and not cleanup_error, 'cleanup')
        require(not live_after_exit, 'cleanup')
        require(self.observed is not None, 'cleanup')
        expected = (self.observed.si_status if self.observed.si_code == os.CLD_EXITED
                    else -self.observed.si_status)
        require(status == expected, 'cleanup')
        return status

    @producer_boundary
    def run(self, command, *, cwd=None, env=None, consume=None, sample=None):
        platform_preflight()
        require(self.process is None, 'invalid_state')
        check_time(self.work_end)
        failure = True
        try:
            self.process = subprocess.Popen(command, cwd=cwd, env=env,
                                            stdin=subprocess.DEVNULL,
                                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                            start_new_session=True, bufsize=0)
            self.state = OwnerState.RUNNING
            counts = {'stdout': 0, 'stderr': 0}
            sampled = 0
            with selectors.DefaultSelector() as selector:
                for name in counts:
                    stream = getattr(self.process, name)
                    os.set_blocking(stream.fileno(), False)
                    selector.register(stream, selectors.EVENT_READ, name)
                drain_end = None
                while True:
                    observed = self.observe()
                    if not selector.get_map() and observed is not None:
                        break
                    check_time(self.work_end)
                    if observed is not None:
                        if drain_end is None:
                            drain_end = min(self.work_end, time.monotonic()+2)
                        require(time.monotonic() < drain_end, 'cleanup')
                    if sample is not None and observed is None and time.monotonic()-sampled >= 1:
                        sample()
                        sampled = time.monotonic()
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
            if self.process is not None:
                # A returned process is owned even if RUNNING bookkeeping failed.
                if self.state == OwnerState.NEW:
                    self.state = OwnerState.RUNNING
                status = self.cleanup(failure)
        return status


def open_regular(path, deadline):
    """Never block on a FIFO before validating the opened file kind."""
    check_time(deadline)
    fd = None
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        require(stat.S_ISREG(os.fstat(fd).st_mode), 'io')
        return fd
    except BaseException as error:
        if fd is not None:
            os.close(fd)
        if isinstance(error, OSError):
            raise ProducerFailure('io') from None
        raise


def file_digest(path, deadline):
    fd = open_regular(path, deadline)
    try:
        observed, digest = digest_fd(fd, deadline)
        return observed[4], observed[2], digest.hex()
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


def verify_native_archive(path, host, deadline):
    """Check native object architecture without executing or extracting members."""
    machines = {'x86_64-unknown-linux-gnu': ('elf', 62),
                'aarch64-unknown-linux-gnu': ('elf', 183),
                'x86_64-apple-darwin': ('macho', 0x1000007),
                'aarch64-apple-darwin': ('macho', 0x100000c)}
    require(host in machines, 'unsupported_platform')
    kind, machine = machines[host]
    fd = open_regular(path, deadline)
    try:
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_size <= 2*1024**3, 'input_limit')
        require(os.read(fd, 8) == b'!<arch>\n', 'invalid_state')
        offset, members, objects = 8, 0, 0
        while offset < info.st_size:
            check_time(deadline)
            os.lseek(fd, offset, os.SEEK_SET)
            header = os.read(fd, 60)
            require(len(header) == 60 and header[58:] == b'`\n', 'invalid_state')
            name = header[:16].rstrip()
            raw_size = header[48:58].strip()
            require(raw_size.isdigit(), 'invalid_state')
            size = int(raw_size)
            require(offset+60+size <= info.st_size, 'invalid_state')
            payload_size = size
            if name.startswith(b'#1/'):
                require(name[3:].isdigit() and int(name[3:]) <= min(size, 4096), 'invalid_state')
                count = int(name[3:])
                name = os.read(fd, count).rstrip(b'\0')
                payload_size -= count
            if name not in (b'/', b'//', b'/SYM64/') and not name.startswith(b'__.SYMDEF'):
                require(payload_size >= 20, 'invalid_state')
                prefix = os.read(fd, 20)
                if kind == 'elf':
                    require(prefix[:6] == b'\x7fELF\x02\x01'
                            and struct.unpack('<H', prefix[18:20])[0] == machine, 'invalid_state')
                else:
                    require(prefix[:4] == b'\xcf\xfa\xed\xfe'
                            and struct.unpack('<I', prefix[4:8])[0] == machine, 'invalid_state')
                objects += 1
            offset += 60+size+(size & 1)
            members += 1
            require(members <= 100000, 'input_limit')
        require(offset == info.st_size and objects > 0 and identity(os.fstat(fd)) == identity(info),
                'invalid_state')
    finally:
        os.close(fd)


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
                + len(ancestors)*512 + 12288)
    archive = bytearray()
    def collect(chunk):
        require(len(archive)+len(chunk) <= bound, 'output_limit')
        archive.extend(chunk)
    owner = BuildOwner(deadline=end, seconds=100, cap=bound)
    require(owner.run(['/usr/bin/git', '-c', 'tar.umask=0022', 'archive', '--format=tar', tree],
                      cwd=repo, env=env, consume=collect) == 0, 'producer_exit')
    validate_tar_headers(archive)
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
    lock = tomllib.loads((destination/'Cargo.lock').read_text())
    require(not any(p.get('source', '').startswith('git+') for p in lock.get('package', [])))
    return inventory(destination, end), len(archive)


def validate_tar_headers(archive):
    # tarfile otherwise silently consumes GNU/PAX extension headers. This
    # snapshot format deliberately supports only ordinary Git USTAR entries.
    offset = 0
    while offset+512 <= len(archive):
        header = archive[offset:offset+512]
        if header == bytes(512):
            require(len(archive)-offset >= 1024 and not any(archive[offset:]))
            return
        require(header[156] in (0, ord('0'), ord('5')) and header[257:263] == b'ustar\0')
        raw = header[124:136].strip(b'\0 ')
        require(bool(raw) and all(byte in b'01234567' for byte in raw))
        size = int(raw, 8)
        offset += 512+((size+511)//512)*512
        require(offset <= len(archive))
    require(False)


def prepare_inputs(root, *, registry, native, toolchain, deadline):
    platform_preflight()
    require(shutil.disk_usage(root).free >= 20*1024**3, 'input_limit')
    home, cargo_home, tmp = (root/x for x in ('home', 'cargo-home', 'tmp'))
    for path in (home, cargo_home, tmp):
        path.mkdir(mode=0o700)
    minimal = {'HOME': str(home), 'CARGO_HOME': str(cargo_home),
               'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'LC_ALL': 'C'}
    native_inventory = inventory(native, min(deadline, time.monotonic()+30))
    require('libonnxruntime.a' in native_inventory, 'invalid_state')
    tools = {name: (toolchain/'bin'/name).resolve(strict=True)
             for name in ('cargo', 'rustc', 'rustdoc')}
    tool_inventory = {k: file_digest(v, deadline) for k, v in tools.items()}
    for name, path in tools.items():
        version = metadata([str(path), '--version'], env=minimal, deadline=deadline)
        require(version.startswith((name+' 1.96.0 ').encode()), 'invalid_state')
    host = metadata([str(tools['rustc']), '-vV'], env=minimal, deadline=deadline)
    hosts = [line[6:].decode('ascii') for line in host.splitlines() if line.startswith(b'host: ')]
    require(len(hosts) == 1)
    verify_native_archive(native/'libonnxruntime.a', hosts[0], min(deadline, time.monotonic()+30))
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
            with os.fdopen(open_regular(source/rel, cache_end), 'rb') as src, dest.open('xb') as out:
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


def closed_json(raw, *, cap=MIB):
    require(type(cap) is int and 0 < cap <= 4*MIB, 'invalid_limits')
    require(len(raw) <= cap, 'output_limit')
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
    try:
        value = json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid)
    except (ValueError, UnicodeError):
        raise ProducerFailure('protocol') from None
    require(type(value) is dict)
    return value


class CargoEvents:
    def __init__(self, package, source, target, features=('transport-stdio',)):
        self.package, self.source, self.target = package, source, target
        self.features = sorted(features)
        self.pending = bytearray()
        self.count = 0
        self.selected = None
        self.selected_record = None
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
                    and profile.get('opt_level') == '0'
                    and type(profile.get('debuginfo')) is int and profile.get('debuginfo') == 2
                    and profile.get('debug_assertions') is True
                    and profile.get('overflow_checks') is True)
            executable = event.get('executable')
            require(type(executable) is str)
            path = Path(executable)
            require(path.is_absolute() and path == path.resolve(strict=True)
                    and path.is_relative_to(self.target))
            require(stat.S_ISREG(path.lstat().st_mode))
            self.selected = path
            # Keep only the approved unit record; compiler diagnostics are discarded.
            self.selected_record = {key: event[key] for key in (
                'reason', 'package_id', 'target', 'profile', 'executable', 'fresh', 'features')}

    def finish(self, status):
        require(not self.pending and self.finished and self.selected is not None and status == 0)
        return self.selected


def identity(info):
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_mode


def digest_fd(fd, deadline):
    before = os.fstat(fd)
    require(stat.S_ISREG(before.st_mode), 'io')
    digest, count = hashlib.sha256(), 0
    os.lseek(fd, 0, os.SEEK_SET)
    while chunk := os.read(fd, CHUNK):
        check_time(deadline)
        count += len(chunk)
        require(count <= before.st_size, 'io')
        digest.update(chunk)
    require(count == before.st_size and identity(os.fstat(fd)) == identity(before), 'io')
    return identity(before), digest.digest()


class CapturedArtifact:
    """Retained fd is a comparator, never a claim of fd-based execution."""

    def __init__(self, path, deadline):
        require(path == path.resolve(strict=True), 'io')
        self.path, self.deadline = path, deadline
        self.fd = open_regular(path, deadline)
        try:
            self.baseline = digest_fd(self.fd, deadline)
        except BaseException:
            self.close()
            raise

    def compare(self):
        require(self.fd is not None and self.path == self.path.resolve(strict=True), 'io')
        require(digest_fd(self.fd, self.deadline) == self.baseline, 'io')
        other = open_regular(self.path, self.deadline)
        try:
            require(digest_fd(other, self.deadline) == self.baseline, 'io')
        finally:
            os.close(other)

    def close(self):
        if self.fd is not None:
            fd, self.fd = self.fd, None
            os.close(fd)


def remove_owned(path, deadline, *, keep=None):
    """Remove only this private tree, without following generated symlinks."""
    check_time(deadline)
    if keep is not None and path == keep:
        return
    if path.is_symlink() or not path.is_dir():
        path.unlink()
        return
    with os.scandir(path) as entries:
        for entry in entries:
            remove_owned(Path(entry.path), deadline, keep=keep)
    if keep is None or not keep.is_relative_to(path):
        path.rmdir()


@dataclass(frozen=True, repr=False)
class Inputs:
    repo: Path
    revision: str
    registry: Path
    native: Path
    toolchain: Path
    scratch_parent: Path


class BindingState(Enum):
    NEW = 'new'
    PREPARED = 'prepared'
    BUILT = 'built'
    CAPTURED = 'captured'
    CLOSED = 'closed'


class BindingSession:
    """One fresh build and its artifact stay in one parent through retirement."""

    def __init__(self, inputs, *, suite_deadline, variant='control'):
        require(variant in ('control', 'no_transport', 'alternate'), 'invalid_state')
        self.inputs, self.variant = inputs, variant
        self.started = time.monotonic()
        self.deadline = min(suite_deadline, self.started+2400)
        self.work_end = self.deadline-DIRECTORY_RESERVE-PROCESS_RESERVE
        self.state = BindingState.NEW
        self.root = None
        self.captured = None
        self.events = None
        self.owner = None
        self.peak_bytes = 0
        self.measurements = {}

    @producer_boundary
    def __enter__(self):
        require(self.state == BindingState.NEW, 'invalid_state')
        check_time(self.work_end)
        try:
            i = self.inputs
            # Parent aliases must not split Cargo's canonical identity checks.
            self.repo, self.registry, self.native, self.toolchain, parent = (
                p.resolve(strict=True) for p in
                (i.repo, i.registry, i.native, i.toolchain, i.scratch_parent))
            self.scratch_parent = parent
            self.free_before = shutil.disk_usage(parent).free
            require(self.free_before >= 20*1024**3, 'input_limit')
            self.root = Path(tempfile.mkdtemp(prefix='producer-binding-', dir=parent))
            (self.tools, self.host, self.env, self.native_before, self.tools_before,
             self.cache_before, cache_size) = prepare_inputs(
                self.root, registry=self.registry, native=self.native,
                toolchain=self.toolchain, deadline=self.work_end)
            self.source, self.target = self.root/'snapshot', self.root/'target'
            self.source_before, archive_size = snapshot(self.repo, i.revision, self.source, self.work_end)
            lock = tomllib.loads((self.source/'Cargo.lock').read_text())
            ort = [p for p in lock.get('package', []) if p.get('name') == 'ort-sys']
            require(len(ort) == 1 and ort[0].get('version') == '2.0.0-rc.12', 'invalid_state')
            self.snapshot_identity = 'git-tree'
            if self.variant == 'alternate':
                path = self.source/SOURCE
                data = path.read_bytes()
                old, new = b'prefix(PHONE, 8)', b'prefix(PHONE, 9)'
                require(data.count(old) == 1, 'invalid_state')
                path.write_bytes(data.replace(old, new))
                self.source_before = inventory(self.source, self.work_end)
                self.snapshot_identity = 'parent-mutation:partial-prefix-nine'
            self.inventory_identity = hashlib.sha256(json.dumps(
                self.source_before, sort_keys=True).encode()).digest()
            self.target.mkdir(mode=0o700)
            self.measurements.update(snapshot_bytes=sum(v[1] for v in self.source_before.values()),
                                     archive_bytes=archive_size, cache_bytes=cache_size)
            self.state = BindingState.PREPARED
            return self
        except BaseException:
            self.close()
            raise

    def check_inputs(self):
        check_time(self.work_end)
        require(inventory(self.source, self.work_end) == self.source_before, 'io')
        require(inventory(self.native, self.work_end) == self.native_before, 'io')
        require({k: file_digest(v, self.work_end) for k, v in self.tools.items()}
                == self.tools_before, 'io')
        for name, baseline in self.cache_before.items():
            require(inventory(self.registry/name, self.work_end) == baseline, 'io')
            private = {key: (stat.S_IFREG | 0o444, value[1], value[2])
                       for key, value in baseline.items()}
            require(inventory(Path(self.env['CARGO_HOME'])/'registry'/name, self.work_end)
                    == private, 'io')
        no_configs(self.source)
        for name in ('config', 'config.toml', 'credentials', 'credentials.toml'):
            require(not (Path(self.env['CARGO_HOME'])/name).exists(), 'invalid_state')

    def sample_target(self):
        size = 0
        for path in self.target.rglob('*'):
            check_time(self.work_end)
            try:
                if path.is_file():
                    size += path.stat().st_size
            except FileNotFoundError:
                pass
        self.peak_bytes = max(self.peak_bytes, size)
        return size

    @producer_boundary
    def build(self):
        require(self.state == BindingState.PREPARED, 'invalid_state')
        require(not any(self.target.iterdir()), 'invalid_state')
        self.check_inputs()
        planned = closed_json(metadata([str(self.tools['cargo']), 'metadata', '--format-version', '1',
                                       '--no-deps', '--locked', '--offline',
                                       '--manifest-path', str(self.source/'Cargo.toml')],
                                      cwd=self.source, env=self.env, deadline=self.work_end, cap=4*MIB),
                              cap=4*MIB)
        packages = [p for p in planned['packages'] if p['name'] == 'gaze-mcp-rmcp'
                    and p['manifest_path'] == str(self.source/'crates/gaze-mcp-rmcp/Cargo.toml')]
        require(len(packages) == 1)
        features = () if self.variant == 'no_transport' else ('transport-stdio',)
        self.events = CargoEvents(packages[0]['id'], self.source/SOURCE, self.target, features)
        self.owner = BuildOwner(deadline=self.work_end)
        command = [str(self.tools['cargo']), 'build', '--locked', '--offline',
                   '-p', 'gaze-mcp-rmcp', '--example', 'evidence_bridge', '--no-default-features',
                   '--target', self.host, '--target-dir', str(self.target), '--message-format=json',
                   '--manifest-path', str(self.source/'Cargo.toml')]
        if features:
            command += ['--features', ','.join(features)]
        started = time.monotonic()
        status = self.owner.run(command, cwd=self.source, env=self.env,
                                consume=self.events.feed, sample=self.sample_target)
        self.executable = self.events.finish(status)
        require(self.owner.reaped, 'cleanup')
        self.check_inputs()
        final_bytes = self.sample_target()
        self.measurements.update(build_seconds=round(time.monotonic()-started, 3),
                                 target_bytes=final_bytes, sampled_peak_target_bytes=self.peak_bytes,
                                 peak_sample_interval_seconds=1, build_events=self.events.count,
                                 leader_exit=status, group_signals=len(self.owner.signals))
        self.state = BindingState.BUILT
        return self.events.selected_record

    def validate_record(self, record):
        # Used before capture as well as by real foreign-artifact falsifiers.
        events = CargoEvents(self.events.package, self.source/SOURCE, self.target,
                             self.events.features)
        events.event(record)
        events.event({'reason': 'build-finished', 'success': True})
        return events.finish(0)

    @producer_boundary
    def capture(self):
        require(self.state == BindingState.BUILT, 'invalid_state')
        self.captured = self.capture_record(self.events.selected_record)
        self.state = BindingState.CAPTURED

    def capture_record(self, record):
        require(self.state in (BindingState.BUILT, BindingState.CAPTURED)
                and self.owner.reaped, 'invalid_state')
        self.check_inputs()
        require(self.validate_record(record) == self.executable)
        return CapturedArtifact(self.executable, self.work_end)

    @producer_boundary
    def run_bridge(self):
        require(self.state == BindingState.CAPTURED and self.variant != 'no_transport', 'invalid_state')
        self.check_inputs()
        self.captured.compare()
        # B1's direct-child cleanup gets its own reserved time inside this entry.
        remaining = self.work_end-time.monotonic()-5
        require(remaining > 0, 'deadline')
        limits = TransportLimits(invocation_seconds=remaining,
                                 handshake_seconds=min(120, remaining),
                                 exchange_seconds=min(300, remaining),
                                 finish_seconds=min(30, remaining))
        try:
            result = evidence_bridge.run(self.executable, _owner=BenchSubprocess(
                [str(self.executable)], cwd=self.source, env=self.env, limits=limits))
        finally:
            self.captured.compare()
            self.check_inputs()
        return result

    @producer_boundary
    def retire_build_products(self):
        require(self.state == BindingState.CAPTURED, 'invalid_state')
        self.captured.compare()
        remove_owned(self.target, self.work_end, keep=self.executable)
        self.captured.compare()

    def close(self):
        if self.state == BindingState.CLOSED:
            return
        try:
            if self.captured is not None:
                self.captured.close()
        finally:
            if self.root is not None:
                remove_owned(self.root, self.deadline)
            self.state = BindingState.CLOSED
        self.measurements['elapsed_seconds'] = round(time.monotonic()-self.started, 3)
        if self.root is not None:
            self.measurements['free_delta_bytes'] = self.free_before-shutil.disk_usage(self.scratch_parent).free

    @producer_boundary
    def __exit__(self, kind, error, traceback):
        try:
            if kind is None:
                require(self.state == BindingState.CAPTURED, 'invalid_state')
                self.check_inputs()
                self.captured.compare()
        finally:
            self.close()
        return False


@producer_boundary
def run_binding(inputs, *, suite_deadline):
    with BindingSession(inputs, suite_deadline=suite_deadline) as session:
        session.build()
        session.capture()
        result = session.run_bridge()
    return {'binding_verified': True, 'numeric_verified': result.numeric_verified,
            'measurements': session.measurements}
