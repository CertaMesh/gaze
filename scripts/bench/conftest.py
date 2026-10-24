"""Keep model-free Python tests from accidentally starting native builds."""

import os
from pathlib import Path
import shlex
import shutil
import sys
import tempfile


def pytest_configure(config):
    cargo = shutil.which('cargo')
    directory = tempfile.TemporaryDirectory(prefix='gaze-pytest-no-cargo-')
    guard = Path(directory.name) / 'cargo'
    guard.write_text("#!/bin/sh\necho 'native Cargo is disabled by pytest' >&2\nexit 78\n")
    guard.chmod(0o700)
    blocked = {guard.resolve()}
    if cargo:
        blocked.add(Path(cargo).resolve())
    previous = {key: os.environ.get(key) for key in ('PATH', 'CARGO')}
    os.environ['PATH'] = directory.name + os.pathsep + previous['PATH']
    os.environ['CARGO'] = str(guard)
    state = {'active': True}

    def audit(event, values):
        if event != 'subprocess.Popen' or not state['active']:
            return
        executable, argv, _cwd, environment = values
        arguments = shlex.split(argv) if isinstance(argv, str) else list(argv)
        if Path(os.fsdecode(executable)).name in {'sh', 'bash', 'zsh'} and '-c' in arguments:
            arguments += shlex.split(arguments[arguments.index('-c') + 1])
        path = (environment or os.environ).get('PATH', os.defpath)
        for value in (executable, *arguments):
            value = os.fsdecode(value)
            if Path(value).name != 'cargo':
                continue
            resolved = shutil.which(value, path=path)
            if resolved and Path(resolved).resolve() in blocked:
                raise AssertionError('native Cargo is disabled by pytest; mock the producer builder')

    sys.addaudithook(audit)
    config._gaze_native_guard = directory, previous, state


def pytest_unconfigure(config):
    directory, previous, state = config._gaze_native_guard
    state['active'] = False
    for key, value in previous.items():
        if value is None:
            os.environ.pop(key, None)
        else:
            os.environ[key] = value
    directory.cleanup()
