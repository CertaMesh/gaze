#!/usr/bin/env python3
"""Replay a published enumeration with current scoring and pinned historical inputs.

Usage: replay_enumeration_proof.py SCRIPT INPUT_COMMIT INPUT_SHA256 BASE HEAD OUT
SCRIPT is iban_trailing_word_enumeration, family_action_policy_matrix, or
family_action_full_matrix. The historical IBAN generator is verified against
its committed source digest; the daemon adapter and scoring stay current.
Remaining arguments (for example --workers) pass to SCRIPT.
"""
import hashlib
import importlib
from pathlib import Path
import subprocess
import sys
import types

import iban_trailing_word_enumeration as enumeration

SCRIPTS = frozenset({'iban_trailing_word_enumeration', 'family_action_policy_matrix',
                     'family_action_full_matrix'})
INPUT_PATH = 'scripts/bench/iban_trailing_word_enumeration.py'


def main():
    script, revision, digest, *arguments = sys.argv[1:]
    if script not in SCRIPTS or len(arguments) < 3:
        raise ValueError('invalid replay arguments')
    repo = Path(__file__).resolve().parents[2]
    # Historical code is a trusted repository input, never an external payload.
    source = subprocess.check_output(['git', 'show', f'{revision}:{INPUT_PATH}'], cwd=repo)
    if hashlib.sha256(source).hexdigest() != digest:
        raise ValueError('historical input source digest mismatch')
    historical = types.ModuleType('pinned_iban_inputs')
    historical.__file__ = str(repo / INPUT_PATH)
    exec(compile(source, INPUT_PATH, 'exec'), historical.__dict__)
    enumeration.documents = historical.documents
    target = importlib.import_module(script)
    sys.argv = [script, *arguments]
    return target.main()


if __name__ == '__main__':
    raise SystemExit(main())
