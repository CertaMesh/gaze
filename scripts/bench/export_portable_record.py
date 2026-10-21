"""Export a value-free observation archive with portable policy provenance.

Only the capture header's policy path may change. Observations, source identity,
policy digest, corpus identity and scores are preserved. A receipt pins both the
original capture and its publication copy; the original stays in private custody.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import shutil

import agentic_layers
import scorecard_record


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


# Any user's home directory, not only the exporting machine's own.
HOME_PREFIX = re.compile(r'^(?:/Users/[^/]+|/home/[^/]+|[A-Za-z]:[\\/]+Users[\\/]+[^\\/]+)(?=[\\/]|$)')
# An Anvil worktree is a repository checkout: `~/anvil/workspaces/<project>/<worktree>/<repo path>`.
ANVIL_WORKTREE = re.compile(r'^~/anvil/workspaces/[^/]+/[^/]+/(?P<repo_path>.+)$')


def portable_path(value: str) -> str:
    """Return a repo-relative or `~`-prefixed path; other values pass through unchanged."""
    normalized = agentic_layers.normalize_home_path(value)
    match = HOME_PREFIX.match(normalized)
    if match:
        normalized = '~' + normalized[match.end():].replace('\\', '/')
    worktree = ANVIL_WORKTREE.match(normalized)
    if worktree:
        # Matches run_no_opf_benchmark.portable_repo_path for a policy inside the checkout.
        return worktree.group('repo_path')
    return normalized


def portable_header(header: dict) -> dict:
    result = copy.deepcopy(header)
    policy = result['scorecard'].get('runner_provenance', {}).get('policy')
    if policy is not None:
        value = policy['path']
        policy['path'] = portable_path(value)
        if Path(policy['path']).is_absolute() or HOME_PREFIX.match(policy['path']):
            raise ValueError('policy provenance is not portable')
    return result


def normalization_kind(header: dict, normalized: dict) -> str:
    if normalized == header:
        return 'none'
    path = normalized['scorecard']['runner_provenance']['policy']['path']
    return 'policy-path-home-prefix' if path.startswith('~') else 'policy-path-repo-relative'


def export_record(source: Path, output: Path) -> dict:
    if source.resolve() == output.resolve():
        raise ValueError('publication copy must be separate from the original')
    header, observations = scorecard_record._read(source)
    normalized = portable_header(header)
    if normalized == header:
        shutil.copyfile(source, output)
    else:
        scorecard_record._write_rows(output, [normalized, *observations])
    published_header, published_observations = scorecard_record._read(output)
    if published_header != normalized or published_observations != observations:
        raise ValueError('publication copy changed observation content')
    return {
        'schema_version': 1,
        'source_sha256': sha256(source),
        'exported_sha256': sha256(output),
        'metadata_normalization': normalization_kind(header, normalized),
        'observation_rows_unchanged': True,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    receipt = export_record(args.source, args.output)
    args.receipt.write_text(json.dumps(receipt, indent=2) + '\n')
    print('PORTABLE_RECORD_EXPORT_OK')


if __name__ == '__main__':
    main()
