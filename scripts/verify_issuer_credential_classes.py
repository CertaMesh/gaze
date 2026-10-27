#!/usr/bin/env python3
"""Probe generated credential classes using an existing binary and external pack.

No builds, model inference or disk scan. Fixture values are synthetic. Output
contains case names and counts only; every successful call must restore exactly.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import issuer_credential_cases as cells


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/gaze')
    parser.add_argument('--pack', type=Path, default=ROOT / 'crates/gaze-recognizers/embedded/secrets.toml')
    parser.add_argument('--kind', choices=['identifiers', 'pem', 'issuers', 'residuals', 'all'], default='all')
    args = parser.parse_args()
    env = {**os.environ, 'GAZE_DISABLE_OS_KEYSTORE': '1'}

    def check(name, cases):
        separator = '\n!CASE!\n'
        raw = separator.join(text for _, text, _ in cases)
        result = subprocess.run([str(args.binary.resolve()), 'clean', '--rulepack-bundled', 'none',
                                 '--rulepack-path', str(args.pack.resolve()), '--locale', 'global'],
                                input=raw, text=True, capture_output=True, check=True, env=env)
        data = json.loads(result.stdout)
        observed = data['clean_text'].split(separator)
        if len(observed) != len(cases):
            raise AssertionError(f'{name}: case separators changed')
        tokens = {entry['raw']: entry['token'] for entry in data['entries']}
        failures = []
        for (uid, text, values), actual in zip(cases, observed):
            expected = text
            for value in values:
                if value not in tokens:
                    failures.append(uid + ': missing complete value')
                    break
                expected = expected.replace(value, tokens[value])
            else:
                if actual != expected:
                    failures.append(uid + ': unexpected changed bytes')
        restored = subprocess.run([str(args.binary.resolve()), 'restore'], input=json.dumps(
            {'session_blob': data['session_blob'], 'text': data['clean_text']}),
            text=True, capture_output=True, check=True, env=env)
        assert json.loads(restored.stdout)['text'] == raw, 'exact restore failed'
        print(f'{name}: {len(cases)} cases; {len(failures)} failures; exact restore passed', flush=True)
        for failure in failures[:12]:
            print(f'  {failure}', flush=True)
        return len(failures)

    failures = 0
    if args.kind in ('identifiers', 'all'):
        cases = [(f'{prefix}/{boundary!r}/{style}/{length}', value, ())
                 for prefix, boundary, style, length, value in cells.identifier_cases('test')
                 if not (style.endswith('_digit') and prefix in cells.BASE64URL_PREFIXES)]
        failures += check('identifiers', cases)
    if args.kind in ('pem', 'all'):
        cases = []
        for label in ['PRIVATE KEY', 'RSA PRIVATE KEY', 'EC PRIVATE KEY', 'DSA PRIVATE KEY',
                      'OPENSSH PRIVATE KEY', 'ENCRYPTED PRIVATE KEY', 'PGP PRIVATE KEY BLOCK']:
            for framing, br in cells.framing_cases():
                value = f'-----BEGIN {label}-----{br}bm90LWEta2V5c3ludGhldGlj{br}-----END {label}-----'
                cases.append((label + '/' + framing, value, (value,)))
        failures += check('pem', cases)
    if args.kind in ('issuers', 'all'):
        cases = []
        for prefix in cells.ISSUER_PREFIXES:
            # Values are shape witnesses, never usable credentials. Mixed digits
            # make the new evidence criterion independent of prefix markers.
            value = cells.issuer_value(prefix, lambda n: ('A1b2C3d4E5f6G7h8' * (n // 16 + 1))[:n])
            for boundary in cells.IDENTIFIER_BOUNDARIES:
                cases.append((prefix + '/' + repr(boundary), boundary + value, (value,)))
            for framing, br in cells.framing_cases():
                cases.append((prefix + '/' + framing, br + value + br, (value,)))
        for prefix in cells.BASE64URL_PREFIXES:
            for size in (33, 128):
                value = prefix + ('A1_-' * ((size + 3) // 4))[:size]
                for framing, br in cells.framing_cases():
                    cases.append((prefix + '/separator_dense/' + str(size) + '/' + framing,
                                  br + value + br, (value,)))
        jwt = 'ghs_123456_eyJ' + 'A_-1' * 16 + '.' + 'B_-1' * 16 + '.' + 'C_-1' * 16
        for framing, br in cells.framing_cases():
            cases.append(('ghs_JWT/separator_dense/' + framing, br + jwt + br, (jwt,)))
        for prefix in cells.ISSUER_PREFIXES:
            value = prefix + 'A' * 128
            for surface, text in [('cue', 'api key: ' + value),
                                  ('field', '{"api_key":"' + value + '"}')]:
                cases.append((prefix + '/independent_' + surface, text, (value,)))
        failures += check('issuers', cases)
    if args.kind in ('residuals', 'all'):
        cases = []
        for prefix in cells.ISSUER_PREFIXES:
            value = prefix + 'verifyHeaderSettingServiceRefreshOption' * 3 + '1'
            cases.append((prefix + '/camel_digit', value, (value,)))
        for prefix, boundary, style, length, value in cells.identifier_cases('test'):
            if style.endswith('_digit') and prefix in cells.BASE64URL_PREFIXES:
                cases.append((prefix + '/' + style + '/' + str(length), value,
                              (value[len(boundary):],)))
        for placeholder in ['YourPrivateKeyGoesHere', 'Base64EncodedKeyMaterial']:
            value = f'-----BEGIN PRIVATE KEY-----\n{placeholder}\n-----END PRIVATE KEY-----'
            cases.append(('pem/letter_placeholder', value, (value,)))
        failures += check('documented residuals', cases)
    if failures:
        raise SystemExit(1)
    print('CLASS PROBE DONE: all selected classes passed')


if __name__ == '__main__':
    main()
