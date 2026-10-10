#!/usr/bin/env python3
"""Kill phone regression mutations with committed assertions; always restore sources.

Run from the repository root: python scripts/bench/phone_mutation_probe.py
No parallel builds or source writers may use this worktree during the probe.
"""
from __future__ import annotations
import json
import shutil
import subprocess
from pathlib import Path


def insert_function(source, name, line):
    start = source.index('    fn ' + name + '(')
    body = source.index('{', start) + 1
    return source[:body] + '\n' + line + '\n' + source[body:]


def disable_formats(source):
    for name in ('phone.e164.spaced', 'phone.e164.spaced.cued'):
        start = source.index('id = "' + name + '"')
        enabled = source.index('enabled = true', start)
        source = source[:enabled] + source[enabled:].replace('enabled = true', 'enabled = false', 1)
    return source


def main():
    root = Path.cwd()
    output = root / 'target/phones-evidence'
    output.mkdir(parents=True, exist_ok=True)
    regex = root / 'crates/gaze-recognizers/src/regex.rs'
    registry = root / 'crates/gaze/src/registry.rs'
    core = root / 'crates/gaze-recognizers/embedded/core.toml'
    common = ['-p', 'gaze-recognizers', '--test', 'core_extended']
    mutants = [
        ('plus-prefix', registry, lambda s: s.replace(
            '!input[candidate.span.clone()].trim().is_empty()',
            'input[candidate.span.clone()].bytes().any(|byte| byte.is_ascii_digit())'),
            common + ['phone_card_overlap_keeps_the_international_plus_prefix_protected']),
        ('adjacency', regex, lambda s: insert_function(s, 'phone_parts',
            '        if !input.is_empty() { return vec![span]; }'), common + ['adjacent_reserved_phones']),
        ('ipv4-tail', regex, lambda s: insert_function(s, 'ipv4_phone_tail',
            '        if !input.is_empty() { return false; }'),
            common + ['phone_and_ipv4_suppressions_are_audited_and_oid_ips_remain_eligible']),
        ('national-formats', core, disable_formats, common + ['expanded_phone_formats']),
        ('e164-limit', root / 'crates/gaze-types/src/lib.rs',
            lambda s: s.replace('> 15\n', '> 150\n'),
            common + ['phone_number_validator_enforces_e164_fifteen_digit_cap']),
        ('locale-veto', registry, lambda s: s.replace(
            'claimed.extend(survivors.iter().map(|candidate| &candidate.span));',
            'claimed.extend(class_candidates.iter().map(|candidate| &candidate.span));'),
            ['-p', 'gaze-pii', '--lib', 'vetoed_locale_candidate_does_not_block_partial_overlap_fallback']),
    ]
    results = []
    for name, path, change, args in mutants:
        if shutil.disk_usage('/').free < 40 * 1024**3:
            raise RuntimeError('Disk below 40 GiB; stop and report')
        original = path.read_text()
        mutated = change(original)
        if original == mutated:
            raise RuntimeError('Mutation did not change source: ' + name)
        try:
            path.write_text(mutated)
            result = subprocess.run(['cargo', 'test', *args], capture_output=True,
                                    text=True, timeout=600)
            log = result.stdout + result.stderr
            (output / ('mutation-' + name + '.log')).write_text(log)
            killed = result.returncode != 0 and 'test result: FAILED' in log
            results.append({'mutation': name, 'exit': result.returncode,
                            'killed_by_assertion': killed})
            if not killed:
                raise RuntimeError('Mutation not killed by test assertion: ' + name)
        finally:
            path.write_text(original)
    (output / 'mutations.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()
