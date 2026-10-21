"""Audit inputs that require the full CI owners, without invoking Cargo.

The declared paths were seeded from the input audit, then extended by discovery.
After reviewing new gate reads, run `python3 scripts/ci/ci_inputs.py --update`.
Computed gate paths cannot be resolved by a literal scanner: reader fingerprints
make changes to that code require an explicit input audit as well.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
TABLE = ROOT / 'scripts/ci/full-inputs.json'
INCLUDE = re.compile(r'\binclude(?:_str|_bytes)?!\s*\(\s*')
STRING = re.compile(r'r(?P<hashes>\#*)"(?P<raw>.*?)"(?P=hashes)|"(?P<plain>(?:\\.|[^"\\])*)"', re.S)


def tracked_files(root):
    return subprocess.check_output(['git', 'ls-files', '-z'], cwd=root).decode().split('\0')[:-1]


def includes(source, text, root):
    for match in INCLUDE.finditer(text):
        literal = STRING.match(text, match.end())
        if literal is None:
            raise ValueError(f'{source}: computed include needs input-audit support')
        value = literal['raw'] if literal['raw'] is not None else json.loads(literal[0])
        yield (source.parent / value).resolve().relative_to(root.resolve()).as_posix()


def discover(root=ROOT):
    """Resolve includes, manifests, docs, gate fixtures and literal xtask paths.

    Gate directory walks over production Rust sources are covered by a declared
    pattern, rather than duplicating the entire source tree in the path list.
    """
    files = tracked_files(root)
    paths = set()
    readers = {}
    for name in files:
        source = root / name
        if not source.is_file():
            continue
        if (name.startswith(('docs/', 'crates/xtask/fixtures/', 'crates/xtask/snapshots/'))
                or (name.startswith('crates/') and name.endswith('README.md'))):
            paths.add(name)
        if name.endswith('.rs'):
            text = source.read_text()
            paths.update(includes(source, text, root))
            if name.startswith('crates/xtask/'):
                # Include constants and paths passed indirectly through helpers.
                for literal in STRING.finditer(text):
                    value = literal['raw'] if literal['raw'] is not None else literal['plain']
                    if '\n' in value or '\\' in value:
                        continue
                    for base in (root, source.parent, root / 'crates/xtask'):
                        candidate = (base / value).resolve()
                        if candidate.is_file() and candidate.is_relative_to(root.resolve()):
                            paths.add(candidate.relative_to(root.resolve()).as_posix())
                readers[name] = hashlib.sha256(source.read_bytes()).hexdigest()
        if name in {'scripts/ci/tutorial_gate.py', '.github/workflows/tutorials.yml'}:
            paths.add(name)
            readers[name] = hashlib.sha256(source.read_bytes()).hexdigest()
        if name.endswith('Cargo.toml'):
            paths.add(name)
            manifest = tomllib.loads(source.read_text())
            package = manifest.get('package', {})
            readme = package.get('readme', 'README.md')
            if isinstance(readme, str) and (source.parent / readme).is_file():
                paths.add((source.parent / readme).resolve().relative_to(root.resolve()).as_posix())
    # These gate families consume directory contents, not a fixed filename list.
    patterns = {
        'docs/*': 'tutorial pages and published documentation links',
        'crates/*/src/*.rs': 'xtask production-source walks for fixture and tenant checks',
        'crates/*/build.rs': 'Cargo build scripts',
        'crates/*/tests/ui/*': 'trybuild-fixture-hygiene directory walks',
        'crates/*/tests/fixtures/*': 'runtime test fixture inputs',
        'crates/*/testdata/*': 'runtime test data and coverage corpus inputs',
        'crates/xtask/*': 'xtask gate sources, fixtures and generated snapshots',
        'crates/gaze-recognizers/embedded/*': 'bundle-tokenization-drift directory walk',
        'tests/ui/*': 'trybuild-fixture-hygiene directory walk',
        'lint/*': 'Dylint and class-map gate inputs',
    }
    return dict(paths=sorted(paths), patterns=patterns, gate_readers=readers)


def audit(table, root=ROOT):
    actual = discover(root)
    missing = set(actual['paths']) - set(table['paths'])
    if missing:
        raise ValueError('FULL input table is missing paths: ' + ', '.join(sorted(missing)))
    if actual['patterns'] != table['patterns'] or actual['gate_readers'] != table['gate_readers']:
        raise ValueError('gate readers changed: audit computed reads and update full-inputs.json')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--update', action='store_true')
    args = parser.parse_args()
    table = json.loads(TABLE.read_text())
    if args.update:
        current = discover()
        current['paths'] = sorted(set(table['paths']) | set(current['paths']))
        TABLE.write_text(json.dumps(current, indent=2, sort_keys=True) + '\n')
    else:
        audit(table)
        print(f"FULL input audit passed: {len(table['paths'])} declared paths")


if __name__ == '__main__':
    main()
