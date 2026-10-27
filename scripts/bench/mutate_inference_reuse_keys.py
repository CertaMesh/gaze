#!/usr/bin/env python3
"""Remove each reuse-key input, require a runtime test failure, then restore the source."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time

SOURCE = Path('crates/gaze-recognizers/src/safety_net/nym/reuse.rs')
TEST = 'safety_net::nym::reuse::tests::every_execution_input_separates_reuse'
MUTATIONS = {
    'model': ('            model,', '            model: 0,'),
    'thresholds': ('.map(|(k, v)| (k, v.to_bits()))', '.map(|(k, _)| (k, 0))'),
    'view': ('            view,', '            view: View::Stable,'),
    'bytes': ('            bytes,', '            bytes: String::new(),'),
    'tensor_ids': ('            ids: ids.to_vec(),', '            ids: Vec::new(),'),
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    path = root / SOURCE
    original = path.read_text()
    committed = subprocess.check_output(['git', 'show', f'HEAD:{SOURCE}'], cwd=root, text=True)
    if original != committed:
        raise RuntimeError('commit the implementation before running mutation probes')
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    args.output.mkdir(parents=True, exist_ok=True)
    results = []
    try:
        for name, (before, after) in MUTATIONS.items():
            if original.count(before) != 1:
                raise RuntimeError(f'{name}: mutation no longer uniquely matches the constructor')
            while True:
                free = subprocess.check_output(['memory_pressure'], text=True).splitlines()[-1]
                if int(''.join(c for c in free if c.isdigit())) >= 30:
                    break
                print('waiting for 30% free RAM', flush=True)
                time.sleep(10)
            path.write_text(original.replace(before, after))
            command = ['anvil', 'run', '--', 'env', 'CARGO_BUILD_JOBS=2', 'nice', '-n', '10',
                       'cargo', 'test', '--locked', '--release', '-p', 'gaze-recognizers',
                       '--features', 'safety-net-nym,test-support', '--lib', TEST, '--', '--exact']
            result = subprocess.run(command, cwd=root, text=True, capture_output=True)
            log = result.stdout + result.stderr
            (args.output / f'{name}.log').write_text(log)
            killed = (result.returncode != 0 and 'changed execution input reused a window' in log
                      and '1 failed' in log)
            results.append({'component': name, 'exit_code': result.returncode, 'runtime_test_killed': killed})
            path.write_text(original)
            print(json.dumps(results[-1]), flush=True)
            if not killed:
                raise AssertionError(f'{name}: key removal did not cause the expected runtime failure')
    finally:
        path.write_text(original)
        (args.output / 'mutations.json').write_text(json.dumps({
            'source_head': head, 'source_sha256': hashlib.sha256(original.encode()).hexdigest(),
            'test': TEST, 'results': results}, indent=2) + '\n')


if __name__ == '__main__':
    main()
