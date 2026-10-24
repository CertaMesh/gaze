"""Keep model-free Python tests from accidentally starting native builds."""

import json
import os
from pathlib import Path
import shutil
import tempfile

import native_build_guard as guard


def pytest_configure(config):
    cargo, rustup = shutil.which('cargo'), shutil.which('rustup')
    directory = tempfile.TemporaryDirectory(prefix='gaze-pytest-no-cargo-')
    root = Path(directory.name)
    launcher = guard.launchers(root, cargo, rustup)
    blocked = {launcher.resolve(), Path.home() / '.cargo/bin/cargo'}
    if cargo:
        blocked.add(Path(cargo).resolve())
    rustup_home = Path(os.environ.get('RUSTUP_HOME', Path.home() / '.rustup'))
    blocked.update(path.resolve() for path in rustup_home.glob('toolchains/*/bin/cargo'))
    keys = ('PATH', 'CARGO', 'PYTHONPATH', guard.ENVIRONMENT)
    previous = {key: os.environ.get(key) for key in keys}
    os.environ['PATH'] = directory.name + os.pathsep + (previous['PATH'] or os.defpath)
    os.environ['CARGO'] = str(launcher)
    os.environ[guard.ENVIRONMENT] = json.dumps(sorted(str(path) for path in blocked))
    (root / '_gaze_native_guard.py').write_bytes(Path(guard.__file__).read_bytes())
    (root / 'sitecustomize.py').write_text(
        'from _gaze_native_guard import activate_from_environment\nactivate_from_environment()\n')
    os.environ['PYTHONPATH'] = directory.name + (os.pathsep + previous['PYTHONPATH'] if previous['PYTHONPATH'] else '')
    state = guard.install_audit(blocked)
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
