#!/usr/bin/env python3
"""5,600 deterministic phone/benign inputs for paired clean_for_bench binaries.

Usage: python phone_differential_enumeration.py BASE_BIN CANDIDATE_BIN OUTPUT
Build each binary with cargo build --release -p gaze-recognizers --example clean_for_bench.
The runner's default rule-floor-extended policy is used on both sides. This
supplements the full benchmark; it asserts no previously protected phone digit
is lost, no candidate phone digit remains raw, and exact restore/valid manifest.
Report display-byte coverage and false positives separately, without raw values.

Fixture origins: BNetzA Mitteilung 148/2021 (0171 39200xx), Ofcom drama mobile
07700 900xxx, NANPA 202-555-01xx, and ARCEP reserved fiction 02.61.91.xx.xx.
The enumeration samples display syntax and list separators, never corpus values.
"""
from __future__ import annotations
import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

SEPARATORS = (' ', ' / ', '\t', ',', ' or ', ' oder ', '\n', '\u2028', ' /\n')


def cases():
    rows = []
    def add(family, text, values, locale):
        gold = set()
        digits = set()
        at = 0
        for value in values:
            start = text.index(value, at)
            a, b = len(text[:start].encode()), len(text[:start + len(value)].encode())
            gold.update(range(a, b))
            digits.update(i for i in range(a, b) if 48 <= text.encode()[i] <= 57)
            at = start + len(value)
        rows.append((family, text, gold, digits, locale))
    for n in range(100):
        de = f'0171 39200{n:02d}'
        next_de = f'0171 39200{(n + 1) % 100:02d}'
        gb = f'+44 7700 900{n:03d}'
        ngb = f'07700 900{n:03d}'
        us = f'202 555 01{n:02d}'
        fr = f'02.61.91.00.{n:02d}'
        for sep in SEPARATORS:
            add('de_national', f'Tel: {de}{sep}{next_de}', (de, next_de), 'de-DE')
            ide = f'+49 171 39200{n:02d}'
            jde = f'+49 171 39200{(n + 1) % 100:02d}'
            add('de_international', ide + sep + jde, (ide, jde), 'de-DE')
            jgb = f'+44 7700 900{(n + 1) % 100:03d}'
            add('gb_cued', f'Phone: {gb}{sep}{jgb}', (gb, jgb), 'en-GB')
            jus = f'202 555 01{(n + 1) % 100:02d}'
            add('us_national', f'Phone: {us}{sep}{jus}', (us, jus), 'en-US')
        for tail in (' ', ' / ', '\t'):
            add('de_adjacent_tail', de + tail + next_de + '.', (de, next_de), 'de-DE')
        add('fr_dotted', fr, (fr,), 'fr-FR')
        add('fr_dotted_cued', 'Tel: ' + fr, (fr,), 'fr-FR')
        add('gb_00', f'0044 7700 900{n:03d}', (f'0044 7700 900{n:03d}',), 'en-GB')
        add('gb_national_cued', 'Phone: ' + ngb, (ngb,), 'en-GB')
        add('gb_reserved', gb, (gb,), 'en-GB')
        add('gb_trunk', f'+44 (0)7700 900{n:03d}', (f'+44 (0)7700 900{n:03d}',), 'en-GB')
        add('us_001', f'001 202 555 01{n:02d}', (f'001 202 555 01{n:02d}',), 'en-US')
        for prefix in ('Firmware ', 'part=', 'build=', 'Version ', 'OID ', 'Amount: '):
            add('benign_dotted', prefix + fr, (), 'en-US')
        for prefix in ('Amount: ', 'Order ', 'Invoice ', 'Ref '):
            add('benign_grouped', prefix + f'000 {n:03d} 000', (), 'en-US')
    assert len(rows) == 5600
    return rows


def run(binary, rows):
    requests = [{'fixture_id': f'phone-enum-{i:04d}', 'text': text,
                 'locale_chain': [locale, 'global']}
                for i, (_, text, _, _, locale) in enumerate(rows)]
    result = subprocess.run([str(binary)], input=''.join(json.dumps(r) + '\n' for r in requests),
                            capture_output=True, text=True, check=True)
    responses = [json.loads(line) for line in result.stdout.splitlines()]
    if len(responses) != len(rows):
        raise RuntimeError('missing runner responses')
    indexed = {r['fixture_id']: r for r in responses}
    if len(indexed) != len(rows):
        raise RuntimeError('duplicate runner response')
    return [indexed[f'phone-enum-{i:04d}'] for i in range(len(rows))]


def protected(response):
    return {byte for span in response['final_protection_trace']
            for byte in range(span['raw_start'], span['raw_end'])}


def main():
    base, candidate, output = map(Path, sys.argv[1:4])
    rows = cases()
    responses = [run(binary, rows) for binary in (base, candidate)]
    totals = defaultdict(lambda: defaultdict(int))
    failures = []
    for i, ((family, _, gold, digits, _), b, c) in enumerate(zip(rows, *responses, strict=True)):
        if any('pipeline_error_code' in r for r in (b, c)):
            failures.append({'case': i, 'reason': 'pipeline_error'})
            continue
        pb, pc = protected(b), protected(c)
        s = totals[family]
        s['cases'] += 1
        s['base_leaked_bytes'] += len(gold - pb)
        s['candidate_leaked_bytes'] += len(gold - pc)
        s['base_raw_leaked_bytes'] += len(digits - pb)
        s['candidate_raw_leaked_bytes'] += len(digits - pc)
        s['newly_lost_bytes'] += len((gold & pb) - pc)
        s['newly_lost_digit_bytes'] += len((digits & pb) - pc)
        s['newly_protected_bytes'] += len((gold & pc) - pb)
        s['base_fp_bytes'] += len(pb - gold)
        s['candidate_fp_bytes'] += len(pc - gold)
        if ((digits & pb) - pc or digits - pc or not c['restore']['exact']
                or any(value != 0 for key, value in c['manifest_integrity'].items() if key != 'spans')):
            failures.append({'case': i, 'reason': 'digit_coverage_or_restore_or_manifest'})
    report = {'cases': len(rows), 'families': {k: dict(v) for k, v in sorted(totals.items())},
              'failures': failures, 'pass': not failures}
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    return int(bool(failures))


if __name__ == '__main__':
    raise SystemExit(main())
