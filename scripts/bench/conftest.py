"""Keep model-free Python tests from accidentally starting native builds."""

import os
from pathlib import Path
import shlex
import shutil
import sys
import tempfile


ALLOWED = {'metadata', 'new', 'version', 'help', '--version', '--help'}


def cargo_allowed(arguments):
    return bool(arguments) and arguments[0] in ALLOWED


def pytest_configure(config):
    cargo = shutil.which('cargo')
    directory = tempfile.TemporaryDirectory(prefix='gaze-pytest-no-cargo-')
    guard = Path(directory.name) / 'cargo'
    native = shlex.quote(cargo) if cargo else 'false'
    guard.write_text("#!/bin/sh\ncase \"${1:-}\" in\n"
                     "metadata|new|version|help|--version|--help) exec " + native + " \"$@\";;\n"
                     "*) echo 'native Cargo is disabled by pytest' >&2; exit 78;;\nesac\n")
    guard.chmod(0o700)
    blocked = {guard.resolve()}
    if cargo:
        blocked.add(Path(cargo).resolve())
    rustup_home = Path(os.environ.get('RUSTUP_HOME', Path.home() / '.rustup'))
    blocked.update(path.resolve() for path in rustup_home.glob('toolchains/*/bin/cargo'))
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
        candidates = [(executable, arguments[1:])]
        candidates += [(value, arguments[index + 1:]) for index, value in enumerate(arguments)]
        for value, remaining in candidates:
            value = os.fsdecode(value)
            if Path(value).name != 'cargo':
                continue
            resolved = shutil.which(value, path=path)
            if resolved and Path(resolved).resolve() in blocked:
                if not cargo_allowed(remaining):
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
