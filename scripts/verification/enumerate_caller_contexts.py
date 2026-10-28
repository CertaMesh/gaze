"""Enumerate benchmark caller inputs without running detection or a benchmark.

Run from the repository root with the pinned Dataiku parquet and an output dir.
The resulting contexts.jsonl is private input for the caller_context_probe example.
Only aggregate counts and hashes belong in public evidence.
"""
import argparse
import hashlib
import json
import sys
import subprocess
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts/bench'))
import known_record_arm as arm
import known_record_cells as cells
import dictionary_sequences as dictionaries
import dictionary_sequence_generators as registry
import perf_invocation_snapshot as perf

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--dataset', type=Path, required=True)
parser.add_argument('--output-dir', type=Path, required=True)
args = parser.parse_args()
out = args.output_dir
out.mkdir(parents=True, exist_ok=True)
counts = Counter()
hasher = hashlib.sha256()
sources = {}
for p in sorted((ROOT / 'scripts/bench').glob('*.py')):
    sources[str(p.relative_to(ROOT))] = hashlib.sha256(p.read_bytes()).hexdigest()
for p in [ROOT / 'docs/reference/benchmarks/scored-labels-v2.json', ROOT / 'docs/reference/benchmarks/scored-labels-agentic.json', ROOT / 'scripts/bench/lookalike_cue_vocabulary.json', ROOT / 'crates/gaze-cli/src/commands/setup.rs']:
    sources[str(p.relative_to(ROOT))] = hashlib.sha256(p.read_bytes()).hexdigest()
for p in [ROOT / 'docs/reference/benchmarks/dictionary-sequences/generator-v1.py.gz', ROOT / 'crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl', ROOT / 'crates/gaze-recognizers/assets/record-common-names-v1.txt']:
    sources[str(p.relative_to(ROOT))] = hashlib.sha256(p.read_bytes()).hexdigest()
dataset = args.dataset
sources['dataset/test.parquet'] = hashlib.sha256(dataset.read_bytes()).hexdigest()
policy = {'rule': [{'kind': 'default', 'action': 'tokenize'}]}

context_file = out / 'contexts.jsonl'
context_file.touch(mode=0o600)
context_file.chmod(0o600)
with context_file.open('w') as stream:
    def emit(name, context):
        if context is None:
            return
        context = context if isinstance(context, str) else json.dumps(context, ensure_ascii=False)
        row = json.dumps({'arm': name, 'context': context}, ensure_ascii=False, separators=(',', ':')) + '\n'
        stream.write(row)
        hasher.update(row.encode())
        counts[name] += 1

    for contract in ('v1', 'v2'):
        layers = arm.corpus(ROOT, contract, dataset)
        pool = {}
        for doc in layers['C'] + layers['A']:
            context, _ = arm.record_for_document(doc, policy)
            if context is not None:
                pool.setdefault(doc.language, []).append(context)
        counterdocs, countercontexts = arm.explicit_counterweights(pool)
        layers['D'] += counterdocs
        pairs = cells.generate()
        layers['K'], _ = cells.documents(ROOT, contract, pairs)
        for shipping in (True, False):
            kindcontexts = arm.kind_contexts_for_measurement(pairs, shipping_defaults=shipping)
            for layer, docs in layers.items():
                core = [doc for doc in docs if doc.uid not in countercontexts and doc.uid not in kindcontexts]
                contexts, _ = arm.paired_records(core, policy, pool if layer == 'D' else None)
                selected = {doc.uid for doc in docs}
                contexts.update({uid: value for uid, value in countercontexts.items() if uid in selected})
                contexts.update({uid: value for uid, value in kindcontexts.items() if uid in selected})
                for context in contexts.values():
                    emit(f'known-record/{contract}/{"shipping" if shipping else "probe"}/{layer}', context if shipping else arm.enable_name_multi_measurement(context))
    for cell in cells.cells(pairs):
        emit('known-record-cells/raw', cell.context_json())
        for enabled in (False, True):
            emit(f'known-record-cells/kinds-{enabled}', cells.arm_context(cell, enabled))
    pinned = registry.resolve(registry.DEFAULT_GENERATOR)
    for label, generator in [('current', dictionaries), ('pinned', pinned)]:
        for partition in ('published', 'calibration'):
            for case in generator.generate(dictionaries.SEED, partition):
                emit(f'dictionary/{label}/{partition}', case.context())
    for fixture in perf.fixtures():
        for request in fixture['requests']:
            if 'context' in request:
                emit('perf-snapshot', request['context'])
    emit('perf-hook-deletion', {})
    emit('positive-control', {'record': {'name': 'Qx'}})

identity = {'generator_source_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(), 'contexts_per_arm': dict(counts), 'context_stream_sha256': hasher.hexdigest(), 'generator_input_sha256': hashlib.sha256(json.dumps(sources, sort_keys=True).encode()).hexdigest(), 'dataset_sha256': sources['dataset/test.parquet'], 'policy': policy, 'dictionary_seed': dictionaries.SEED, 'positive_control_expected': 1}
(out / 'inputs.json').write_text(json.dumps(identity, indent=2) + '\n')
print(json.dumps({k: v for k, v in identity.items() if k != 'sources'}, indent=2))
