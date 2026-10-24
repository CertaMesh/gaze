"""Accidental-build guard for pytest and Python descendants with inherited env.

This is not a sandbox: stripped environments, Python -I/-S, arbitrary native
launchers and renamed/copied tools are outside the guard. Synthetic Cargo
fixtures outside the registered proxy/toolchain locations remain usable.
"""

import json
import os
from pathlib import Path
import shlex
import shutil
import sys

ALLOWED = {'metadata', 'new', 'version', 'help', '--version', '--help'}
ENVIRONMENT = '_GAZE_PYTEST_NATIVE_GUARD'


def cargo_allowed(arguments):
    return bool(arguments) and arguments[0] in ALLOWED


def install_audit(blocked):
    blocked = {Path(path).resolve() for path in blocked}
    state = {'active': True}

    def audit(event, values):
        if not state['active']:
            return
        if event == 'subprocess.Popen':
            executable, argv, _cwd, environment = values
        elif event in {'os.exec', 'os.posix_spawn'}:
            executable, argv, environment = values
        elif event == 'os.system':
            argv = shlex.split(os.fsdecode(values[0]))
            executable, environment = argv[0] if argv else '', None
        else:
            return
        arguments = shlex.split(argv) if isinstance(argv, str) else list(argv)
        arguments = [os.fsdecode(value) for value in arguments]
        if Path(os.fsdecode(executable)).name in {'sh', 'bash', 'zsh'} and '-c' in arguments:
            arguments += shlex.split(arguments[arguments.index('-c') + 1])
        # Looking up a tool does not execute it.
        if Path(os.fsdecode(executable)).name == 'rustup' and arguments[1:2] == ['which']:
            return
        environment = os.environ if environment is None else environment
        path = environment.get('PATH', os.defpath)
        toolchains = Path(os.environ.get('RUSTUP_HOME', Path.home() / '.rustup')) / 'toolchains'
        candidates = [(executable, arguments[1:])]
        candidates += [(value, arguments[index + 1:]) for index, value in enumerate(arguments)]
        for value, remaining in candidates:
            value = os.fsdecode(value)
            if Path(value).name != 'cargo':
                continue
            resolved = shutil.which(value, path=path)
            if resolved and (Path(resolved).resolve() in blocked
                             or Path(resolved).resolve().is_relative_to(toolchains.resolve())):
                if not cargo_allowed(remaining):
                    raise AssertionError('native Cargo is disabled by pytest; mock the producer builder')

    sys.addaudithook(audit)
    return state


def activate_from_environment():
    value = os.environ.get(ENVIRONMENT)
    if value:
        install_audit(json.loads(value))


def launchers(directory, cargo, rustup):
    """PATH shims also cover shell children without a Python audit hook."""
    deny = "echo 'native Cargo is disabled by pytest' >&2; exit 78"
    allowed = '|'.join(sorted(ALLOWED))
    native = shlex.quote(cargo) if cargo else 'false'
    guard = directory / 'cargo'
    guard.write_text('#!/bin/sh\ncase "${1:-}" in\n' + allowed + ') exec ' + native +
                     ' "$@";;\n*) ' + deny + ';;\nesac\n')
    guard.chmod(0o700)
    native = shlex.quote(rustup) if rustup else 'false'
    launcher = directory / 'rustup'
    launcher.write_text('#!/bin/sh\ncheck_cargo() {\nif [ "${1:-}" = run ]; then\n'
                        '  for arg do\n    case "$arg" in cargo|*/cargo)\n'
                        '      shift\n      case "${1:-}" in\n        ' + allowed +
                        ') break;;\n        *) ' + deny + ';;\n      esac;;\n'
                        '      *) shift;;\n    esac\n  done\nfi\n}\ncheck_cargo "$@"\n' +
                        'exec ' + native + ' "$@"\n')
    launcher.chmod(0o700)
    return guard
