#!/usr/bin/env python3
"""Prove detection source identity after removing the exact added timer hooks."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
FILES = (
    "crates/gaze-assembly/src/lib.rs",
    "crates/gaze-cli/src/commands/daemon.rs",
    "crates/gaze-cli/src/main.rs",
    "crates/gaze-cli/src/pipeline/build.rs",
    "crates/gaze-recognizers/src/bundle.rs",
    "crates/gaze-recognizers/src/dob_judge.rs",
    "crates/gaze-recognizers/src/ner/backend/ort.rs",
    "crates/gaze-recognizers/src/ner/detector.rs",
    "crates/gaze-recognizers/src/ner/loader.rs",
    "crates/gaze-recognizers/src/safety_net/nym/ort.rs",
)


def without_hooks(source):
    source = re.sub(r'^\s*#\[cfg\(feature = "perf-timers"\)\]\n\s*(?:crate::perf::(?:ready|policy_loaded)|perf::start)\(\);\n', '', source, flags=re.M)
    source = re.sub(r'^ *let (?:_construction|policy_load|_assembly|rules|_policy_rules|_attach|_constructor|tokenizer_load|model_load|_verify|outer_verify|startup) = gaze_types::perf_span!\("[a-z_]+"\);\n', '', source, flags=re.M)
    source = re.sub(r'^ *drop\((?:policy_load|rules|tokenizer_load|model_load|outer_verify|startup)\);\n', '', source, flags=re.M)
    return source


def identity(base):
    records = []
    for name in FILES:
        before = subprocess.check_output(['git', 'show', f'{base}:{name}'], cwd=ROOT)
        after = (ROOT / name).read_bytes()
        stripped = without_hooks(after.decode()).encode()
        row = {'file': name, 'base_sha256': hashlib.sha256(before).hexdigest(),
               'candidate_sha256': hashlib.sha256(after).hexdigest(),
               'without_hooks_sha256': hashlib.sha256(stripped).hexdigest()}
        if stripped != before:
            raise ValueError(f'non-hook source change in {name}')
        records.append(row)
    return {'base': subprocess.check_output(['git', 'rev-parse', base], cwd=ROOT, text=True).strip(),
            'files': records, 'text_path_identical_without_hooks': True}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    args.out.write_text(json.dumps(identity(args.base), indent=2) + '\n')
