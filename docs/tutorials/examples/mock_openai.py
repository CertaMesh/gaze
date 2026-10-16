"""Local, non-streaming Chat Completions echo server for the proxy tutorial."""

from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path


class Echo(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/v1/chat/completions":
            self.send_error(404)
            return
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        Path("upstream-request.json").write_text(json.dumps(request))
        response = {
            "id": "mock-completion",
            "object": "chat.completion",
            "created": 0,
            "model": "mock-model",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": request["messages"][-1]["content"]},
                "finish_reason": "stop",
            }],
        }
        body = json.dumps(response).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass


if __name__ == "__main__":
    with HTTPServer(("127.0.0.1", 0), Echo) as server:
        host, port = server.server_address
        Path("mock-upstream-url.txt").write_text(f"http://{host}:{port}")
        server.serve_forever()
