#!/usr/bin/env python3
"""Keep OpenAI Privacy Filter warm behind a private Unix-domain socket."""

from __future__ import annotations

import argparse
import json
import os
import signal
import socket
import stat
import sys
from pathlib import Path


MAX_REQUEST_BYTES = 2 * 1024 * 1024
MAX_RESPONSE_BYTES = 4 * 1024 * 1024
DEFAULT_TIMEOUT_SECONDS = 30.0


def receive_all(connection: socket.socket, limit: int) -> bytes:
    chunks: list[bytes] = []
    total = 0
    while True:
        chunk = connection.recv(64 * 1024)
        if not chunk:
            break
        total += len(chunk)
        if total > limit:
            raise ValueError("OPF daemon message exceeds the byte limit")
        chunks.append(chunk)
    return b"".join(chunks)


def validate_socket_parent(path: Path) -> None:
    if len(os.fsencode(path)) >= 100:
        raise ValueError("OPF daemon socket path must be shorter than 100 bytes")
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    path.parent.chmod(0o700)
    if path.exists() or path.is_symlink():
        mode = path.lstat().st_mode
        if not stat.S_ISSOCK(mode):
            raise RuntimeError(f"refusing to replace non-socket path: {path}")
        path.unlink()


def serve(
    socket_path: Path, checkpoint: Path, device: str, trace_offsets: bool
) -> int:
    from opf import OPF

    os.umask(0o077)
    validate_socket_parent(socket_path)
    redactor = OPF(model=checkpoint, device=device, output_mode="typed")
    redactor.redact("No private data.")

    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(str(socket_path))
    socket_path.chmod(0o600)
    server.listen(16)

    def stop(_signum: int, _frame: object) -> None:
        raise SystemExit(0)

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    print(f"OPF_DAEMON_READY {socket_path}", flush=True)
    request_index = 0
    try:
        while True:
            connection, _ = server.accept()
            with connection:
                try:
                    request = json.loads(receive_all(connection, MAX_REQUEST_BYTES))
                    text = request.get("text")
                    if not isinstance(text, str):
                        raise ValueError("OPF daemon request is missing text")
                    result = redactor.redact(text)
                    request_index += 1
                    if trace_offsets:
                        print(
                            json.dumps(
                                {
                                    "request": request_index,
                                    "characters": len(text),
                                    "utf8_bytes": len(text.encode("utf-8")),
                                    "spans": [
                                        [span.start, span.end, span.label]
                                        for span in result.detected_spans
                                    ],
                                },
                                separators=(",", ":"),
                            ),
                            file=sys.stderr,
                            flush=True,
                        )
                    response = result.to_json(indent=None).encode("utf-8")
                except Exception as error:
                    response = json.dumps(
                        {"error": type(error).__name__}, separators=(",", ":")
                    ).encode("utf-8")
                try:
                    connection.sendall(response)
                except (BrokenPipeError, ConnectionResetError):
                    continue
    finally:
        server.close()
        socket_path.unlink(missing_ok=True)


def client(socket_path: Path, timeout: float) -> int:
    text = sys.stdin.read()
    request = json.dumps({"text": text}, ensure_ascii=False).encode("utf-8")
    if len(request) > MAX_REQUEST_BYTES:
        raise ValueError("OPF daemon request exceeds the byte limit")
    connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    connection.settimeout(timeout)
    with connection:
        connection.connect(str(socket_path))
        connection.sendall(request)
        connection.shutdown(socket.SHUT_WR)
        response = receive_all(connection, MAX_RESPONSE_BYTES)
    sys.stdout.buffer.write(response)
    sys.stdout.buffer.write(b"\n")
    return 0


def parse_server_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("serve", nargs="?")
    parser.add_argument("--socket", required=True, type=Path)
    parser.add_argument("--checkpoint", required=True, type=Path)
    parser.add_argument("--device", choices=("cpu", "cuda"), default="cpu")
    parser.add_argument("--trace-offsets", action="store_true")
    return parser.parse_args()


def main() -> int:
    if len(sys.argv) > 1 and sys.argv[1] == "serve":
        args = parse_server_args()
        return serve(args.socket, args.checkpoint, args.device, args.trace_offsets)
    socket_value = os.environ.get("GAZE_OPF_DAEMON_SOCKET")
    if not socket_value:
        raise RuntimeError("GAZE_OPF_DAEMON_SOCKET is not set")
    timeout = float(
        os.environ.get("GAZE_OPF_DAEMON_TIMEOUT_SECONDS", DEFAULT_TIMEOUT_SECONDS)
    )
    return client(Path(socket_value), timeout)


if __name__ == "__main__":
    raise SystemExit(main())
