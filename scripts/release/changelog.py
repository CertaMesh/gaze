#!/usr/bin/env python3
"""Check and assemble per-PR changelog fragments in changelog.d/.

Each PR adds `changelog.d/<name>.<section>.md` instead of editing CHANGELOG.md,
so parallel PRs never conflict on the changelog. A fragment holds one or more
Markdown list items. At release time, `assemble` moves every fragment into a
new version section of CHANGELOG.md and deletes the fragments.

  python3 scripts/release/changelog.py check
  python3 scripts/release/changelog.py check --none-pending
  python3 scripts/release/changelog.py assemble 1.0.0 --date 2026-12-01
"""
import argparse
import datetime
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
FRAGMENTS = 'changelog.d'
# Keep a Changelog order; `benchmark` keeps the project's evidence section.
SECTIONS = {
    'added': 'Added',
    'changed': 'Changed',
    'deprecated': 'Deprecated',
    'removed': 'Removed',
    'fixed': 'Fixed',
    'security': 'Security',
    'benchmark': 'Benchmark',
}
NAME = re.compile(r'^[a-z0-9][a-z0-9-]*\.(?P<section>[a-z]+)\.md$')
UNRELEASED = '## [Unreleased]'
COMPARE = re.compile(r'^\[Unreleased\]: (?P<base>\S+)/compare/v(?P<previous>\S+?)\.\.\.HEAD$', re.M)


def fragments(root):
    """Return (section key, path) for every fragment, failing on malformed ones."""
    directory = root / FRAGMENTS
    found, errors = [], []
    for path in sorted(directory.iterdir()) if directory.is_dir() else []:
        if path.name == 'README.md':
            continue
        match = NAME.match(path.name)
        if not path.is_file() or match is None or match['section'] not in SECTIONS:
            errors.append(f'{path.relative_to(root)}: name must be <name>.<section>.md with '
                          f'section one of {", ".join(SECTIONS)}')
            continue
        text = path.read_text()
        if not text.strip():
            errors.append(f'{path.relative_to(root)}: fragment is empty')
        elif not text.lstrip().startswith('- '):
            errors.append(f'{path.relative_to(root)}: fragment must start with a "- " list item')
        elif re.search(r'^#', text, re.M):
            errors.append(f'{path.relative_to(root)}: fragment must not contain headings')
        found.append((match['section'], path))
    if errors:
        raise ValueError('\n'.join(errors))
    return found


def unreleased_bounds(changelog):
    start = changelog.find(UNRELEASED + '\n')
    if start < 0:
        raise ValueError(f'CHANGELOG.md has no "{UNRELEASED}" heading')
    body = start + len(UNRELEASED) + 1
    end = changelog.find('\n## [', body)
    return start, body, len(changelog) if end < 0 else end + 1


def merge(body, entries):
    """Add fragment entries to the first matching `###` block, else a new block."""
    blocks = re.split(r'(?m)^(?=### )', body.strip('\n'))
    blocks = [block.rstrip('\n') for block in blocks if block.strip()]
    for key, title in SECTIONS.items():
        items = [entry for section, entry in entries if section == key]
        if not items:
            continue
        text = '\n'.join(items)
        heading = f'### {title}'
        index = next((i for i, block in enumerate(blocks)
                      if block.split('\n', 1)[0].strip() == heading), None)
        if index is None:
            blocks.append(f'{heading}\n\n{text}')
        else:
            blocks[index] = f'{blocks[index]}\n{text}'
    return '\n\n'.join(blocks) + '\n\n' if blocks else ''


def assemble(root, version, date):
    if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?', version):
        raise ValueError(f'version must be X.Y.Z without a leading v: {version}')
    path = root / 'CHANGELOG.md'
    changelog = path.read_text()
    if f'## [{version}]' in changelog:
        raise ValueError(f'CHANGELOG.md already has a section for {version}')
    found = fragments(root)
    entries = [(section, fragment.read_text().strip('\n')) for section, fragment in found]
    start, body, end = unreleased_bounds(changelog)
    released = merge(changelog[body:end], entries)
    if not released.strip():
        raise ValueError('nothing to release: no fragments and an empty Unreleased section')
    changelog = (changelog[:start] + f'{UNRELEASED}\n\n## [{version}] - {date}\n\n'
                 + released + changelog[end:])
    link = COMPARE.search(changelog)
    if link:
        base, previous = link['base'], link['previous']
        changelog = (changelog[:link.start()]
                     + f'[Unreleased]: {base}/compare/v{version}...HEAD\n'
                     + f'[{version}]: {base}/compare/v{previous}...v{version}'
                     + changelog[link.end():])
    path.write_text(changelog)
    for _, fragment in found:
        fragment.unlink()
    return len(found)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='command', required=True)
    check = commands.add_parser('check', help='validate fragment names and contents')
    check.add_argument('--none-pending', action='store_true',
                       help='also fail if any fragment is left (release tags)')
    run = commands.add_parser('assemble', help='move fragments into a new version section')
    run.add_argument('version')
    run.add_argument('--date', default=datetime.date.today().isoformat())
    args = parser.parse_args(argv)
    try:
        if args.command == 'check':
            found = fragments(ROOT)
            if args.none_pending and found:
                raise ValueError('unreleased changelog fragments remain; run '
                                 '`python3 scripts/release/changelog.py assemble <version>` '
                                 'in the release PR:\n'
                                 + '\n'.join(str(path.relative_to(ROOT)) for _, path in found))
            print(f'changelog fragments: {len(found)} valid')
        else:
            count = assemble(ROOT, args.version, args.date)
            print(f'changelog: assembled {count} fragment(s) into [{args.version}]')
    except ValueError as error:
        print(f'changelog: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
