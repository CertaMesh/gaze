"""A misplaced build mock must fail before any native process starts."""

import os
from pathlib import Path
import shutil
import subprocess

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


def test_cargo_version_remains_available():
    result = subprocess.run(['cargo', '--version'], check=True, text=True, capture_output=True)
    assert result.stdout.startswith('cargo ')
