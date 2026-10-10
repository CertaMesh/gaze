#!/usr/bin/env python3
"""The original 5,600-input phone differential, under its core-only daemon policy.

Usage: python phone_differential_enumeration.py BASE_GAZE CANDIDATE_GAZE OUTPUT
Build each binary with cargo build --release -p gaze-cli --all-features.
Gold, display separators and the mixed active locales reproduce the original
phone proof. Report phone-class coverage separately from all-class protection.
Assert no newly lost phone coverage and no raw gold bytes on the candidate.
Fixture sources: BNetzA Mitteilung 148/2021, Ofcom drama mobile and London
020 7946 0xxx ranges, NANPA 202-555-01xx, ARCEP fiction 02.61.91.xx.xx.
"""
from __future__ import annotations
import json
import subprocess
import sys
import tempfile
from pathlib import Path

POLICY = 'schema_version = "0.1.0"\n[session]\nscope = "conversation"\n[locale]\nactive = ["global", "de-DE", "en-GB", "en-US"]\n[policy.rulepacks]\nbundled = ["core"]\n[[rule]]\nkind = "default"\naction = "tokenize"\n'


def cases():
    cases=[]
    def add(family,text,values=()):
        raw=text.encode(); gold=set(); at=0
        for value in values:
            value=value.encode(); start=raw.index(value,at); gold.update(range(start,start+len(value))); at=start+len(value)
        cases.append((family,text,gold))
    for n in range(100):
        de=f"0171 39200{n:02d}"; de2=f"0171 39200{(n+1)%100:02d}"
        gb=f"020 7946 0{n:03d}"; gb2=f"020 7946 0{(n+1)%100:03d}"
        us=f"202 555 01{n:02d}"; us2=f"202 555 01{(n+1)%100:02d}"
        mobile=f"+44 7700 900{n:03d}"
        fr=f"02.61.91.{n:02d}.{(n+1)%100:02d}"
        for sep in [" ",","," / ","/","\t","\u00a0","\n"," or "," oder "]:
            for family,values in [('de_national',(de,de2)),('gb_cued',(gb,gb2)),('us_national',(us,us2)),('de_international',('+49 '+de[1:],'+49 '+de2[1:]))]:
                text=sep.join(values)
                if family=='gb_cued': text='Phone: '+text
                add(family,text,values)
        for family,text,value in [
            ('gb_reserved',mobile,mobile),('gb_national_cued','Mobile: '+gb,gb),
            ('fr_dotted',fr,fr),('fr_dotted_cued','Tél.: '+fr,fr),
            ('gb_00','0044 '+gb[1:],'0044 '+gb[1:]),
            ('gb_trunk','+44 (0)'+gb[1:],'+44 (0)'+gb[1:]),
            ('us_001','001-'+us.replace(' ','-'),'001-'+us.replace(' ','-'))]: add(family,text,(value,))
        for suffix in ['.',')',' 12 Uhr']:
            add('de_adjacent_tail',de+' '+de2+suffix,(de,de2))
        for field in ['Firmware','build','Version','part number','Catalog part','model']:
            add('benign_dotted',field+': '+fr)
        for field in ['Order','Amount','Invoice','version']:
            add('benign_grouped',field+': '+gb)
    assert len(cases) == 5600
    return cases


def parse_responses(payload):
    # JSONL uses LF boundaries. Unicode line separators can be string data.
    return [json.loads(line) for line in payload.split('\n') if line]


def run(binary, rows, policy):
    requests = ''.join(json.dumps({'session_id': str(i), 'text': text}, ensure_ascii=False) + '\n'
                       for i, (_, text, _) in enumerate(rows))
    result = subprocess.run([str(binary), 'daemon', '--policy', str(policy)], input=requests,
                            text=True, capture_output=True, check=True, timeout=600)
    responses = parse_responses(result.stdout)
    if len(responses) != len(rows):
        raise RuntimeError('response count')
    coverage = []
    for (_, text, gold), response in zip(rows, responses, strict=True):
        if 'error' in response:
            raise RuntimeError(response['error'])
        phone = set()
        all_covered = set()
        clean = response['clean_text'].encode()
        for span in response['manifest']:
            a, b = span['clean_span']['start'], span['clean_span']['end']
            start, end = span['raw_span']['start'], span['raw_span']['end']
            if not (0 <= a < b <= len(clean) and 0 <= start < end <= len(text.encode())):
                raise RuntimeError('invalid manifest bounds')
            all_covered.update(range(start, end))
            if b':Custom:phone_' in clean[a:b]:
                phone.update(range(start, end))
        coverage.append((gold - phone, phone - gold, gold - all_covered))
    return coverage


def main():
    base_binary, candidate_binary, output = map(Path, sys.argv[1:4])
    rows = cases()
    with tempfile.TemporaryDirectory(prefix='gaze-phone-differential-') as temporary:
        policy = Path(temporary) / 'policy.toml'
        policy.write_text(POLICY)
        base = run(base_binary, rows, policy)
        candidate = run(candidate_binary, rows, policy)
    report = {'cases': len(rows), 'families': {}}
    for family in sorted({row[0] for row in rows}):
        indexes = [i for i, row in enumerate(rows) if row[0] == family]
        report['families'][family] = {
            'cases': len(indexes),
            'base_leaked_bytes': sum(len(base[i][0]) for i in indexes),
            'candidate_leaked_bytes': sum(len(candidate[i][0]) for i in indexes),
            'base_raw_leaked_bytes': sum(len(base[i][2]) for i in indexes),
            'candidate_raw_leaked_bytes': sum(len(candidate[i][2]) for i in indexes),
            'newly_protected_bytes': sum(len(base[i][0] - candidate[i][0]) for i in indexes),
            'newly_lost_bytes': sum(len(candidate[i][0] - base[i][0]) for i in indexes),
            'base_fp_bytes': sum(len(base[i][1]) for i in indexes),
            'candidate_fp_bytes': sum(len(candidate[i][1]) for i in indexes),
        }
    report['pass'] = all(r['newly_lost_bytes'] == 0 and r['candidate_raw_leaked_bytes'] == 0
                         for r in report['families'].values())
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    return int(not report['pass'])


if __name__ == '__main__':
    raise SystemExit(main())
