#!/usr/bin/env python3
"""Exercise proxy routing controls against a real gaze setup policy and local upstream.

Run `gaze setup --non-interactive --force --policy-out target/model-policy.toml`
first, then pass --gaze and --policy. No provider credentials or network service needed.
Use --baseline to record pre-fix outcomes without requiring model aliases to pass.
"""

import argparse
import json
import socket
import subprocess
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


MODELS = (
    "claude-sonnet-4-5",
    "claude-sonnet-5",
    "claude-sonnet-4-6",
    "claude-sonnet-4-5-20250929",
    "claude-opus-4-1",
    "claude-haiku-4-5",
)
# Reserved domain, NANPA fictional phone range, published synthetic IBAN fixture.
REFUSED = (
    "alice@example.invalid",
    "202-555-0142",
    "DE89370400440532013000",
    "Please write to Anna Schneider",
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gaze", required=True)
    parser.add_argument("--policy", required=True)
    parser.add_argument("--baseline", action="store_true")
    args = parser.parse_args()
    policy = open(args.policy, encoding="utf-8").read()
    assert "[ner]" in policy and 'backend = "nym"' in policy, "needs full setup policy"
    forwarded = []

    class Upstream(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            forwarded.append((self.path, body))
            response = json.dumps({
                "id": "msg_test", "type": "message", "role": "assistant",
                "model": "safe-model", "content": [{"type": "text", "text": "ok"}],
                "stop_reason": "end_turn", "stop_sequence": None,
                "usage": {"input_tokens": 1, "output_tokens": 1},
            }).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(response)))
            self.end_headers()
            self.wfile.write(response)

    upstream = ThreadingHTTPServer(("127.0.0.1", 0), Upstream)
    thread = threading.Thread(target=upstream.serve_forever, daemon=True)
    thread.start()
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    upstream_url = f"http://127.0.0.1:{upstream.server_port}"
    command = [args.gaze, "proxy", "serve", "--bind", f"127.0.0.1:{port}",
               "--policy", args.policy]
    for provider in ("anthropic", "openai", "gemini"):
        command.extend([f"--upstream-{provider}", upstream_url])
    proxy = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        deadline = time.monotonic() + 120
        while True:
            assert proxy.poll() is None, proxy.stderr.read().decode()
            try:
                with urllib.request.urlopen(base + "/_gaze_proxy/healthz", timeout=1):
                    break
            except (OSError, urllib.error.URLError):
                assert time.monotonic() < deadline, "proxy startup timed out"
                time.sleep(0.05)

        for provider in ("anthropic", "openai", "gemini"):
            for model in MODELS + REFUSED:
                if provider == "anthropic":
                    path = "/v1/messages"
                    body = {"model": model, "max_tokens": 16,
                            "messages": [{"role": "user", "content": "hello"}]}
                elif provider == "openai":
                    path = "/v1/chat/completions"
                    body = {"model": model, "messages": [{"role": "user", "content": "hello"}]}
                else:
                    path = "/v1beta/models/" + urllib.parse.quote(model, safe="") + ":generateContent"
                    body = {"contents": [{"parts": [{"text": "hello"}]}]}
                before = len(forwarded)
                request = urllib.request.Request(base + path, json.dumps(body).encode(), headers={
                    "Content-Type": "application/json", "x-api-key": "synthetic-key",
                    "anthropic-version": "2023-06-01",
                })
                try:
                    response = urllib.request.urlopen(request, timeout=120)
                except urllib.error.HTTPError as error:
                    response = error
                with response:
                    status = response.status
                    result = response.read().decode()
                print(json.dumps({"provider": provider, "model": model, "status": status}), flush=True)
                if not args.baseline:
                    accepted = model in MODELS
                    assert status == (200 if accepted else 422), result
                    assert len(forwarded) == before + int(accepted)
                    if accepted:
                        path_sent, body_sent = forwarded[-1]
                        if provider == "gemini":
                            assert urllib.parse.unquote(path_sent) == urllib.parse.unquote(path)
                        else:
                            assert body_sent["model"] == model
    finally:
        proxy.terminate()
        try:
            _, stderr = proxy.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            proxy.kill()
            _, stderr = proxy.communicate()
        upstream.shutdown()
        upstream.server_close()
        assert b"no NER model" not in stderr, stderr.decode()


if __name__ == "__main__":
    main()
