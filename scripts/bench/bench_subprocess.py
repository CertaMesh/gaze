"""Bounded POSIX JSONL exchange with trusted benchmark producers.

Pipes are private memory, never diagnostic output. This is not a child sandbox.
"""
from __future__ import annotations

import functools
import json
import math
import os
import selectors
import subprocess
import sys
import time
from dataclasses import dataclass


CODES = frozenset({
    "unsupported_platform", "invalid_limits", "invalid_state", "input_limit",
    "output_limit", "stderr_limit", "deadline", "protocol", "producer_exit",
    "io", "payload_processing", "cleanup", "cancelled",
})
PHASES = frozenset({"start", "handshake", "exchange", "finish", "cleanup", "payload"})


class ProducerFailure(RuntimeError):
    """Only closed codes may cross the producer boundary."""

    def __init__(self, code: str, phase: str = "payload") -> None:
        self.code = code if code in CODES else "payload_processing"
        self.phase = phase if phase in PHASES else "payload"
        super().__init__(f"{self.code}:{self.phase}")


def _owner_phase(args, default):
    """A bound transport method knows its live phase; a module-level call does not."""
    try:
        phase = getattr(args[0], "phase", None) if args else None
        return phase if type(phase) is str and phase in PHASES else default
    except BaseException:
        # Error reporting must not expose a second diagnostic from an accessor.
        return default


def _raise_closed(code, phase):
    """Raise a closed failure with no chain, even inside a live caller handler.

    Clearing __context__ before the raise does not survive: the raise statement
    re-attaches the caller's active exception. Only a bare re-raise skips that.
    """
    try:
        raise ProducerFailure(code, phase) from None
    except ProducerFailure as closed:
        closed.__cause__ = None
        closed.__context__ = None
        closed.__suppress_context__ = True
        raise


def producer_boundary(function):
    """Drop diagnostic exceptions, including their implicit chained context."""
    @functools.wraps(function)
    def wrapped(*args, **kwargs):
        code, phase = "payload_processing", "payload"
        try:
            return function(*args, **kwargs)
        except ProducerFailure as error:
            code, phase = error.code, error.phase
        except Exception:
            phase = _owner_phase(args, phase)
        except BaseException:
            # Cancellation stays a closed outcome by contract: the raw signal
            # exception can carry payload, and every caller must still abort.
            code, phase = "cancelled", _owner_phase(args, phase)
        # Raising inside except would retain the original private exception.
        _raise_closed(code, phase)
    return wrapped


@dataclass(frozen=True)
class TransportLimits:
    request_bytes: int = 16 * 1024 * 1024
    stdout_bytes: int = 64 * 1024 * 1024
    stderr_bytes: int = 64 * 1024 * 1024
    nesting: int = 64
    chunk_bytes: int = 64 * 1024
    handshake_seconds: float = 120
    exchange_seconds: float = 300
    invocation_seconds: float = 21600
    finish_seconds: float = 30
    terminate_seconds: float = 2
    reap_seconds: float = 3

    def validate(self) -> None:
        for name in ("request_bytes", "stdout_bytes", "stderr_bytes", "nesting", "chunk_bytes"):
            value = getattr(self, name)
            if type(value) is not int or value <= 0:
                raise ProducerFailure("invalid_limits", "start")
        for name in ("handshake_seconds", "exchange_seconds", "invocation_seconds",
                     "finish_seconds", "terminate_seconds", "reap_seconds"):
            value = getattr(self, name)
            if type(value) not in (int, float) or not math.isfinite(value) or value <= 0:
                raise ProducerFailure("invalid_limits", "start")
        if self.nesting > 64 or self.chunk_bytes > min(self.request_bytes, self.stdout_bytes, self.stderr_bytes):
            raise ProducerFailure("invalid_limits", "start")
        if max(self.handshake_seconds, self.exchange_seconds, self.finish_seconds) > self.invocation_seconds:
            raise ProducerFailure("invalid_limits", "start")
        # terminate_seconds/reap_seconds stay outside that comparison on purpose:
        # cleanup keeps its own finite budget once the invocation budget expires.


class BenchSubprocess:
    """One direct child, no inherited diagnostic sink or persistent I/O threads."""

    def __init__(self, command, *, cwd=None, env=None, limits=None):
        self.limits = limits if limits is not None else TransportLimits()
        self.command, self.cwd, self.env = command, cwd, env
        self.process = None
        self.selector = None
        self.invocation_deadline = 0.0
        self.stderr_seen = 0
        self.phase = "start"
        self.finished = False
        self.message_deadline = 0.0

    def _check(self, deadline):
        if time.monotonic() >= deadline:
            raise ProducerFailure("deadline", self.phase)

    def check_deadline(self):
        self._check(self.invocation_deadline)

    @producer_boundary
    def __enter__(self):
        if os.name != "posix" or sys.platform not in ("darwin", "linux"):
            raise ProducerFailure("unsupported_platform", "start")
        self.limits.validate()
        if self.process is not None:
            raise ProducerFailure("invalid_state", "start")
        self.invocation_deadline = time.monotonic() + self.limits.invocation_seconds
        try:
            self.process = subprocess.Popen(
                self.command, cwd=self.cwd, env=self.env, stdin=subprocess.PIPE,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0,
            )
            self.selector = selectors.DefaultSelector()
            for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
                os.set_blocking(stream.fileno(), False)
            self.selector.register(self.process.stdout, selectors.EVENT_READ, "stdout")
            self.selector.register(self.process.stderr, selectors.EVENT_READ, "stderr")
            self.check_deadline()
            return self
        except BaseException:
            self._cleanup()
            raise

    def _unregister(self, stream):
        if self.selector is None or stream is None:
            return
        try:
            self.selector.unregister(stream)
        except (KeyError, ValueError):
            pass

    def _close_stream(self, stream):
        self._unregister(stream)
        if stream is not None:
            stream.close()

    def _read(self, key):
        try:
            chunk = os.read(key.fd, self.limits.chunk_bytes)
        except BlockingIOError:
            return None
        if not chunk:
            self._close_stream(key.fileobj)
        return chunk

    def _stderr(self, chunk):
        self.stderr_seen += len(chunk)
        if self.stderr_seen > self.limits.stderr_bytes:
            raise ProducerFailure("stderr_limit", self.phase)

    def _idle_output(self):
        # Bytes already waiting before a request cannot be its response.
        for key, _ in self.selector.select(0):
            chunk = self._read(key)
            if chunk and key.data == "stdout":
                raise ProducerFailure("protocol", self.phase)
            if chunk and key.data == "stderr":
                self._stderr(chunk)

    def _encode(self, value, deadline):
        output = bytearray()
        active = set()

        def put(data):
            self._check(deadline)
            if len(output) + len(data) > self.limits.request_bytes:
                raise ProducerFailure("input_limit", self.phase)
            output.extend(data)

        def string(text):
            put(b'"')
            # JSONEncoder.iterencode emits a whole string at once. Slice first.
            for offset in range(0, len(text), min(4096, self.limits.chunk_bytes)):
                piece = text[offset:offset + min(4096, self.limits.chunk_bytes)]
                put(json.dumps(piece, ensure_ascii=False)[1:-1].encode("utf-8"))
            put(b'"')

        def visit(node, depth):
            self._check(deadline)
            if depth > self.limits.nesting:
                raise ProducerFailure("input_limit", self.phase)
            kind = type(node)
            if kind is str:
                string(node)
            elif node is None:
                put(b"null")
            elif kind is bool:
                put(b"true" if node else b"false")
            elif kind in (int, float):
                if kind is int and node.bit_length() > self.limits.request_bytes * 4:
                    raise ProducerFailure("input_limit", self.phase)
                put(json.dumps(node, allow_nan=False).encode("ascii"))
            elif kind in (dict, list, tuple):
                if id(node) in active:
                    raise ProducerFailure("input_limit", self.phase)
                active.add(id(node))
                mapping = kind is dict
                put(b"{" if mapping else b"[")
                for index, item in enumerate(node):
                    if index:
                        put(b",")
                    if mapping:
                        if type(item) is not str:
                            raise ProducerFailure("protocol", self.phase)
                        string(item)
                        put(b":")
                        visit(node[item], depth + 1)
                    else:
                        visit(item, depth + 1)
                put(b"}" if mapping else b"]")
                active.remove(id(node))
            else:
                raise ProducerFailure("protocol", self.phase)

        if type(value) is not dict:
            raise ProducerFailure("protocol", self.phase)
        visit(value, 1)
        put(b"\n")
        return output

    def _decode(self, frame, deadline):
        depth, quoted, escaped = 0, False, False
        for index, char in enumerate(frame):
            if index % self.limits.chunk_bytes == 0:
                self._check(deadline)
            if quoted:
                if escaped:
                    escaped = False
                elif char == 92:
                    escaped = True
                elif char == 34:
                    quoted = False
            elif char == 34:
                quoted = True
            elif char in (91, 123):
                depth += 1
                if depth > self.limits.nesting:
                    raise ProducerFailure("output_limit", self.phase)
            elif char in (93, 125):
                depth -= 1

        def pairs(items):
            result = {}
            for key, value in items:
                if key in result:
                    raise ProducerFailure("protocol", self.phase)
                result[key] = value
            return result

        def constant(_value):
            raise ProducerFailure("protocol", self.phase)

        result = json.loads(frame.decode("utf-8"), object_pairs_hook=pairs, parse_constant=constant)
        if type(result) is not dict:
            raise ProducerFailure("protocol", self.phase)
        # JSON exponent overflow is not covered by parse_constant.
        def finite(node):
            self._check(deadline)
            if type(node) is float and not math.isfinite(node):
                raise ProducerFailure("protocol", self.phase)
            if type(node) in (dict, list):
                for value in (node.values() if type(node) is dict else node):
                    finite(value)
        finite(result)
        self._check(deadline)
        return result

    def _receive(self, deadline, request=None):
        frame = bytearray()
        offset = 0
        if request is not None:
            self.selector.register(self.process.stdin, selectors.EVENT_WRITE, "stdin")
        try:
            while True:
                self._check(deadline)
                events = self.selector.select(min(0.05, max(0, deadline - time.monotonic())))
                # Snapshot before the batch: a response cannot predate its request,
                # whichever order this batch happens to report stdin and stdout in.
                pending = request is not None and offset != len(request)
                complete = False
                for key, _ in events:
                    self._check(deadline)
                    if key.data == "stdin":
                        try:
                            written = os.write(key.fd, memoryview(request)[offset:offset + self.limits.chunk_bytes])
                        except BlockingIOError:
                            continue
                        offset += written
                        if offset == len(request):
                            self._unregister(self.process.stdin)
                        continue
                    chunk = self._read(key)
                    if chunk is None:
                        continue
                    if key.data == "stderr":
                        self._stderr(chunk)
                        continue
                    if not chunk or pending:
                        raise ProducerFailure("protocol", self.phase)
                    if len(frame) + len(chunk) > self.limits.stdout_bytes:
                        raise ProducerFailure("output_limit", self.phase)
                    # Scan only the new bytes; every earlier chunk was newline-free.
                    newline = chunk.find(b"\n")
                    frame.extend(chunk)
                    if newline >= 0:
                        if newline != len(chunk) - 1:
                            raise ProducerFailure("protocol", self.phase)
                        complete = True
                if complete:
                    return self._decode(frame, deadline)
        finally:
            if request is not None:
                # Never strand a write registration, and never mask the first failure.
                self._unregister(self.process.stdin)

    @producer_boundary
    def receive_handshake(self):
        self.phase = "handshake"
        deadline = min(self.invocation_deadline, time.monotonic() + self.limits.handshake_seconds)
        self.message_deadline = deadline
        return self._receive(deadline)

    @producer_boundary
    def exchange(self, request):
        if self.finished or self.process is None or self.process.stdin.closed:
            raise ProducerFailure("invalid_state", "exchange")
        self.phase = "exchange"
        deadline = min(self.invocation_deadline, time.monotonic() + self.limits.exchange_seconds)
        self.message_deadline = deadline
        self._idle_output()
        encoded = self._encode(request, deadline)
        return self._receive(deadline, encoded)

    def check_message_deadline(self):
        self._check(self.message_deadline)

    def _wait_exit(self, deadline, *, discard):
        while True:
            if time.monotonic() >= deadline:
                return False
            for key, _ in self.selector.select(min(0.05, max(0, deadline - time.monotonic()))):
                chunk = self._read(key)
                if chunk and not discard:
                    if key.data == "stdout":
                        raise ProducerFailure("protocol", self.phase)
                    self._stderr(chunk)
            if self.process.poll() is not None:
                # Drain currently available data, never wait on inherited writers.
                while True:
                    if time.monotonic() >= deadline:
                        return False
                    ready = self.selector.select(0)
                    if not ready:
                        return True
                    for key, _ in ready:
                        chunk = self._read(key)
                        if chunk and not discard:
                            if key.data == "stdout":
                                raise ProducerFailure("protocol", self.phase)
                            self._stderr(chunk)

    @producer_boundary
    def finish(self):
        self.phase = "finish"
        self._close_stream(self.process.stdin)
        deadline = min(self.invocation_deadline, time.monotonic() + self.limits.finish_seconds)
        if not self._wait_exit(deadline, discard=False):
            raise ProducerFailure("deadline", "finish")
        if self.process.returncode != 0:
            raise ProducerFailure("producer_exit", "finish")
        self.finished = True

    def _cleanup(self):
        clean = True
        if self.process is None:
            return clean
        try:
            self._close_stream(self.process.stdin)
            if self.process.poll() is None:
                self.process.terminate()
                if self.selector is None:
                    self.selector = selectors.DefaultSelector()
                self._wait_exit(time.monotonic() + self.limits.terminate_seconds, discard=True)
        except Exception:
            clean = False
        finally:
            # A drain/close failure must not skip the final kill/reap attempt.
            try:
                if self.process.poll() is None:
                    self.process.kill()
                    self.process.wait(timeout=self.limits.reap_seconds)
            except Exception:
                clean = False
            for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
                try:
                    self._close_stream(stream)
                except Exception:
                    clean = False
            if self.selector is not None:
                self.selector.close()
        return clean

    def __exit__(self, kind, error, traceback):
        failure = None
        try:
            if kind is None and not self.finished:
                self.finish()
        except ProducerFailure as caught:
            failure = (caught.code, caught.phase)
        finally:
            clean = self._cleanup()
        if not clean:
            _raise_closed("cleanup", "cleanup")
        if failure is not None:
            _raise_closed(*failure)
        return False
