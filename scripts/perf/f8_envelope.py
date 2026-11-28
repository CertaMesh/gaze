#!/usr/bin/env python3
"""F8 chat-turn performance envelope: measure a warm daemon against fixed budgets.

The workload is the one decided for gaze 1.0 (F8): a warm `gaze daemon`
session holding a 12-turn EN/DE conversation of about 32 KiB, plus a new user
message of up to 4 KiB that contains PII, under the policy `gaze setup` writes.
Restore is the daemon's source-byte strict restore.

Each sample opens a daemon protocol v2 `text-blocks-v1` session, primes it with
the 12 history blocks in one `clean_blocks` call, then times one or more new
turns. A turn sends the complete inventory: history blocks by hash only, the new
block with its text. This is how a resident integration such as Teyos sends a
conversation. After the timed turns the sample also times a full `resync`
(re-clean of every block) and a strict `restore` of the newest cleaned block,
which must reproduce its raw text exactly.

All text is synthetic. Names are invented, email addresses use
`example.invalid`, phone numbers use the fictional NANPA 555-0100..0199 and
BNetzA drama ranges, and the IBAN is the documented `DE89 3704 0044 0532 0130 00`
example.

Subcommands:

    measure  run the workload against one binary and write a receipt
    check    gate a receipt against docs/reference/benchmarks/f8-budgets.json
    compare  require two receipts to have identical output digests

Only envelopes with `release_gate: true` gate a release. The others still get a
published, measured receipt; `check` judges it the same way and says it does
not gate.

A receipt records every latency, RSS samples, host load, binary and policy
hashes, and one SHA-256 over the canonical cleaned outputs (manifests and token
metadata included) with random session token prefixes normalized. Equal digests
from a base and a candidate binary show byte-identical output on this workload.

`measure` writes the policy itself with the binary's own `gaze setup` and passes
no other daemon flags, so a receipt always describes the decided profile.

A timing claim needs the Solo `bench-machine` lease and a quiet host: load1
below 2.0 and no foreign build or benchmark process at every probe (start, every
few seconds during the run, end). `check` recomputes quietness from the probes
and fails a receipt that is not quiet, that was measured against other budgets,
another platform or extra daemon flags, that misses any budgeted measurement,
or that exceeds any budget. Budgets are fixed before measuring; never edit them to fit
a result.

Usage (from the repository root):

    python3 scripts/perf/f8_envelope.py measure --binary target/release/gaze \\
        --envelope laptop --output target/f8/laptop.json
    python3 scripts/perf/f8_envelope.py check target/f8/laptop.json
    python3 scripts/perf/f8_envelope.py compare base.json candidate.json
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import random
import queue
import signal
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BUDGETS = ROOT / "docs/reference/benchmarks/f8-budgets.json"
SCHEMA = "gaze-f8-envelope-v1"
QUIET_LOAD_1M = 2.0
FOREIGN = re.compile(
    r"cargo (build|test|run|clippy)|rustc |clean_for_bench|run_no_opf_benchmark|"
    r"privacy-filter/venv/bin/opf|opf_daemon|f8_envelope\.py measure|cargo (check|nextest|bench)|"
    r"gaze daemon"
)
# Session prefixes in bracket (`<hex:Class_1>`), bare (`hex:class_1`) and
# fake-email (`email1.hex@gaze-fake.invalid`) token forms.
TOKEN_PREFIX = re.compile(r"(?<![0-9A-Za-z])([0-9a-f]{8})(?=:|@gaze-fake\.invalid)")
PLATFORMS = {"laptop": ("Darwin", "arm64"), "server": ("Linux", "x86_64")}
PROBE_SECONDS = 5.0
EXCHANGE_TIMEOUT = 600.0
COLD_RUNS_MIN = 5
HISTORY_TURNS = 12
HISTORY_BYTES = 32 * 1024
NEW_MESSAGE_MAX = 4 * 1024
PROFILE = "text-blocks-v1"
PROJECTION = "f8-chat-v1"

# Synthetic, fictional values only (AGENTS.md rule 2).
FIRST = ["Mara", "Jonas", "Lena", "Tobias", "Ines", "Felix", "Greta", "Paul"]
LAST = ["Feldheim", "Quandt", "Ostermeyer", "Brandlow", "Kessler", "Wendrich", "Holmberg"]
STREETS = ["Lindenweg", "Am Kiesgrund", "Birkenallee", "Quellstraße"]
CITIES = [("10115", "Berlin"), ("80331", "München"), ("50667", "Köln"), ("20095", "Hamburg")]
IBAN = "DE89 3704 0044 0532 0130 00"

EN = [
    "Thanks for the summary, that matches what I saw in the logs yesterday.",
    "Can you rewrite the onboarding paragraph so it reads less like a contract?",
    "The release checklist still mentions the old staging cluster; please drop it.",
    "I compared both drafts and prefer the second one, mostly for the shorter intro.",
    "Please keep the tone friendly but direct, and avoid marketing phrases.",
    "We agreed to ship the migration on Tuesday after the backup finishes.",
    "The invoice export failed twice because the date column was empty.",
    "Could you list the open questions at the end so I can forward them?",
]
DE = [
    "Danke für die Zusammenfassung, das deckt sich mit meinen Notizen.",
    "Kannst du den Absatz zur Kündigungsfrist etwas klarer formulieren?",
    "Die Rechnung vom letzten Monat ist noch nicht im System verbucht.",
    "Bitte prüfe, ob der Termin am Donnerstag noch passt.",
    "Wir haben beschlossen, die alte Schnittstelle Ende des Quartals abzuschalten.",
    "Der Export enthält doppelte Zeilen, vermutlich wegen der Zeitzone.",
    "Schick mir bitte eine kurze Liste der offenen Punkte.",
    "Die Lieferung kam beschädigt an, ich habe Fotos gemacht.",
]


def person(rng: random.Random) -> str:
    return f"{rng.choice(FIRST)} {rng.choice(LAST)}"


def pii_sentence(rng: random.Random, german: bool) -> str:
    name = person(rng)
    local = name.lower().replace(" ", ".")
    zipcode, city = rng.choice(CITIES)
    street = f"{rng.choice(STREETS)} {rng.randint(1, 98)}"
    phone_us = f"(415) 555-01{rng.randint(0, 99):02d}"
    phone_de = f"+49 171 39200{rng.randint(0, 99):02d}"
    choices = [
        f"Kontakt: {name}, {local}@example.invalid, Telefon {phone_de}." if german
        else f"Contact {name} at {local}@example.invalid or {phone_us}.",
        f"Bitte überweise den Betrag auf {IBAN}, Kontoinhaber {name}." if german
        else f"Please refund the amount to {IBAN}, account holder {name}.",
        f"Die neue Anschrift lautet {street}, {zipcode} {city}." if german
        else f"Ship it to {name}, {street}, {zipcode} {city}, Germany.",
    ]
    return rng.choice(choices)


def message(rng: random.Random, size: int, german: bool, pii: bool) -> str:
    pool = DE if german else EN
    parts = [pii_sentence(rng, german)] if pii else []
    while sum(len(p.encode()) + 1 for p in parts) < size:
        parts.append(pii_sentence(rng, german) if pii and rng.random() < 0.15 else rng.choice(pool))
    text = " ".join(parts)
    while len(text.encode()) > size and len(parts) > 1:
        parts.pop()
        text = " ".join(parts)
    return text


def conversation(seed: int, turns: int) -> tuple[list[str], list[str]]:
    """Return 12 history blocks of about 32 KiB and `turns` new messages of at most 4 KiB."""
    rng = random.Random(seed)
    per_block = HISTORY_BYTES // HISTORY_TURNS
    history = [message(rng, per_block, german=i % 2 == 1, pii=rng.random() < 0.5)
               for i in range(HISTORY_TURNS)]
    new = [message(rng, rng.randint(512, NEW_MESSAGE_MAX), german=rng.random() < 0.5, pii=True)
           for _ in range(turns)]
    return history, new


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def normalize(value, prefixes: dict[str, str]):
    """Replace random session token prefixes with per-sample ordinals."""
    if isinstance(value, str):
        return TOKEN_PREFIX.sub(lambda m: "S%s" % prefixes.setdefault(m.group(1), str(len(prefixes))), value)
    if isinstance(value, list):
        return [normalize(item, prefixes) for item in value]
    if isinstance(value, dict):
        return {key: normalize(item, prefixes) for key, item in value.items()}
    return value


def percentile(values: list[float], q: float) -> float | None:
    """Nearest-rank percentile; None when there are too few samples to name it."""
    # Rounding keeps float error from turning 1 / 0.05 into 21 required samples.
    if len(values) < math.ceil(round(1 / (1 - q), 9)):
        return None
    ordered = sorted(values)
    return ordered[max(0, math.ceil(round(q * len(ordered), 9)) - 1)]


def rss_bytes(pid: int) -> int | None:
    status = Path(f"/proc/{pid}/status")
    if status.exists():
        for line in status.read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1]) * 1024
        return None
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    return int(out) * 1024 if out.isdigit() else None


def foreign_processes() -> list[str]:
    """Matching processes outside this harness's own ancestors and descendants."""
    out = subprocess.run(["ps", "-axo", "pid=,ppid=,command="], capture_output=True, text=True).stdout
    table = []
    for line in out.splitlines():
        parts = line.split(None, 2)
        if len(parts) == 3 and parts[0].isdigit() and parts[1].isdigit():
            table.append((int(parts[0]), int(parts[1]), parts[2]))
    parent = {pid: ppid for pid, ppid, _ in table}
    mine, cursor = set(), os.getpid()
    while cursor and cursor not in mine:
        mine.add(cursor)
        cursor = parent.get(cursor, 0)
    grew = True
    while grew:
        grew = False
        for pid, ppid, _ in table:
            if ppid in mine and pid not in mine and ppid != 1:
                mine.add(pid)
                grew = True
    found = {FOREIGN.search(cmd).group(0) for pid, _, cmd in table if pid not in mine and FOREIGN.search(cmd)}
    return sorted(found)


def host_probe() -> dict:
    load = os.getloadavg()
    foreign = foreign_processes()
    return {"load": [round(x, 2) for x in load], "foreign": foreign,
            "quiet": load[0] < QUIET_LOAD_1M and not foreign}


class Daemon:
    def __init__(self, binary: Path, policy: Path, stderr: Path, probes: list | None = None):
        self._stderr = stderr.open("ab")
        self.proc = subprocess.Popen(
            [str(binary), "daemon", "--policy", str(policy)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self._stderr,
            env={k: v for k, v in os.environ.items() if not k.startswith("GAZE_PERF")},
        )
        self.peak = 0
        self.samples: list[int] = []
        self.probes = probes
        # A reader thread owns stdout, so buffered replies are never stranded
        # behind a select() on an already drained descriptor.
        self._lines: queue.Queue[bytes] = queue.Queue()
        self._reader = threading.Thread(target=self._read, daemon=True)
        self._reader.start()
        self._stop = threading.Event()
        self._poller = threading.Thread(target=self._poll, daemon=True)
        self._poller.start()

    def _read(self):
        for line in self.proc.stdout:
            self._lines.put(line)
        self._lines.put(b"")

    def _poll(self):
        # The kernel's ru_maxrss at exit gives the true peak; polling only adds
        # host probes and a coarse running peak, so it stays cheap.
        while not self._stop.is_set() and self.proc.returncode is None:
            rss = rss_bytes(self.proc.pid)
            if rss:
                self.peak = max(self.peak, rss)
            if self.probes is not None:
                self.probes.append(host_probe())
            self._stop.wait(PROBE_SECONDS)

    def rss(self) -> int | None:
        rss = rss_bytes(self.proc.pid)
        if rss:
            self.samples.append(rss)
            self.peak = max(self.peak, rss)
        return rss

    def exchange(self, requests: list[dict]) -> list[tuple[dict, float]]:
        """Write every frame at once (concurrent sessions), then time each response."""
        start = time.perf_counter()
        self.proc.stdin.write(b"".join(json.dumps(r, separators=(",", ":")).encode() + b"\n" for r in requests))
        self.proc.stdin.flush()
        out = []
        deadline = start + EXCHANGE_TIMEOUT
        for _ in requests:
            try:
                line = self._lines.get(timeout=max(0.0, deadline - time.perf_counter()))
            except queue.Empty:
                raise RuntimeError("daemon did not respond within the exchange timeout") from None
            if not line:
                raise RuntimeError("daemon exited before responding")
            out.append((json.loads(line), time.perf_counter() - start))
        return out

    def close(self):
        """Stop the daemon and fold the kernel's peak RSS into `peak`."""
        self._stop.set()
        self._poller.join()
        try:
            self.proc.stdin.close()
        except OSError:
            pass
        try:
            deadline = time.monotonic() + 30
            while True:
                pid, status, usage = os.wait4(self.proc.pid, os.WNOHANG)
                if pid:
                    break
                if time.monotonic() > deadline:
                    os.kill(self.proc.pid, signal.SIGKILL)
                    pid, status, usage = os.wait4(self.proc.pid, 0)
                    break
                time.sleep(0.05)
            self.proc.returncode = os.waitstatus_to_exitcode(status)
            # ru_maxrss is bytes on macOS and KiB on Linux.
            maxrss = usage.ru_maxrss if platform.system() == "Darwin" else usage.ru_maxrss * 1024
            self.peak = max(self.peak, maxrss)
        except ChildProcessError:
            # Already reaped elsewhere: the kernel peak is lost, so report none.
            self.peak = 0
        finally:
            self._reader.join(timeout=5)
            # A reader still blocked in read() holds the buffer lock; closing
            # would wait for it forever.
            if not self._reader.is_alive():
                self.proc.stdout.close()
            self._stderr.close()


class Session:
    def __init__(self, sid: str):
        self.sid = sid
        self.state: dict = {}
        self.retained: dict[str, str] = {}
        self.blocks: list[str] = []

    def open(self) -> dict:
        return {"version": 2, "op": "open", "request_id": "open", "session_id": self.sid,
                "cache": "blocks", "framing_profile": PROFILE, "projection_id": PROJECTION}

    def _common(self, op: str, name: str) -> dict:
        s = self.state
        return {"version": 2, "op": op, "request_id": name, "request_seq": s["next_request_seq"],
                "session_id": self.sid, "daemon_epoch": s["daemon_epoch"], "session_epoch": s["session_epoch"],
                "pipeline_id": s["pipeline_id"], "framing_profile": PROFILE, "projection_id": PROJECTION}

    def clean(self, new: dict[str, str]) -> dict:
        request = self._common("clean_blocks", f"clean-{self.state['next_request_seq']}")
        request["expected_conversation_revision"] = self.state["conversation_revision"]
        request["expected_manifest_generation"] = self.state["manifest_generation"]
        self.blocks += [bid for bid in new if bid not in self.blocks]
        blocks = []
        for bid in self.blocks:
            text = new.get(bid, self.retained.get(bid))
            entry = {"id": bid, "sha256": sha256_text(text)}
            if bid in new:
                entry["text"] = text
            blocks.append(entry)
        self.retained.update(new)
        request["blocks"] = blocks
        return request

    def resync(self) -> dict:
        request = self._common("resync", f"resync-{self.state['next_request_seq']}")
        request["blocks"] = [{"id": b, "sha256": sha256_text(self.retained[b]), "text": self.retained[b]}
                             for b in self.blocks]
        return request

    def restore(self, text: str) -> dict:
        request = self._common("restore", f"restore-{self.state['next_request_seq']}")
        request["minimum_manifest_generation"] = self.state["manifest_generation"]
        request["text"] = text
        return request

    def accept(self, response: dict, op: str) -> dict:
        if response.get("op") != op or "error" in response:
            raise RuntimeError(f"{op} failed: {response.get('error')} {response.get('reason', '')}".strip())
        for key in ("daemon_epoch", "session_epoch", "pipeline_id", "conversation_revision",
                    "manifest_generation", "next_request_seq"):
            self.state[key] = response[key]
        if op in ("clean_blocks", "resync"):
            ids = [b["id"] for b in response["blocks"]]
            if ids != self.blocks:
                raise RuntimeError(f"{op} returned a different block inventory")
        return response


def digest_outputs(outputs: list) -> str:
    return hashlib.sha256(json.dumps(outputs, sort_keys=True, separators=(",", ":"),
                                     ensure_ascii=False).encode()).hexdigest()


def cold_start(args, policy: Path, stderr: Path) -> tuple[float, str]:
    """Spawn to first validated clean of a short PII message (page cache warm)."""
    start = time.perf_counter()
    daemon = Daemon(args.binary, policy, stderr)
    try:
        session = Session("cold")
        [(opened, _)] = daemon.exchange([session.open()])
        session.accept(opened, "open")
        [(cleaned, _)] = daemon.exchange([session.clean({"m0": message(random.Random(1), 256, False, True)})])
        session.accept(cleaned, "clean_blocks")
        elapsed = time.perf_counter() - start
        return elapsed, cleaned["pipeline_id"]
    finally:
        daemon.close()


def measure(args) -> dict:
    budgets = json.loads(BUDGETS.read_text())
    envelope = budgets["envelopes"][args.envelope]
    concurrency = args.concurrency or envelope["workload"]["concurrent_sessions"]
    start_probe = host_probe()
    if not args.allow_noisy and not start_probe["quiet"]:
        sys.exit(f"host not quiet at start: {start_probe}; pass --allow-noisy for a diagnostic run")
    if args.samples % concurrency:
        sys.exit("--samples must be a multiple of the concurrent session count")
    work = Path(tempfile.mkdtemp(prefix="gaze-f8-"))
    policy = work / "policy.toml"
    stderr = args.output.with_suffix(".daemon.log")
    stderr.parent.mkdir(parents=True, exist_ok=True)
    # The binary's own setup profile, exactly as `gaze setup` writes it.
    setup = subprocess.run([str(args.binary), "setup", "--non-interactive", "--force", "--policy-out", str(policy)],
                           capture_output=True, text=True)
    if setup.returncode:
        shutil.rmtree(work, ignore_errors=True)
        sys.exit(f"gaze setup failed ({setup.returncode}):\n{setup.stderr}")

    probes = [start_probe]
    cold = [cold_start(args, policy, stderr) for _ in range(args.cold_runs)]
    daemon = Daemon(args.binary, policy, stderr, probes)
    turn_ms, reclean_ms, restore_ms, outputs = [], [], [], []
    try:
        for group in range(args.samples // concurrency):
            sessions, convs = [], []
            for lane in range(concurrency):
                index = group * concurrency + lane
                sessions.append(Session(f"s{index}"))
                convs.append(conversation(args.seed + index, args.turns))
            for s, (r, _) in zip(sessions, daemon.exchange([s.open() for s in sessions])):
                s.accept(r, "open")
            history = [{f"h{i}": text for i, text in enumerate(h)} for h, _ in convs]
            for s, (r, _) in zip(sessions, daemon.exchange([s.clean(h) for s, h in zip(sessions, history)])):
                s.accept(r, "clean_blocks")
            daemon.rss()
            sample_outputs = [[] for _ in sessions]
            for turn in range(args.turns):
                requests = [s.clean({f"n{turn}": new[turn]}) for s, (_, new) in zip(sessions, convs)]
                for lane, (s, (r, elapsed)) in enumerate(zip(sessions, daemon.exchange(requests))):
                    s.accept(r, "clean_blocks")
                    turn_ms.append(elapsed * 1000)
                    sample_outputs[lane].append([{k: b[k] for k in ("id", "clean_text", "manifest", "tokens")}
                                                 for b in r["blocks"]])
            daemon.rss()
            for lane, (s, (r, elapsed)) in enumerate(zip(sessions, daemon.exchange([s.resync() for s in sessions]))):
                s.accept(r, "resync")
                reclean_ms.append(elapsed * 1000)
                sample_outputs[lane].append([{k: b[k] for k in ("id", "clean_text", "manifest", "tokens")}
                                             for b in r["blocks"]])
                sample_outputs[lane].append({k: r[k] for k in ("conversation_revision", "manifest_generation")})
            newest = [s.blocks[-1] for s in sessions]
            cleaned = [sample_outputs[lane][args.turns - 1][-1]["clean_text"] for lane in range(len(sessions))]
            responses = daemon.exchange([s.restore(c) for s, c in zip(sessions, cleaned)])
            for s, bid, (r, elapsed) in zip(sessions, newest, responses):
                s.accept(r, "restore")
                if r["text"] != s.retained[bid]:
                    raise RuntimeError("strict restore did not reproduce the raw block")
                restore_ms.append(elapsed * 1000)
            for lane, s in enumerate(sessions):
                outputs.append(normalize(sample_outputs[lane], {}))
                [(closed, _)] = daemon.exchange([s._common("close", "close")])
                s.accept(closed, "close")
            print(f"group {group + 1}/{args.samples // concurrency}: last turn {turn_ms[-1]:.1f} ms",
                  file=sys.stderr)
    finally:
        daemon.close()
        steady = sorted(daemon.samples)[len(daemon.samples) // 2] if daemon.samples else None
        peak = daemon.peak or None
        policy_sha = sha256_file(policy)
        shutil.rmtree(work, ignore_errors=True)
    end_probe = host_probe()
    probes.append(end_probe)

    pipeline_ids = {pid for _, pid in cold}
    return {
        "schema": SCHEMA,
        "envelope": args.envelope,
        "budgets_sha256": sha256_file(BUDGETS),
        "binary_sha256": sha256_file(args.binary),
        "policy_sha256": policy_sha,
        "policy_source": "gaze setup --non-interactive",
        "commit": args.commit,
        "workload": {"seed": args.seed, "samples": args.samples, "turns_per_sample": args.turns,
                     "concurrent_sessions": concurrency, "history_turns": HISTORY_TURNS,
                     "history_bytes_target": HISTORY_BYTES, "new_message_max_bytes": NEW_MESSAGE_MAX,
                     "daemon_args": [], "pipeline_ids_distinct": len(pipeline_ids)},
        "host": {"os": platform.system(), "release": platform.release(), "machine": platform.machine(),
                 "cpu_count": os.cpu_count(), "probes": probes},
        "quiet": all(p["quiet"] for p in probes),
        "output_sha256": digest_outputs(outputs),
        "samples_ms": {"turn": turn_ms, "reclean": reclean_ms, "restore": restore_ms,
                       "cold_start": [c * 1000 for c, _ in cold]},
        "rss_bytes": {"steady": steady, "peak": peak},
    }


def metrics(receipt: dict) -> dict[str, float | None]:
    s = receipt["samples_ms"]
    return {
        "turn_p50_ms": percentile(s["turn"], 0.50),
        "turn_p95_ms": percentile(s["turn"], 0.95),
        "turn_p99_ms": percentile(s["turn"], 0.99),
        "reclean_p95_ms": percentile(s["reclean"], 0.95),
        "restore_p95_ms": percentile(s["restore"], 0.95),
        "cold_start_max_ms": max(s["cold_start"]) if s["cold_start"] else None,
        "steady_rss_bytes": receipt["rss_bytes"]["steady"],
        "peak_rss_bytes": receipt["rss_bytes"]["peak"],
    }


def check(receipt: dict, budgets: dict) -> list[str]:
    """Every failure as text; empty means the receipt passes its envelope."""
    failures = []
    if receipt.get("schema") != SCHEMA:
        return [f"unknown receipt schema {receipt.get('schema')!r}"]
    envelope = budgets["envelopes"].get(receipt["envelope"])
    if envelope is None:
        return [f"unknown envelope {receipt['envelope']!r}"]
    probes = receipt.get("host", {}).get("probes") or []
    if len(probes) < 2 or not all(p.get("load", [99])[0] < QUIET_LOAD_1M and not p.get("foreign") for p in probes):
        failures.append("host was not quiet at every probe; the receipt is diagnostic only")
    if receipt.get("budgets_sha256") != sha256_file(BUDGETS):
        failures.append("receipt was measured against a different budgets file")
    if receipt.get("workload", {}).get("daemon_args"):
        failures.append("receipt used extra daemon flags; the envelope uses the setup profile only")
    if receipt.get("policy_source") != "gaze setup --non-interactive":
        failures.append("receipt policy was not written by the binary's own gaze setup")
    host = receipt.get("host", {})
    if (host.get("os"), host.get("machine")) != PLATFORMS[receipt["envelope"]]:
        failures.append(f"platform {host.get('os')}/{host.get('machine')} does not match the envelope")
    workload = envelope["workload"]
    if receipt["workload"]["concurrent_sessions"] != workload["concurrent_sessions"]:
        failures.append("concurrent session count differs from the envelope workload")
    if len(receipt["samples_ms"]["cold_start"]) < COLD_RUNS_MIN:
        failures.append(f"cold_start: fewer than {COLD_RUNS_MIN} runs (not a pass)")
    observed = metrics(receipt)
    for name, limit in envelope["budgets"].items():
        value = observed.get(name)
        if value is None:
            failures.append(f"{name}: missing measurement (not a pass)")
        elif value > limit:
            failures.append(f"{name}: {value:.1f} exceeds budget {limit}")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    m = sub.add_parser("measure")
    m.add_argument("--binary", type=Path, required=True)
    m.add_argument("--envelope", choices=["laptop", "server"], required=True)
    m.add_argument("--output", type=Path, required=True)
    m.add_argument("--commit", default=None, help="source commit of the binary, recorded verbatim")
    m.add_argument("--samples", type=int, default=100, help="timed sessions; p99 needs at least 100 turns")
    m.add_argument("--turns", type=int, default=1, help="timed turns per session after the 32 KiB history")
    m.add_argument("--concurrency", type=int, default=None, help="defaults to the envelope's session count")
    m.add_argument("--cold-runs", type=int, default=5)
    m.add_argument("--seed", type=int, default=20261126)
    m.add_argument("--allow-noisy", action="store_true", help="diagnostic run on a loaded host")
    c = sub.add_parser("check")
    c.add_argument("receipt", type=Path)
    d = sub.add_parser("compare")
    d.add_argument("base", type=Path)
    d.add_argument("candidate", type=Path)
    args = parser.parse_args()

    if args.command == "measure":
        receipt = measure(args)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        print(json.dumps({"quiet": receipt["quiet"], "output_sha256": receipt["output_sha256"],
                          **metrics(receipt)}, indent=2))
        return 0
    if args.command == "check":
        receipt = json.loads(args.receipt.read_text())
        budgets = json.loads(BUDGETS.read_text())
        failures = check(receipt, budgets)
        print(json.dumps(metrics(receipt), indent=2))
        for failure in failures:
            print(f"FAIL {failure}")
        print("F8 PASS" if not failures else "F8 FAIL")
        if not budgets["envelopes"].get(receipt["envelope"], {}).get("release_gate", True):
            print(f"note: the {receipt['envelope']} envelope is a published receipt and does not gate the release")
        return 1 if failures else 0
    base, cand = (json.loads(p.read_text()) for p in (args.base, args.candidate))
    keys = ("seed", "samples", "turns_per_sample", "concurrent_sessions")
    if [base["workload"][k] for k in keys] != [cand["workload"][k] for k in keys]:
        print("FAIL receipts used different workloads")
        return 1
    if base["policy_sha256"] != cand["policy_sha256"]:
        print("note: policies differ; each side uses its own `gaze setup` policy")
    same = base["output_sha256"] == cand["output_sha256"]
    print("outputs byte-identical" if same else "FAIL outputs differ")
    return 0 if same else 1


if __name__ == "__main__":
    sys.exit(main())
