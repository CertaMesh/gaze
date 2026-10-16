#!/usr/bin/env python3
"""Reproduce synthetic stored hashes with OpenSSL and Python's hashlib."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'crates/gaze-recognizers/tests/fixtures/password_hash_producers.json'
PASSWORD = 'synthetic-fixture-only!'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--openssl', default='openssl')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    rows = []
    for kind in ('1', '5', '6'):
        for salt in ('salt-test', 'salt_test'):
            encoded = subprocess.check_output(
                [args.openssl, 'passwd', '-' + kind, '-salt', salt, '-stdin'],
                input=PASSWORD + '\n', text=True).strip()
            rows.append({'producer': 'openssl passwd -' + kind, 'salt': salt, 'encoded': encoded})
    for algorithm in ('sha256', 'sha1'):
        for salt in ('salt+test', 'salt@test', 'salt with space', 'sält:test'):
            digest = hashlib.pbkdf2_hmac(algorithm, PASSWORD.encode(), salt.encode(), 1000)
            encoded = f'pbkdf2_{algorithm}$1000${salt}$' + base64.b64encode(digest).decode()
            rows.append({'producer': 'hashlib.pbkdf2_hmac ' + algorithm, 'salt': salt, 'encoded': encoded})
    for salt in ('salt+test', 'salt@test'):
        digest = hashlib.scrypt(PASSWORD.encode(), salt=salt.encode(), n=1024, r=8, p=1, dklen=64)
        encoded = f'scrypt$1024${salt}$8$1$' + base64.b64encode(digest).decode()
        rows.append({'producer': 'hashlib.scrypt', 'salt': salt, 'encoded': encoded})
    result = json.dumps(rows, indent=2, ensure_ascii=False) + '\n'
    if args.check:
        if DEST.read_text() != result:
            raise SystemExit('Producer fixtures differ; regenerate with OpenSSL supporting passwd -1/-5/-6')
    else:
        DEST.write_text(result)
    print(f'{len(rows)} synthetic producer fixtures verified')


if __name__ == '__main__':
    main()
