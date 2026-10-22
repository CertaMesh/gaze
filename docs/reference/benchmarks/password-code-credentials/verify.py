"""Re-derive each published scorecard and gate from the portable records."""
import hashlib
import collections
import gzip
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
EVIDENCE = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / 'scripts/bench'))
import gaze_bench_score as score
import scorecard_record as record
import verify_record_scorecards as verify
import bench_json
import agentic_layers as agentic

if sys.argv[1:] not in ([], ['--record']):
    raise SystemExit('Usage: python3 verify.py [--record]')


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


checks = []
for side in ('base', 'candidate'):
    for suffix, version in [('v1', 1), ('v2', 2), ('v3', 3), ('v4', 4), ('native-v1', 1)]:
        path = EVIDENCE / (side + '-' + suffix + '.json.gz')
        expected = bench_json.load(path)
        archive = EVIDENCE / expected['observation_record']['file']
        assert sha(archive) == expected['observation_record']['sha256']
        if version == 1:
            contract = score.SCORED_LABEL_CONTRACT_V1
        else:
            relative = 'docs/reference/benchmarks/scored-labels-v' + str(version) + '.json'
            contract = score.load_scored_label_contract(ROOT / relative, display_path=relative)
        actual = record.rescore(archive, contract, max_workers=1)
        matches = verify.exact_except_timing(actual, expected)
        if not matches:
            raise AssertionError((path.name, [key for key in set(actual) | set(expected)
                if verify.without_timing(actual.get(key)) != verify.without_timing(expected.get(key))]))
        checks.append(dict(file=path.name, file_sha256=sha(path), content_sha256=bench_json.content_sha256(path), record_sha256=sha(archive),
                           exact_except_timing=True))
        print('REPLAY_OK', path.name, flush=True)

with tempfile.TemporaryDirectory() as temporary:
    for suffix in ('v1', 'v2', 'v3', 'v4', 'native-v1'):
        measured_gate = EVIDENCE / ('gate-' + suffix + '.json')
        output = Path(temporary) / measured_gate.name
        inputs = {}
        for side in ('base', 'candidate'):
            source = EVIDENCE / (side + '-' + suffix + '.json.gz')
            inputs[side] = Path(temporary) / (side + '-' + suffix + '.json')
            inputs[side].write_bytes(bench_json.read_content(source))
        command = [sys.executable, str(ROOT / 'scripts/bench/agentic_layers.py'), 'gate',
                   '--base', str(inputs['base']),
                   '--candidate', str(inputs['candidate']),
                   '--output', str(output)]
        subprocess.run(command, cwd=ROOT, check=True)
        actual = json.loads(output.read_text(encoding='utf-8'))
        assert actual == json.loads(measured_gate.read_text(encoding='utf-8')), suffix
        assert actual['verdict'] == 'pass', suffix
        checks.append(dict(file=measured_gate.name, sha256=sha(measured_gate),
                           gate_reproduces=True, verdict='pass'))
# Attribute every changed generated-cell FP byte from the retained trace.
cards = {side: bench_json.load(EVIDENCE / (side + '-v2.json.gz')) for side in ('base', 'candidate')}
cell_deltas = {}
for layer in ('A', 'D', 'R'):
    base = cards['base']['layers'][layer]['runs'][0]['per_cell']
    candidate = cards['candidate']['layers'][layer]['runs'][0]['per_cell']
    changed = []
    for cell in base.keys() | candidate.keys():
        old, new = base[cell]['utf8_bytes']['false_positive'], candidate[cell]['utf8_bytes']['false_positive']
        if old != new:
            changed.append(dict(cell=cell, base=old, candidate=new, delta=new-old))
    changed.sort(key=lambda row: (-row['delta'], row['cell']))
    cell_deltas[layer] = dict(delta=sum(row['delta'] for row in changed), cells=changed)
selected = {row['cell'] for layer in cell_deltas.values() for row in layer['cells']}
version = cards['base']['layers']['generator']['generator_version']
generated = agentic.records_as_of(version, agentic.generate('test'))
assert hashlib.sha256(agentic.corpus_bytes(generated)).hexdigest() == cards['base']['layers']['generator']['corpus_sha256']
synthetic = {row.uid: row for row in generated}
totals = {}
for side, card in cards.items():
    found = collections.Counter()
    with gzip.open(EVIDENCE / card['observation_record']['file'], 'rt', encoding='utf-8') as stream:
        header = json.loads(next(stream))
        documents = {row['id']: row for row in header['documents'] if row['cell'] in selected}
        for line in stream:
            observation = json.loads(line)
            document = documents.get(observation['document_id'])
            if document is None or observation['response'].get('refused'):
                continue
            text = synthetic[document['id']].text.encode()
            gold = score.merge_intervals((span[0], span[1]) for span in document['gold'])
            for item in observation['response']['final_protection_trace']:
                outside = score.subtract_intervals([(item['raw_start'], item['raw_end'])], gold)
                for start, end in outside:
                    context = 'SQL keyword VALUES' if text[start:end] == b'VALUES' else 'other synthetic context'
                    source = '+'.join(item['provenance']['source_ids'])
                    found[(document['layer'], source, item['class'], context)] += end-start
    totals[side] = found
attribution = []
for key in sorted(totals['base'].keys() | totals['candidate'].keys()):
    base, candidate = totals['base'][key], totals['candidate'][key]
    if base != candidate:
        attribution.append(dict(layer=key[0], source=key[1], predicted_class=key[2], context=key[3],
                                base=base, candidate=candidate, delta=candidate-base))
for layer, details in cell_deltas.items():
    assert sum(row['delta'] for row in attribution if row['layer'] == layer) == details['delta']
attribution_report = dict(schema_version=1,
    method='Non-gold portions of retained final protection traces in every cell with changed raw FP; byte intervals joined to committed synthetic generator inputs.',
    changed_cell_totals=cell_deltas, source_context_deltas=attribution)
assert json.loads((EVIDENCE / 'false-positive-attribution.json').read_text(encoding='utf-8')) == attribution_report
checks.append(dict(file='false-positive-attribution.json', sha256=sha(EVIDENCE / 'false-positive-attribution.json'),
                   exact_trace_attribution=True))
print('FALSE_POSITIVE_ATTRIBUTION_VERIFIED')

report = {'schema_version': 1,
    'method': 'scorecard_record.rescore(max_workers=1); exact_except_timing; agentic_layers.py gate',
    'checks': checks}
if sys.argv[1:] == ['--record']:
    (EVIDENCE / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
elif sys.argv[1:]:
    raise SystemExit('Usage: python3 verify.py [--record]')
else:
    assert json.loads((EVIDENCE / 'verification.json').read_text(encoding='utf-8')) == report
print('PUBLISHED_MAIN_EVIDENCE_VERIFIED')
