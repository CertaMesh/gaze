#!/usr/bin/env python3
"""Synthetic, untimed CLI protection proof across feature-off and timer builds."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import queue
import re
import subprocess
import tempfile
import threading

import perf_attribution as perf

FIXTURES = [
    "alice@example.invalid",
    "To: alice@example.invalid\nCc: alice@example.invalid",
    "Unicode: Grüße — alice@example.invalid",
    '{"email":"alice@example.invalid"}',
    "No contact details here.",
    "Phone +1 202 555 0100; alice@example.invalid",
    "Card 4111 1111 1111 1111; alice@example.invalid",
]


def canonical(response: dict, *, daemon: bool) -> bytes:
    # Only the random eight-hex session prefix in ordinary tokens is normalized.
    # Class, token index/spelling, clean/raw spans, mapping values and disposition stay exact.
    entries = response["tokens" if daemon else "entries"]
    replacements = {e["token"]: re.sub(r'^<([0-9a-f]{8}):', '<00000000:', e["token"]) for e in entries}
    text = response["clean_text"]
    if replacements:
        pattern = re.compile('|'.join(re.escape(t) for t in sorted(replacements, key=len, reverse=True)))
        text = pattern.sub(lambda m: replacements[m[0]], text)
    projection = {"clean_text": text, "entries": [{**e, "token": replacements[e["token"]]} for e in entries]}
    if daemon:
        projection["manifest"] = response["manifest"]
    else:
        projection["stats"] = response["stats"]
        projection["leak_report"] = response["leak_report"]
    return json.dumps(projection, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()


def one_shot(binary: Path, policy: Path, env: dict) -> list[bytes]:
    rows = []
    for text in FIXTURES:
        output = subprocess.run([str(binary), 'clean', '--policy', str(policy)], input=text,
                                capture_output=True, text=True, env=env, timeout=30, check=True)
        response = json.loads(output.stdout)
        restored = subprocess.run([str(binary), 'restore'], input=json.dumps({"session_blob": response["session_blob"], "text": response["clean_text"]}),
                                  capture_output=True, text=True, env=env, timeout=30, check=True)
        if json.loads(restored.stdout)['text'] != text:
            raise AssertionError('one-shot restore differs')
        traces = [json.loads(line[len(perf.PREFIX):]) for line in output.stderr.splitlines() if line.startswith(perf.PREFIX)]
        perf.validate_lifecycle(traces, 1, daemon=False, policy=policy, required=env['GAZE_PERF'] == '1')
        rows.append(canonical(response, daemon=False))
    return rows


def daemon(binary: Path, policy: Path, env: dict) -> list[bytes]:
    rows = []
    with tempfile.TemporaryFile(mode='w+') as errors:
        child = subprocess.Popen([str(binary), 'daemon', '--policy', str(policy)], stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=errors, text=True, env=env)
        responses = queue.Queue()
        def read():
            for line in child.stdout:
                responses.put(json.loads(line))
        thread = threading.Thread(target=read, daemon=True)
        thread.start()
        def exchange(request):
            child.stdin.write(json.dumps(request) + '\n')
            child.stdin.flush()
            result = responses.get(timeout=30)
            if result.get('error'):
                raise AssertionError('daemon refused synthetic request')
            return result
        try:
            for i, text in enumerate(FIXTURES):
                session = f'proof-{i}'
                response = exchange({'session_id': session, 'text': text})
                restored = exchange({'version': 1, 'op': 'restore', 'session_id': session, 'text': response['clean_text']})
                if restored['text'] != text:
                    raise AssertionError('daemon restore differs')
                rows.append(canonical(response, daemon=True))
            child.stdin.close()
            child.wait(timeout=30)
            thread.join(timeout=30)
            if child.returncode:
                raise AssertionError('daemon exit failed')
            errors.seek(0)
            traces = [json.loads(line[len(perf.PREFIX):]) for line in errors if line.startswith(perf.PREFIX)]
            perf.validate_lifecycle(traces, 2 * len(FIXTURES), daemon=True, policy=policy, required=env['GAZE_PERF'] == '1')
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
            child.stdin.close()
            child.stdout.close()
    return rows


def prove(plain: Path, timers: Path) -> dict:
    if perf.file_sha(plain) == perf.file_sha(timers):
        raise ValueError('separate feature-off and timer binaries required')
    # A feature-off executable must ignore GAZE_PERF=1 as well as its default.
    probe = subprocess.run([str(plain), 'clean', '--safety-net', 'none'], input=FIXTURES[0],
                           text=True, capture_output=True, timeout=30, check=True,
                           env=perf.child_environment())
    if any(line.startswith(perf.PREFIX) for line in probe.stderr.splitlines()):
        raise AssertionError('feature-off binary contains active instrumentation')
    modes = {}
    with tempfile.TemporaryDirectory() as tmp:
        policy = Path(tmp) / 'policy.toml'
        policy.write_text('schema_version="0.1.0"\n[session]\nscope="conversation"\n'
                          '[policy.rulepacks]\nbundled=["core"]\n[safety_net]\nbackend="none"\n'
                          '[[rule]]\nkind="default"\naction="tokenize"\n')
        for name, run in [('one-shot', one_shot), ('daemon', daemon)]:
            results = []
            for binary, enabled in [(plain, False), (timers, False), (timers, True)]:
                env = {**perf.child_environment(), 'GAZE_PERF': '1' if enabled else '0'}
                result = run(binary, policy, env)
                results.append(result)
            if not results[0] == results[1] == results[2]:
                raise AssertionError(f'{name} semantic output differs')
            modes[name] = {'fixtures': len(FIXTURES), 'exact_restores': len(FIXTURES) * 3,
                           'semantic_sha256': [perf.sha(row) for row in results[0]],
                           'identical': True}
    return {'schema': 1, 'timed': False, 'plain_binary_sha256': perf.file_sha(plain),
            'timer_binary_sha256': perf.file_sha(timers),
            'normalization': 'Only eight-hex session prefixes inside declared ordinary tokens become 00000000. Within-mode clean bytes, token spelling/index, mapping values, stats/disposition and daemon manifest spans are otherwise exact. The Rust fixed-snapshot test compares byte-for-byte without normalization.',
            'modes': modes}


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--plain', type=Path, required=True)
    p.add_argument('--timers', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    args = p.parse_args()
    args.out.write_text(json.dumps(prove(args.plain.resolve(), args.timers.resolve()), indent=2) + '\n')
