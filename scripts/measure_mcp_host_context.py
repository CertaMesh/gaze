#!/usr/bin/env python3
"""Measure release MCP stdio round trips, not detection benchmark scores.

CLI Absent uses gaze_read_text and its filesystem manifest store. The embedded
rmcp host uses a synthetic lookup and a no-I/O manifest store with the same core
pipeline and envelope. Its 32 record fields and 32 argument scalars are fixed;
context is host-owned process configuration, never a model request field.
Five fresh-process runs per path alternate embedded Absent/Present order. Each
run initializes MCP, checks the tool catalog, warms up, then measures sequential
validated tools/call round trips. Peak RSS includes startup and warmup and is
read from wait4 for exactly that server process. No raw responses are retained.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import select
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def percentile(values, quantile):
    return sorted(values)[max(0, math.ceil(len(values) * quantile) - 1)]


class Client:
    def __init__(self, command, cwd):
        self.child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      stderr=subprocess.DEVNULL, cwd=cwd,
                                      env=dict(os.environ, GAZE_DISABLE_OS_KEYSTORE="1"), bufsize=0)
        self.pending = b""
        self.next_id = 0

    def write(self, body):
        self.child.stdin.write(json.dumps(body, separators=(",", ":")).encode() + b"\n")

    def request(self, method, params):
        self.next_id += 1
        self.write({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params})
        deadline = time.monotonic() + 60
        while True:
            while b"\n" not in self.pending:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not select.select([self.child.stdout], [], [], remaining)[0]:
                    raise RuntimeError("MCP response timeout")
                block = os.read(self.child.stdout.fileno(), 65536)
                if not block:
                    raise RuntimeError("MCP server closed output")
                self.pending += block
            line, self.pending = self.pending.split(b"\n", 1)
            body = json.loads(line)
            if body.get("id") == self.next_id:
                if "error" in body:
                    raise RuntimeError("MCP JSON-RPC error")
                return body["result"]

    def close(self):
        self.child.stdin.close()
        deadline = time.monotonic() + 15
        while True:
            pid, status, usage = os.wait4(self.child.pid, os.WNOHANG)
            if pid:
                self.child.returncode = os.waitstatus_to_exitcode(status)
                if self.child.returncode:
                    raise RuntimeError("MCP server exited unsuccessfully")
                self.child.stdout.close()
                return {"peak_rss_bytes": usage.ru_maxrss * (1 if platform.system() == "Darwin" else 1024),
                        "server_cpu_seconds": usage.ru_utime + usage.ru_stime}
            if time.monotonic() >= deadline:
                self.child.kill()
                self.child.wait()
                raise RuntimeError("MCP server shutdown timeout")
            time.sleep(0.01)


def run(command, mode, calls, warmup, cwd):
    driver_start = time.process_time()
    load_at_start = os.getloadavg()
    client = Client(command, cwd)
    try:
        start = time.perf_counter_ns()
        client.request("initialize", {"protocolVersion": "2025-03-26", "capabilities": {},
                                      "clientInfo": {"name": "synthetic-measurement", "version": "1"}})
        client.write({"jsonrpc": "2.0", "method": "notifications/initialized"})
        tools = client.request("tools/list", {})["tools"]
        name = "gaze_read_text" if mode == "cli-absent" else "lookup"
        if name not in {tool["name"] for tool in tools}:
            raise RuntimeError("expected tool missing")
        startup_ms = (time.perf_counter_ns() - start) / 1e6
        arguments = ({"text": "Synthetic public text alpha beta."} if mode == "cli-absent" else
                     {f"field{i:02}": f"Public synthetic value {i:02}" for i in range(32)})
        latencies = []
        for index in range(calls + warmup):
            before = time.perf_counter_ns()
            result = client.request("tools/call", {"name": name, "arguments": arguments})
            elapsed = (time.perf_counter_ns() - before) / 1e6
            if result.get("isError") or not result.get("content"):
                raise RuntimeError("tool failed or returned no content")
            texts = [entry["text"] for entry in result["content"] if entry.get("type") == "text"]
            if not texts:
                raise RuntimeError("tool returned no text")
            value = json.loads(texts[0])
            if mode == "cli-absent":
                valid = "Synthetic public text alpha beta." in value["clean_markdown"]
            elif mode == "embedded-absent":
                valid = value["text"] == "found Qorvex Zunvela"
            else:
                valid = (value["text"].startswith("found <") and
                         "Qorvex" not in value["text"] and "Zunvela" not in value["text"])
            if not valid:
                raise RuntimeError(f"{mode}: response contract failed")
            if index >= warmup:
                latencies.append(elapsed)
        resource = client.close()
        return {"p50_ms": percentile(latencies, .5), "p95_ms": percentile(latencies, .95),
                "latencies_ms": latencies, "startup_ms": startup_ms,
                "validated_calls": calls + warmup, "errors": 0,
                "load_average_at_start": load_at_start,
                "driver_cpu_seconds": time.process_time() - driver_start, **resource}
    finally:
        if client.child.returncode is None:
            client.child.kill()
            client.child.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli-bin", type=Path, required=True)
    parser.add_argument("--host-bin", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--calls", type=int, default=50)
    parser.add_argument("--warmup", type=int, default=10)
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()
    if args.calls < 20 or args.runs < 5 or args.warmup < 1:
        parser.error("require at least 20 calls, five runs and one warmup")
    cli, host = args.cli_bin.resolve(strict=True), args.host_bin.resolve(strict=True)
    observations = {mode: [] for mode in ("cli-absent", "embedded-absent", "embedded-present")}
    with tempfile.TemporaryDirectory(prefix="gaze-mcp-measure-") as cwd:
        for repetition in range(args.runs):
            modes = ["cli-absent"] + (["embedded-absent", "embedded-present"] if repetition % 2 == 0
                                       else ["embedded-present", "embedded-absent"])
            for mode in modes:
                command = ([str(cli), "mcp", "serve", "--manifest-dir", str(Path(cwd) / f"manifest-{repetition}")]
                           if mode == "cli-absent" else [str(host), mode.removeprefix("embedded-")])
                observations[mode].append(run(command, mode, args.calls, args.warmup, cwd))
    summary = {}
    for mode, rows in observations.items():
        summary[mode] = {}
        for key in ("p50_ms", "p95_ms", "peak_rss_bytes", "server_cpu_seconds", "startup_ms"):
            values = [row[key] for row in rows]
            summary[mode][key] = {"median": statistics.median(values), "min": min(values), "max": max(values)}
        summary[mode]["validated_calls"] = sum(row["validated_calls"] for row in rows)
        summary[mode]["errors"] = sum(row["errors"] for row in rows)
    report = {"schema": "gaze-mcp-host-measurement-v1", "source_sha": subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "script_sha256": digest(Path(__file__)), "binary_sha256": {"cli": digest(cli), "host": digest(host)},
        "hardware": {"platform": platform.platform(), "machine": platform.machine(),
                     "cpu_count": os.cpu_count(), "load_average": os.getloadavg(),
                     "cpu": subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip(),
                     "memory_bytes": int(subprocess.check_output(["sysctl", "-n", "hw.memsize"], text=True))},
        "runs_per_path": args.runs, "measured_calls_per_run": args.calls, "warmup_calls_per_run": args.warmup,
        "concurrency": 1, "record_fields": 32, "argument_scalars_embedded": 32,
        "argument_utf8_bytes_embedded": sum(len(f"field{i:02}") + len(f"Public synthetic value {i:02}") for i in range(32)),
        "summary": summary, "runs": observations}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
