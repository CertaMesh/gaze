"""A misplaced build mock must fail before any native process starts."""

import os
from pathlib import Path
import shutil
import subprocess
import shlex
import sys
from types import SimpleNamespace

import pytest


def test_literal_and_absolute_cargo_builds_are_blocked(tmp_path):
    command = shutil.which('cargo')
    assert command is not None
    toolchains = Path(os.environ.get('RUSTUP_HOME', Path.home() / '.rustup')).glob('toolchains/*/bin/cargo')
    executables = ['cargo', command, str(Path.home() / '.cargo/bin/cargo')]
    executables += [str(path) for path in toolchains]
    for executable in executables:
        with pytest.raises(Exception) as refused:
            subprocess.run([executable, 'build'], cwd=tmp_path, check=True)
        assert isinstance(refused.value, AssertionError)
        assert 'native Cargo is disabled by pytest' in str(refused.value)
    assert not (tmp_path / 'target').exists()


def test_synthetic_cargo_fixture_is_allowed(tmp_path):
    cargo = tmp_path / 'cargo'
    cargo.write_text('#!/bin/sh\necho synthetic-build\n')
    cargo.chmod(0o700)
    env = {**os.environ, 'PATH': str(tmp_path) + os.pathsep + os.environ['PATH']}
    result = subprocess.run(['cargo', 'build'], env=env, check=True, text=True, capture_output=True)
    assert result.stdout.strip() == 'synthetic-build'


@pytest.mark.parametrize('command', ['build', 'test', 'check', 'run', 'some-build-alias'])
def test_native_cargo_allowlist_refuses_unknown_commands(tmp_path, command):
    with pytest.raises(Exception) as refused:
        subprocess.run(['cargo', command], cwd=tmp_path, check=True)
    assert isinstance(refused.value, AssertionError)
    assert 'native Cargo is disabled by pytest' in str(refused.value)
    assert not (tmp_path / 'target').exists()


def test_cargo_version_remains_available(tmp_path, monkeypatch):
    cargo, _ = synthetic_native_tools(tmp_path, monkeypatch)
    cargo.write_text('#!/bin/sh\necho cargo synthetic-version\n')
    result = subprocess.run([str(cargo), '--version'], check=True, text=True, capture_output=True)
    assert result.stdout.startswith('cargo ')


def synthetic_native_tools(root, monkeypatch):
    cargo = root / 'toolchains/synthetic/bin/cargo'
    cargo.parent.mkdir(parents=True)
    marker = root / 'native-started'
    cargo.write_text('#!/bin/sh\necho synthetic-native-invoked "$@"\n'
                     'echo started > ' + shlex.quote(str(marker)) + '\n')
    cargo.chmod(0o700)
    bindir = root / 'bin'
    bindir.mkdir()
    rustup = bindir / 'rustup'
    rustup.write_text('#!/bin/sh\nshift 3\nexec ' + shlex.quote(str(cargo)) + ' "$@"\n')
    rustup.chmod(0o700)
    monkeypatch.setenv('RUSTUP_HOME', str(root))
    monkeypatch.setenv('PATH', str(bindir) + os.pathsep + str(cargo.parent) + os.pathsep + os.environ['PATH'])
    return cargo, marker


def python_child(code, *, flags=()):
    bench = str(Path(__file__).resolve().parent)
    return subprocess.run([sys.executable, *flags, '-c',
                           'import sys;sys.path.insert(0,' + repr(bench) + ');' + code],
                          capture_output=True, text=True)


@pytest.mark.parametrize('depth', [1, 2])
def test_descendant_absolute_toolchain_cargo_is_blocked(tmp_path, monkeypatch, depth):
    cargo, marker = synthetic_native_tools(tmp_path, monkeypatch)
    code = 'import subprocess;subprocess.run(' + repr([str(cargo), 'build']) + ',check=True)'
    if depth == 2:
        code = 'import subprocess,sys;subprocess.run([sys.executable,"-c",' + repr(code) + '],check=True)'
    result = python_child(code)
    assert result.returncode != 0
    assert 'native Cargo is disabled by pytest' in result.stderr
    assert not marker.exists(), result.stdout


@pytest.mark.parametrize('producer', [
    'gaze_bench_score.build_validator_probe(root)',
    'run_no_opf_benchmark.build_validator_probe(root, release=True)',
    'dataiku_en_de_gaze_bench.build_binary(root, ())',
    'openpii_gaze_bench.build_binary(root, ())',
    'safety_net_bench_lib.clean_fixtures(root, [])',
    'subject_build.cargo_build_binary(root, ())',
])
def test_descendant_actual_producer_is_blocked(tmp_path, monkeypatch, producer):
    _, marker = synthetic_native_tools(tmp_path, monkeypatch)
    module = producer.split('.')[0]
    code = 'from pathlib import Path;import ' + module + ';root=Path(' + repr(str(tmp_path)) + ');' + producer
    with pytest.raises(AssertionError, match='native Cargo is disabled by pytest'):
        exec(code, {})
    result = python_child(code)
    assert result.returncode != 0
    assert 'native Cargo is disabled by pytest' in result.stderr
    assert not marker.exists(), result.stdout


@pytest.mark.parametrize('command', [['cargo', 'build'], ['rustup', 'run', 'synthetic', 'cargo', 'build']])
def test_path_launchers_block_children_without_python_startup(tmp_path, monkeypatch, command):
    import conftest
    _, marker = synthetic_native_tools(tmp_path, monkeypatch)
    config = SimpleNamespace()
    conftest.pytest_configure(config)
    try:
        result = python_child('import subprocess;subprocess.run(' + repr(command) + ',check=True)', flags=('-S',))
        assert result.returncode != 0
        assert 'native Cargo is disabled by pytest' in result.stderr
        assert not marker.exists(), result.stdout
    finally:
        conftest.pytest_unconfigure(config)


@pytest.mark.parametrize('command', [['cargo', 'metadata'], ['rustup', 'run', 'synthetic', 'cargo', 'metadata']])
def test_path_launchers_preserve_read_only_arguments(tmp_path, monkeypatch, command):
    import conftest
    _, marker = synthetic_native_tools(tmp_path, monkeypatch)
    config = SimpleNamespace()
    conftest.pytest_configure(config)
    try:
        result = python_child('import subprocess;subprocess.run(' + repr(command) + ',check=True)', flags=('-S',))
        assert result.returncode == 0, result.stderr
        assert 'synthetic-native-invoked metadata' in result.stdout
        assert marker.exists()
    finally:
        conftest.pytest_unconfigure(config)
