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
import shutil

import agentic_layers
import scorecard_record


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def portable_header(header: dict) -> dict:
    result = copy.deepcopy(header)
    policy = result['scorecard'].get('runner_provenance', {}).get('policy')
    if policy is not None:
        value = policy['path']
        policy['path'] = agentic_layers.normalize_home_path(value)
        if Path(policy['path']).is_absolute():
            raise ValueError('policy provenance is not portable')
    return result


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
        'metadata_normalization': 'policy-path-home-prefix' if normalized != header else 'none',
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
