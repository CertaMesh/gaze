"""Reproduce scorecards, gates and source attribution from value-free records."""
import collections
import gzip
import hashlib
import json
import os
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parents[5]
EVIDENCE=Path(__file__).resolve().parent
sys.path.insert(0,str(ROOT/'scripts/bench'))
import agentic_layers as agentic
import bench_json
import gaze_bench_score as score
import scorecard_record as record
import verify_record_scorecards as verify


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def card(comparison,side,version):
    return bench_json.load(EVIDENCE/f'{comparison}-{side}-v{version}.json.gz')


def escape_cells():
    result={}
    for layer in ('A','D','R'):
        result[layer]={}
        for side in ('base','candidate'):
            selected=[row['utf8_bytes'] for cell,row in card('main',side,2)['layers'][layer]['runs'][0]['per_cell'].items() if 'credentials_escapes_' in cell]
            result[layer][side]={key:sum(row[key] for row in selected) for key in ('pii','leaked','false_positive')}
    return result


def false_positives():
    cards={side:card('main',side,2) for side in ('base','candidate')}
    changed={}
    for layer in ('A','D','R'):
        base=cards['base']['layers'][layer]['runs'][0]['per_cell']
        candidate=cards['candidate']['layers'][layer]['runs'][0]['per_cell']
        rows=[]
        for cell in base.keys()|candidate.keys():
            old=base[cell]['utf8_bytes']['false_positive']
            new=candidate[cell]['utf8_bytes']['false_positive']
            if old!=new:
                rows.append(dict(cell=cell,base=old,candidate=new,delta=new-old))
        rows.sort(key=lambda row:(-row['delta'],row['cell']))
        changed[layer]=dict(delta=sum(row['delta'] for row in rows),cells=rows)
    selected={row['cell'] for layer in changed.values() for row in layer['cells']}
    version=cards['base']['layers']['generator']['generator_version']
    generated=agentic.records_as_of(version,agentic.generate('test'))
    assert hashlib.sha256(agentic.corpus_bytes(generated)).hexdigest()==cards['base']['layers']['generator']['corpus_sha256']
    synthetic={row.uid:row for row in generated}
    totals={}
    for side,source in cards.items():
        found=collections.Counter()
        with gzip.open(EVIDENCE/source['observation_record']['file'],'rt',encoding='utf-8') as stream:
            header=json.loads(next(stream))
            documents={row['id']:row for row in header['documents'] if row['cell'] in selected}
            for line in stream:
                observation=json.loads(line)
                document=documents.get(observation['document_id'])
                if document is None or observation['response'].get('refused'):
                    continue
                text=synthetic[document['id']].text.encode()
                gold=score.merge_intervals((span[0],span[1]) for span in document['gold'])
                for item in observation['response']['final_protection_trace']:
                    outside=score.subtract_intervals([(item['raw_start'],item['raw_end'])],gold)
                    for start,end in outside:
                        context='SQL keyword VALUES' if text[start:end]==b'VALUES' else 'other synthetic context'
                        source_id='+'.join(item['provenance']['source_ids'])
                        found[(document['layer'],source_id,item['class'],context)]+=end-start
        totals[side]=found
    attribution=[]
    for key in sorted(totals['base'].keys()|totals['candidate'].keys()):
        old,new=totals['base'][key],totals['candidate'][key]
        if old!=new:
            attribution.append(dict(layer=key[0],source=key[1],predicted_class=key[2],context=key[3],base=old,candidate=new,delta=new-old))
    for layer,details in changed.items():
        assert sum(row['delta'] for row in attribution if row['layer']==layer)==details['delta']
    return dict(schema_version=1,method='Non-gold byte portions of retained final protection traces in every generated cell with changed raw false positives; joined to committed synthetic generator inputs.',changed_cell_totals=changed,source_context_deltas=attribution)


def materialize_policies():
    manifest = json.loads((EVIDENCE / 'policies.json').read_text())
    for comparison in ('main', 'mechanism'):
        for side in ('base', 'candidate'):
            entry = manifest[f'{comparison}-{side}']
            snapshot = EVIDENCE / entry['file']
            assert snapshot.resolve().is_relative_to((EVIDENCE / 'policies').resolve())
            raw = snapshot.read_bytes()
            digest = hashlib.sha256(raw).hexdigest()
            assert digest == entry['sha256']
            for version in (1, 2, 3, 4):
                source = card(comparison, side, version)
                proof = source['runner_provenance']['policy']
                assert digest == proof['sha256'] == source['parameters']['policy_sha256']
                assert proof['path'] == entry['recorded_path']
            # The gate reads these exact bytes through the original provenance.
            # Main policies are checked above; only mechanism gates open a policy.
            if comparison == 'mechanism':
                destination = Path(entry['recorded_path'])
                assert not destination.is_absolute() and '~' not in destination.parts
                assert destination.resolve().is_relative_to(ROOT.resolve())
                destination.parent.mkdir(parents=True, exist_ok=True)
                if destination.exists():
                    assert destination.read_bytes() == raw, destination
                else:
                    destination.write_bytes(raw)


def main():
    if sys.argv[1:] not in ([],['--record']):
        raise SystemExit('Usage: python3 verify.py [--record]')
    os.chdir(ROOT)
    materialize_policies()
    checks=[]
    for comparison in ('main','mechanism'):
        for side in ('base','candidate'):
            for version in (1,2,3,4):
                path=EVIDENCE/f'{comparison}-{side}-v{version}.json.gz'
                expected=bench_json.load(path)
                archive=EVIDENCE/expected['observation_record']['file']
                assert sha(archive)==expected['observation_record']['sha256']
                relative=f'docs/reference/benchmarks/scored-labels-v{version}.json'
                contract=score.SCORED_LABEL_CONTRACT_V1 if version==1 else score.load_scored_label_contract(ROOT/relative,display_path=relative)
                actual=record.rescore(archive,contract,max_workers=1)
                assert verify.exact_except_timing(actual,expected),path.name
                checks.append(dict(file=path.name,sha256=sha(path),record_sha256=sha(archive),exact_except_timing=True))
                print('REPLAY_OK',path.name,flush=True)
        for version in (1,2,3,4):
            delta=Path('scripts/bench/mechanisms/password-code-credentials.toml') if comparison=='mechanism' else None
            actual=agentic.gate(card(comparison,'base',version),card(comparison,'candidate',version),policy_delta=delta)
            path=EVIDENCE/f'{comparison}-gate-v{version}.json'
            assert actual==json.loads(path.read_text()) and actual['verdict']=='pass',path.name
            checks.append(dict(file=path.name,sha256=sha(path),gate_reproduces=True,verdict='pass'))
    equivalence={}
    for version in (1,2,3,4):
        main_gate=json.loads((EVIDENCE/f'main-gate-v{version}.json').read_text())
        mechanism_gate=json.loads((EVIDENCE/f'mechanism-gate-v{version}.json').read_text())
        equivalence[f'v{version}']=dict(same_layer_metrics=main_gate['layers']==mechanism_gate['layers'])
    derived={'source-escape-cells.json':escape_cells(),'false-positive-attribution.json':false_positives(),'mechanism-main-equivalence.json':equivalence}
    for filename,value in derived.items():
        path=EVIDENCE/filename
        if sys.argv[1:]==['--record']:
            path.write_text(json.dumps(value,indent=2)+'\n')
        else:
            assert json.loads(path.read_text())==value,filename
        checks.append(dict(file=filename,sha256=sha(path),reproduces=True))
    result=dict(schema_version=1,method='Retained-record rescore; exact comparison except timing; gain gates; byte trace attribution.',checks=checks)
    path=EVIDENCE/'verification.json'
    if sys.argv[1:]==['--record']:
        path.write_text(json.dumps(result,indent=2)+'\n')
    else:
        assert json.loads(path.read_text())==result
    print('SOURCE_ESCAPE_EVIDENCE_VERIFIED',flush=True)


if __name__=='__main__':
    main()
