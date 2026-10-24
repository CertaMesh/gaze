#!/usr/bin/env python3
"""Bounded inference equivalence and diagnostic AB/BA timings; never a full gate."""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import queue
import statistics
import subprocess
import threading
import time

import agentic_layers as agentic
from bench_subprocess import BenchSubprocess
import dataiku_en_de_gaze_bench as dataiku
import gaze_bench_score as score
from perf_instrumentation_proof import canonical

ROOT = Path(__file__).resolve().parents[2]
SEED = 20260710
TYPICAL = ('Please summarize the draft report in plain language. Explain which work is complete, '
           'which questions remain open, and what the next step should be. Use only the supplied text. '
           'Keep the answer brief and factual. If the report does not answer a question, say that the '
           'information is unavailable. Do not infer missing facts or add names.')
SMALL = 'Please review the report and explain the next action clearly.'
PII = ('Please prepare a short summary for the fictional reviewer Dr. Schmidt. The synthetic payment '
       'record lists IBAN DE00 0000 0000 0000 0000 00 and the delivery address Musterstrasse 12, 10115 '
       'Berlin. Treat this as invented test data. Explain the requested action clearly, preserve the '
       'facts, and do not send the raw personal fields outside this local protection check.')
WORKLOADS = [('short', [TYPICAL]), ('pii', [PII]), ('2k', ['The report is ready. ' * 400]),
             *[(f'history_{n}', [TYPICAL] + [SMALL] * (n * 2)) for n in (2, 10, 20)],
             ('10k', ['The report is ready. ' * 2000])]


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def rank(uid):
    return hashlib.sha256(f'{SEED}:{uid}'.encode()).digest()


def corpus(dataset, per_layer):
    heldout, _ = dataiku.load_documents(dataset)
    labels = {'FIRSTNAME', 'SURNAME', 'BUILDINGNUM', 'STREET', 'ZIP', 'DATEOFBIRTH', 'LICENSEPLATENUM'}
    documents = []
    for language in ('en', 'de'):
        candidates = [d for d in heldout if d.language == language]
        candidates.sort(key=lambda d: (-sum(s.label in labels for s in d.spans), rank(d.uid)))
        documents.extend(('C', d) for d in candidates[:per_layer // 2])
    generated = list(agentic.generate(agentic.PUBLISHED_PARTITION))
    for layer in 'ADR':
        candidates = [r for r in generated if r.layer == layer]
        def priority(r):
            nym = any(word in r.family for word in ('name', 'address', 'dob', 'birth', 'block', 'zip', 'date'))
            return (not nym, rank(r.uid))
        # Cover distinct cells before adding another instance of a cell.
        candidates.sort(key=priority)
        chosen, seen = [], set()
        for record in candidates:
            cell = (record.family, record.surface, record.language)
            if cell not in seen:
                chosen.append(record)
                seen.add(cell)
            if len(chosen) == per_layer:
                break
        documents.extend((layer, r.to_document()) for r in chosen)
    if Counter(layer for layer, _ in documents) != Counter({k: per_layer for k in 'ACDR'}):
        raise ValueError('bounded selection lacks a required layer')
    return documents


def request(document):
    return {'fixture_id': document.uid, 'text': document.text,
            'locale_chain': document.locale_chain, 'session_hex': 'deadbeef'}


def without_timing(value):
    return {k: v for k, v in value.items() if k != 'timing'}


def scan_views(response):
    clean = response['clean_text'].encode()
    stable, neutral = bytearray(clean), bytearray(clean)
    for span in response['manifest_spans']:
        start, end = span['clean_start'], span['clean_end']
        token = clean[start:end]
        if not token.isascii():
            continue
        offset = (1 if token.startswith(b'<') and token[9:10] == b':' else
                  token.find(b'.') + 1 if token.startswith(b'email') and token.endswith(b'@gaze-fake.invalid') else
                  0 if token[8:9] == b':' else None)
        if offset is None or len(token[offset:offset + 8]) != 8 or any(
                byte not in b'0123456789abcdef' for byte in token[offset:offset + 8]):
            continue
        normalized = bytearray(token)
        normalized[offset:offset + 8] = b'00000000'
        surrogate = hashlib.sha256(b'gaze-safety-net-token-v3\0' + normalized).hexdigest()[:8].encode()
        stable[start + offset:start + offset + 8] = surrogate
        neutral[start:end] = b'[PII]' + b' ' * (len(token) - 5)
    return stable.decode(), neutral.decode()


def equivalence(args):
    out = args.output
    out.mkdir(parents=True, exist_ok=True)
    documents = corpus(args.dataset, args.per_layer)
    requests = [request(d) for _, d in documents]
    corpus_path = out / 'requests.jsonl'
    corpus_path.write_text(''.join(json.dumps(r, ensure_ascii=False) + '\n' for r in requests))
    inventory = [{'id': d.uid, 'layer': layer, 'text_sha256': hashlib.sha256(d.text.encode()).hexdigest()}
                 for layer, d in documents]
    write(out / 'inventory.json', {'dataset_sha256': sha(args.dataset), 'requests_sha256': sha(corpus_path),
                                 'generator_sha256': sha(ROOT / 'scripts/bench/agentic_layers.py'),
                                 'seed': SEED, 'documents': inventory})
    env = {**os.environ, 'GAZE_BENCH_POLICY': str(args.policy.resolve()), 'GAZE_BENCH_AUDIT_ROWS': '1'}
    responses = {}
    for side in ('base', 'candidate'):
        binary = getattr(args, side).resolve()
        rows = {}
        with BenchSubprocess([str(binary), '--config', 'policy-file'], cwd=ROOT, env=env) as process:
            for i, ((_, document), req) in enumerate(zip(documents, requests), 1):
                observed = process.exchange(req)
                audit = observed.pop('audit_rows', None)
                response = score.validate_response(document, observed)
                if audit is not None:
                    response['audit_rows'] = audit
                rows[document.uid] = response
                print(f'{side} equivalence {i}/{len(documents)}', flush=True)
        responses[side] = rows
        # Capture once. Subsequent contract evaluations replay these observations only.
        write(out / f'{side}-responses.json', rows)
    for i, (_, document) in enumerate(documents):
        if without_timing(responses['base'][document.uid]) != without_timing(responses['candidate'][document.uid]):
            raise AssertionError(f'output/audit/manifest/candidates differ at document {i + 1}')
    contract_reports = {}
    for version in (1, 2):
        contract = None if version == 1 else score.load_scored_label_contract(
            ROOT / 'docs/reference/benchmarks/scored-labels-v2.json')
        cards = {}
        for side in ('base', 'candidate'):
            layers = {}
            for layer in 'ACDR':
                docs = [d for l, d in documents if l == layer]
                effective_contract = contract if layer == 'C' else score.load_scored_label_contract(
                    ROOT / 'docs/reference/benchmarks/scored-labels-agentic.json')
                if effective_contract is not None:
                    docs = score.apply_scored_label_contract(docs, effective_contract)
                metrics, integrity = score.MetricAccumulator(), score.ContractAccumulator()
                refusals = 0
                for d in docs:
                    response = responses[side][d.uid]
                    if 'pipeline_error_code' in response:
                        refusals += 1
                        continue
                    metrics.add(d, score.final_trace_predictions(d, response))
                    integrity.add(response)
                layers[layer] = {'metrics': metrics.result(), 'contract': integrity.result(), 'refusals': refusals}
            cards[side] = layers
            write(out / f'{side}-v{version}.json', layers)
        if cards['base'] != cards['candidate']:
            raise AssertionError(f'bounded replay scorecards differ under v{version}')
        contract_reports[f'v{version}'] = {'identical': True, 'layers': cards['candidate']}
    verifier_path = out / 'window-inputs.jsonl'
    window_inputs = []
    for _, document in documents:
        response = responses['candidate'][document.uid]
        if 'pipeline_error_code' not in response:
            window_inputs.append(document.text)
            window_inputs.extend(scan_views(response))
    window_inputs.extend(dict.fromkeys(text for _, blocks in WORKLOADS for text in blocks))
    verifier_path.write_text(''.join(json.dumps({'text': text}, ensure_ascii=False) + '\n'
                                    for text in window_inputs))
    with verifier_path.open() as stream, (out / 'raw-windows.jsonl').open('w') as result:
        subprocess.run([str(args.verifier.resolve())], stdin=stream, stdout=result, env=env,
                       cwd=ROOT, timeout=1800, check=True)
    raw = json.loads((out / 'raw-windows.jsonl').read_text().splitlines()[-1])
    report = {'scope': 'bounded slice, not main certification or full-corpus gate',
              'documents': len(documents), 'layers': dict(Counter(l for l, _ in documents)),
              'base_sha256': sha(args.base), 'candidate_sha256': sha(args.candidate),
              'policy_sha256': sha(args.policy), 'requests_sha256': sha(corpus_path),
              'compared': 'exact fixed-session clean bytes, manifest spans, audit rows, candidate events, '
                          'protection traces, safety-net statistics and strict restores; only timing excluded',
              'raw_nym_probability_bits': raw, 'window_inputs_sha256': sha(verifier_path),
              'contracts': contract_reports}
    if report['base_sha256'] == report['candidate_sha256']:
        raise AssertionError('old/new executable hashes must differ')
    write(out / 'equivalence.json', report)


class Daemon:
    def __init__(self, binary, policy):
        env = {k: v for k, v in os.environ.items() if k in ('PATH', 'HOME', 'TMPDIR', 'LANG', 'LC_ALL')}
        env['GAZE_PERF'] = '1'
        self.process = subprocess.Popen([str(binary.resolve()), 'daemon', '--policy', str(policy.resolve())],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        text=True, env=env)
        self.responses, self.traces = queue.Queue(), queue.Queue()
        def read(stream, target, prefix=''):
            for line in stream:
                if not prefix or line.startswith(prefix):
                    target.put(json.loads(line[len(prefix):]))
        for stream, target, prefix in ((self.process.stdout, self.responses, ''),
                                       (self.process.stderr, self.traces, 'GAZE_PERF ')):
            threading.Thread(target=read, args=(stream, target, prefix), daemon=True).start()
        self.startup = self.traces.get(timeout=60)
    def req(self, op, text=None):
        req = {'version': 1, 'op': op, 'session_id': 'diagnostic'}
        if text is not None:
            req['text'] = text
        start = time.perf_counter()
        self.process.stdin.write(json.dumps(req) + '\n')
        self.process.stdin.flush()
        response = self.responses.get(timeout=120)
        elapsed = (time.perf_counter() - start) * 1000
        trace = self.traces.get(timeout=5)
        if 'error' in response:
            raise AssertionError('daemon rejected a diagnostic request')
        return response, elapsed, trace
    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        if self.process.returncode:
            raise AssertionError('daemon exited unsuccessfully')


def diagnostic(args):
    args.output.mkdir(parents=True, exist_ok=True)
    rows, reference = [], {}
    def save():
        summary = {}
        for name, _ in WORKLOADS:
            summary[name] = {}
            for side in ('base', 'candidate'):
                times = [r['ms'] for r in rows if r['side'] == side and r['case'] == name]
                if times:
                    summary[name][side] = {'n': len(times), 'median_ms': statistics.median(times),
                                          'min_ms': min(times), 'max_ms': max(times)}
        write(args.output / 'diagnostic.json', {'diagnostic': True, 'publishable': False,
              'bench_machine_lease': False, 'base_sha256': sha(args.base), 'candidate_sha256': sha(args.candidate),
              'policy_sha256': sha(args.policy), 'rows': rows, 'summary': summary})
    def gate():
        while os.getloadavg()[0] >= 12:
            print(f'waiting: load1={os.getloadavg()[0]:.2f}', flush=True)
            time.sleep(10)
    for run in range(args.runs):
        sides = ('base', 'candidate') if run % 2 == 0 else ('candidate', 'base')
        for side in sides:
            gate()
            daemon = Daemon(getattr(args, side), args.policy)
            try:
                daemon.req('open')
                canary = 'Canary alice@example.invalid and Dr. Schmidt'
                response, _, _ = daemon.req('clean', canary)
                restored, _, _ = daemon.req('restore', response['clean_text'])
                assert restored['text'] == canary
                daemon.req('close')
                daemon.req('open')
                for name, blocks in WORKLOADS:
                    gate()
                    load = os.getloadavg()[0]
                    times, traces, protected = [], [], []
                    for text in blocks:
                        response, ms, trace = daemon.req('clean', text)
                        times.append(ms)
                        traces.append(trace)
                        protected.append(response)
                    for text, response in zip(blocks, protected):
                        restored, _, _ = daemon.req('restore', response['clean_text'])
                        if restored['text'] != text:
                            raise AssertionError('diagnostic restore mismatch')
                    normalized = [canonical(r, daemon=True).hex() for r in protected]
                    if name in reference and reference[name] != normalized:
                        raise AssertionError('diagnostic protection output differs')
                    reference[name] = normalized
                    rows.append({'run': run, 'side': side, 'case': name, 'load1': load,
                                 'ms': sum(times), 'block_ms': times, 'perf': traces, 'restore_exact': True})
                    save()
                    print(f'{run} {side} {name}: {sum(times):.2f}ms load1={load:.2f}', flush=True)
            finally:
                daemon.close()
    save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('equivalence', 'diagnostic'))
    for name in ('base', 'candidate', 'policy', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--dataset', type=Path)
    parser.add_argument('--verifier', type=Path)
    parser.add_argument('--per-layer', type=int, default=24)
    parser.add_argument('--runs', type=int, default=5)
    args = parser.parse_args()
    if args.mode == 'equivalence':
        if args.dataset is None or args.verifier is None or args.per_layer < 2 or args.per_layer % 2:
            parser.error('equivalence needs --dataset, --verifier and a positive even --per-layer')
        equivalence(args)
    else:
        if args.runs < 5:
            parser.error('diagnostic requires at least five repetitions per side')
        diagnostic(args)


if __name__ == '__main__':
    main()
