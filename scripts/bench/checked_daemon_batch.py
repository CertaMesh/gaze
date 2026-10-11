"""Checked model-free daemon observations, using the bounded bench transport.

A refusal is a failed measurement. No response reaches a scorer until the
complete batch, its IDs, success shapes and child exit have been checked.
"""
from __future__ import annotations

import selectors
import os
import time

from bench_subprocess import BenchSubprocess, ProducerFailure, TransportLimits, producer_boundary


class _DaemonBatch(BenchSubprocess):
    @producer_boundary
    def collect(self, requests):
        self.phase = 'exchange'
        deadline = min(self.invocation_deadline, time.monotonic() + self.limits.exchange_seconds)
        encoded = bytearray()
        for request in requests:
            frame = self._encode(request, deadline)
            if len(encoded) + len(frame) > self.limits.request_bytes:
                raise ProducerFailure('input_limit', self.phase)
            encoded.extend(frame)
        offset, received = 0, 0
        pending = bytearray()
        responses = []
        if encoded:
            self.selector.register(self.process.stdin, selectors.EVENT_WRITE, 'stdin')
        else:
            self._close_stream(self.process.stdin)
        # Drain both output pipes to EOF; accepting the first N lines could miss
        # an extra response or a failure after an otherwise complete batch.
        while self.selector.get_map():
            self._check(deadline)
            for key, _ in self.selector.select(min(0.05, max(0, deadline - time.monotonic()))):
                self._check(deadline)
                if key.data == 'stdin':
                    try:
                        offset += os.write(key.fd, memoryview(encoded)[offset:offset + self.limits.chunk_bytes])
                    except BlockingIOError:
                        continue
                    if offset == len(encoded):
                        self._close_stream(self.process.stdin)
                    continue
                chunk = self._read(key)
                if not chunk:
                    continue
                if key.data == 'stderr':
                    self._stderr(chunk)
                    continue
                received += len(chunk)
                if received > self.limits.stdout_bytes:
                    raise ProducerFailure('output_limit', self.phase)
                pending.extend(chunk)
                while (end := pending.find(b'\n')) >= 0:
                    responses.append(self._decode(pending[:end], deadline))
                    del pending[:end + 1]
        if pending:
            raise ProducerFailure('protocol', self.phase)
        self.finish()
        return responses


def _span(value, length):
    return (type(value) is dict and type(value.get('start')) is int
            and type(value.get('end')) is int and 0 <= value['start'] < value['end'] <= length)


def _success(response, request):
    if 'error' in response:
        raise ProducerFailure('protocol', 'payload')
    clean, manifest = response.get('clean_text'), response.get('manifest')
    if type(clean) is not str or type(manifest) is not list:
        raise ProducerFailure('protocol', 'payload')
    raw_length, clean_length = len(request['text'].encode()), len(clean.encode())
    for entry in manifest:
        if type(entry) is not dict:
            raise ProducerFailure('protocol', 'payload')
        cls = entry.get('class')
        valid_class = (type(cls) is str and bool(cls)) or (
            type(cls) is dict and set(cls) == {'Custom'}
            and type(cls['Custom']) is str and bool(cls['Custom']))
        if not (valid_class and _span(entry.get('raw_span'), raw_length)
                and _span(entry.get('clean_span'), clean_length)):
            raise ProducerFailure('protocol', 'payload')


@producer_boundary
def run_daemon_batch(binary, policy, docs, *, limits=None):
    """Join exact session IDs and return only validated successes in input order."""
    requests = [{'session_id': f'doc-{i}', 'text': doc['text']} for i, doc in enumerate(docs)]
    limits = limits or TransportLimits(exchange_seconds=600)
    with _DaemonBatch([str(binary), 'daemon', '--policy', str(policy)], limits=limits) as child:
        responses = child.collect(requests)
    expected = {request['session_id']: request for request in requests}
    by_id = {}
    for response in responses:
        sid = response.get('session_id')
        if type(sid) is not str or sid not in expected or sid in by_id:
            raise ProducerFailure('protocol', 'payload')
        _success(response, expected[sid])
        by_id[sid] = response
    if set(by_id) != set(expected):
        raise ProducerFailure('protocol', 'payload')
    return [by_id[request['session_id']] for request in requests]
