#!/usr/bin/env python3
"""Attribute two Gaze revisions on pinned inputs. Dry runs are never measurements.

See perf-attribution.md. Requires the Solo bench-machine lease for real captures.
Receipts contain hashes, fixed metric keys, lengths, counts and durations only.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import platform
import queue
import re
import random
import subprocess
import threading
import time
import tomllib
from dataclasses import dataclass
from pathlib import Path

from perf_lifecycle_contract import OWNERSHIP_CHECKS, violates_ownership, CLOCK_CHECKS, CLOCK_PHASES, REQUEST_ASSEMBLY, STARTUP_CLOCKS, violates_check

SPEC = "gaze-perf-attribution-v2"


def runner_sha256() -> str:
    return hashlib.sha256(Path(__file__).read_bytes() + b"\0"
                          + Path(__file__).with_name("perf_lifecycle_contract.py").read_bytes()).hexdigest()


RUNNER_SHA256 = runner_sha256()
ROOT = Path(__file__).resolve().parents[2]
PREFIX = "GAZE_PERF "
TEXT_SUFFIXES = {".txt", ".md", ".json", ".jsonl", ".toml", ".yaml", ".yml", ".rs", ".py", ".csv", ".tsv"}

LIFECYCLE = set(STARTUP_CLOCKS)
METRICS = set(CLOCK_PHASES)
COUNTERS = {"input_bytes", "ner_encodes", "ner_windows", "sweep_candidates", "nym_windows",
            "nym_windows_stable", "nym_windows_neutral"}


def validate_trace(trace: dict) -> dict:
    """Reject diagnostics outside the value-free protocol before writing a receipt."""
    def metric(value, *, recognizer=False):
        if recognizer:
            return (isinstance(value, dict)
                    and set(value) == {"ns", "invocations", "candidates", "evidence", "model"}
                    and value["evidence"] in ("rule", "learned")
                    and value["model"] in ("none", "ner", "dob", "other")
                    and all(type(value[k]) is int and value[k] >= 0
                            for k in ("ns", "invocations", "candidates")))
        return (isinstance(value, dict) and "ns" in value
                and set(value) <= {"ns", "invocations", "candidates"}
                and all(type(number) is int and number >= 0 for number in value.values()))
    valid = (isinstance(trace, dict) and set(trace) == {"schema", "phase", "ordinal", "metrics", "counters", "recognizers"}
             and type(trace["schema"]) is int and trace["schema"] == 3
             and trace["phase"] in ("startup", "request")
             and type(trace["ordinal"]) is int and trace["ordinal"] >= 0
             and isinstance(trace["metrics"], dict) and set(trace["metrics"]) <= METRICS
             and all(metric(value) for value in trace["metrics"].values())
             and isinstance(trace["counters"], dict) and set(trace["counters"]) <= COUNTERS
             and all(type(n) is int and n >= 0 for n in trace["counters"].values())
             and isinstance(trace["recognizers"], dict)
             and all(re.fullmatch(r"r[1-9][0-9]*", key) and metric(value, recognizer=True)
                     for key, value in trace["recognizers"].items()))
    if not valid:
        raise ValueError("invalid timer metadata; diagnostic contents suppressed")
    return trace


def validate_lifecycle(traces: list[dict], requests: int, *, daemon: bool, policy: Path,
                       required: bool = False) -> tuple[dict | None, list[dict]]:
    """No partial protocol fallback: absent instrumentation is unknown, partial is invalid."""
    if not traces:
        if required:
            raise ValueError("required lifecycle trace missing")
        return None, []
    for trace in traces:
        validate_trace(trace)
    if (len(traces) != requests + 1 or traces[0]["phase"] != "startup"
            or traces[0]["ordinal"] != 0
            or any(t["phase"] != "request" or t["ordinal"] != i + 1
                   for i, t in enumerate(traces[1:]))):
        raise ValueError("lifecycle trace attribution mismatch")
    for record in traces:
        for key, metric in record["metrics"].items():
            one_shot_assembly = not daemon and record["phase"] == "request" and key in REQUEST_ASSEMBLY
            if (record["phase"] not in CLOCK_PHASES[key] or one_shot_assembly) and any(metric.values()):
                raise ValueError("misattributed clock phase")
            if metric["ns"] and not metric.get("invocations", 0):
                raise ValueError("duration without an invocation")
        if record["phase"] == "startup" and (any(record["counters"].values()) or record["recognizers"]):
            raise ValueError("misattributed startup processing")
        for check in CLOCK_CHECKS:
            if check.phase != record["phase"] or (check.daemon_only and not daemon):
                continue
            if violates_check(record, check):
                raise ValueError(f"lifecycle duration attribution mismatch: {check.id}")
    startup = traces[0]
    def inv(trace, key):
        return trace["metrics"].get(key, {}).get("invocations", 0)
    for key in ("process_to_ready_ns", "process_to_policy_ns", "policy_load_ns",
                "pipeline_build_ns", "assembly_ns", "rule_construction_ns", "recognizer_construction_ns"):
        if inv(startup, key) != 1:
            raise ValueError("required lifecycle hook missing")
    if inv(startup, "daemon_startup_ns") != int(daemon) or inv(startup, "request_ns"):
        raise ValueError("misattributed startup work")
    config = tomllib.loads(policy.read_text())
    net = config.get("safety_net", {}).get("backend") == "nym"
    constructors = sum(bool(config.get(name)) and config[name].get("enabled", True)
                       for name in ("ner", "dob_judge")) + int(net)
    for key, count in (("model_constructor_ns", constructors - int(net)),
                       ("nym_model_constructor_ns", int(net)),
                       ("nym_outer_verify_ns", int(net)),
                       ("tokenizer_load_ns", constructors), ("model_load_ns", constructors)):
        if inv(startup, key) != count:
            raise ValueError("model constructor attribution mismatch")
    # Assembly and the Nym constructor both verify today. Do not optimize that away here.
    verifications = constructors + int(net)
    if inv(startup, "bundle_verify_ns") != verifications or inv(startup, "nym_attach_ns") != int(net):
        raise ValueError("bundle verification attribution mismatch")
    roles = {}
    for trace in traces[1:]:
        for check in OWNERSHIP_CHECKS:
            if violates_ownership(trace, check, config=config, roles=roles):
                raise ValueError(f"lifecycle recognizer ownership mismatch: {check}")
        roles.update((key, (entry["evidence"], entry["model"]))
                     for key, entry in trace["recognizers"].items())
        forbidden = LIFECYCLE - REQUEST_ASSEMBLY
        if inv(trace, "request_ns") != 1 or any(inv(trace, key) for key in forbidden):
            raise ValueError("misattributed request work")
        assembly_counts = {inv(trace, key) for key in REQUEST_ASSEMBLY}
        if assembly_counts not in ({0}, {1}):
            raise ValueError("request assembly hook attribution mismatch")
    return startup, traces[1:]


def input_identity(docs: list[Document]) -> str:
    """Bind identity, exact UTF-8 bytes and order (including duplicate occurrences)."""
    return sha(json.dumps([[d.id, sha(d.text.encode()), len(d.text.encode())] for d in docs],
                          separators=(",", ":")).encode())


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def child_environment() -> dict[str, str]:
    # Model/policy overrides must never outrank the inventoried policy paths.
    # Keep OS execution/location variables; setup resolves HOME/XDG model locations
    # into explicit policy paths which are inventoried before workload execution.
    allowed = {"PATH", "HOME", "XDG_DATA_HOME", "SYSTEMROOT", "WINDIR",
               "TMPDIR", "TMP", "TEMP", "LANG", "LC_ALL"}
    return {**{key: value for key, value in os.environ.items() if key in allowed},
            "GAZE_PERF": "1"}


def command(argv: list[str], *, cwd: Path = ROOT, env: dict[str, str] | None = None) -> str:
    result = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True)
    if result.returncode:
        # Child diagnostics can contain source text, policy paths or recognizer IDs.
        raise RuntimeError(f"{Path(argv[0]).name} failed (exit {result.returncode}); diagnostics suppressed")
    return result.stdout


def commit(rev: str) -> str:
    return command(["git", "rev-parse", "--verify", f"{rev}^{{commit}}"]).strip()


def quiet_threshold() -> float:
    # nym-warm-latency.py records load only. cli-latency.py owns the actual gate.
    spec = importlib.util.spec_from_file_location("attribution_cli_latency", ROOT / "scripts/bench/cli-latency.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.QUIET_LOAD_1M


def host() -> dict:
    def sysctl(key: str) -> str | None:
        try:
            result = subprocess.run(["sysctl", "-n", key], capture_output=True, text=True)
            return result.stdout.strip() if result.returncode == 0 else None
        except FileNotFoundError:
            return None
    return {
        "os": platform.system(), "release": platform.release(), "machine": platform.machine(),
        "cpu": sysctl("machdep.cpu.brand_string") or platform.processor(),
        "cores": os.cpu_count(), "memory_bytes": sysctl("hw.memsize") or (os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES") if hasattr(os, "sysconf") else None),
        "load": list(os.getloadavg()), "uptime": command(["uptime"]).strip(),
    }


@dataclass(frozen=True)
class Document:
    id: str
    text: str
    digest: str


def documents(rev: str, subdir: str, seed: int, *, tree: bool = False) -> tuple[str, list[Document]]:
    revision = commit(rev)
    raw = subprocess.check_output(["git", "ls-tree", "-rlz", revision, "--", subdir], cwd=ROOT)
    objects = []
    for entry in raw.split(b"\0"):
        if not entry:
            continue
        metadata, name = entry.split(b"\t", 1)
        mode, kind, oid, size = metadata.split()
        path = name.decode("utf-8")
        if kind != b"blob" or mode == b"120000" or int(size) > 1_000_000:
            continue
        if Path(path).suffix not in (TEXT_SUFFIXES if tree else {".txt"}):
            continue
        objects.append((path, oid))
    # Testdata first for the source-tree scanner arm; deterministic within groups.
    rng = random.Random(seed)
    rng.shuffle(objects)
    if tree:
        objects.sort(key=lambda pair: "testdata" not in pair[0])
    found = []
    for path, oid in objects:
        content = subprocess.check_output(["git", "cat-file", "blob", oid.decode("ascii")], cwd=ROOT)
        try:
            text = content.decode("utf-8")
        except UnicodeDecodeError:
            continue
        if not text.strip() or "\0" in text:
            continue
        found.append(Document(sha(path.encode()), text, sha(content)))
        if tree and len(found) == 400:
            break
    if not found or (tree and len(found) < 400):
        raise ValueError("pinned input tree has insufficient eligible documents")
    return revision, found


def model_inventory(paths: list[Path]) -> list[dict]:
    inventories = []
    for root in paths:
        if not root.is_dir():
            raise ValueError("model directory missing")
        files = [{"id": sha(str(p.relative_to(root)).encode()), "sha256": file_sha(p), "bytes": p.stat().st_size}
                 for p in sorted(root.rglob("*")) if p.is_file()]
        if not files:
            raise ValueError("empty model directory")
        inventories.append({"sha256": sha(json.dumps(files, sort_keys=True).encode()), "files": files})
    return inventories


def policy_models(policy: Path) -> list[Path]:
    config = tomllib.loads(policy.read_text())
    found = []
    net = config.get("safety_net", {})
    if net.get("backend") == "nym" and not net.get("nym", {}).get("model_dir"):
        raise ValueError("Nym policies must pin an explicit model_dir")
    for name in ("ner", "dob_judge"):
        section = config.get(name, {})
        if section and section.get("enabled", True) and not section.get("model_dir"):
            raise ValueError("model policies must pin explicit model_dir values")
    def visit(value):
        if isinstance(value, dict):
            for key, item in value.items():
                if key == "terms_file" or (key == "paths" and item):
                    raise ValueError("external rulepack and terms files are unsupported; use bundled rules and inline terms")
                if key == "model_dir":
                    p = Path(item)
                    if not p.is_absolute():
                        raise ValueError("model_dir must be absolute to freeze the effective input")
                    found.append(p)
                else:
                    visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)
    visit(config)
    return sorted(set(found))


def vernio_policy(path: Path, ner: Path) -> Path:
    path.write_text('schema_version = "0.1.0"\n[session]\nscope = "conversation"\n'
                    '[locale]\nactive = ["de-DE", "en-US", "global"]\n'
                    f'[ner]\nmodel_dir = {json.dumps(str(ner))}\nthreshold = 0.3\n'
                    '[address_blocks]\nenabled = true\n[policy.rulepacks]\nbundled = ["core"]\n'
                    '[safety_net]\nbackend = "none"\n[[rule]]\nkind = "default"\naction = "tokenize"\n')
    return path


def build_binary(rev: str, *, timers: bool) -> tuple[Path, str]:
    revision = commit(rev)
    branch = f"perf/attribution-build-{revision[:12]}{'-timers' if timers else ''}"
    info = json.loads(command(["anvil", "ensure", branch, "--base", revision, "--json"]))
    if not info["ok"] or not info["data"]["prepare"]["ready"]:
        raise RuntimeError("Anvil preparation failed")
    checkout = Path(info["data"]["worktree"]["path"])
    # Verify a reused branch still points at the requested source.
    if command(["git", "rev-parse", "HEAD"], cwd=checkout).strip() != revision:
        raise ValueError("build worktree does not match requested revision")
    if command(["git", "status", "--porcelain"], cwd=checkout).strip():
        raise ValueError("automatic builds require a clean source checkout; supply patched binaries explicitly")
    toolbin = Path(command(["rustup", "which", "--toolchain", "1.96.0", "rustc"]).strip()).parent
    env = {**os.environ, "PATH": str(toolbin) + os.pathsep + os.environ["PATH"],
           "RUSTC": str(toolbin / "rustc"), "RUSTDOC": str(toolbin / "rustdoc"), "CARGO_BUILD_JOBS": "2"}
    if "CARGO_TARGET_DIR" in env:
        raise ValueError("CARGO_TARGET_DIR must be unset")
    argv = ["nice", "-n", "10", "cargo", "build", "--release", "-p", "gaze-cli"]
    if timers:
        argv += ["--features", "perf-timers"]
    log = checkout / "target/perf-attribution-build.log"
    log.parent.mkdir(exist_ok=True)
    with log.open("w") as output:
        subprocess.run(argv, cwd=checkout, env=env, stdout=output, stderr=output, check=True)
    return checkout / "target/release/gaze", revision


class Daemon:
    def __init__(self, binary: Path, policy: Path, timeout: float, child_env: dict[str, str]):
        self.timeout = timeout
        self.rows: queue.Queue = queue.Queue()
        self.perf: queue.Queue = queue.Queue()
        self.ready = threading.Event()
        self.startup_e2e_ns = None
        started = time.perf_counter_ns() if child_env.get("GAZE_PERF_WALL") == "1" else None
        child_env = {k: v for k, v in child_env.items() if k != "GAZE_PERF_WALL"}
        self.child = subprocess.Popen([str(binary), "daemon", "--policy", str(policy)], cwd=ROOT,
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                      text=True, encoding="utf-8", env=child_env)
        def stdout():
            for line in self.child.stdout:
                self.rows.put(line)
            self.rows.put(None)
        def stderr():
            for line in self.child.stderr:
                if line.startswith(PREFIX):
                    try:
                        trace = json.loads(line[len(PREFIX):])
                        self.perf.put(trace)
                        if trace.get("phase") == "startup":
                            self.startup_e2e_ns = time.perf_counter_ns() - started if started is not None else None
                            self.ready.set()
                    except json.JSONDecodeError:
                        self.perf.put({"invalid": True})
        self.threads = [threading.Thread(target=stdout, daemon=True), threading.Thread(target=stderr, daemon=True)]
        for thread in self.threads:
            thread.start()

    def request(self, doc: Document, ordinal: int, *, timed: bool) -> dict:
        started = time.perf_counter_ns() if timed else None
        self.child.stdin.write(json.dumps({"session_id": f"doc-{ordinal}", "text": doc.text}) + "\n")
        self.child.stdin.flush()
        try:
            line = self.rows.get(timeout=self.timeout)
        except queue.Empty:
            raise RuntimeError("daemon response timeout") from None
        elapsed = time.perf_counter_ns() - started if timed else None
        if line is None:
            raise RuntimeError("daemon exited before response; diagnostics suppressed")
        response = json.loads(line)
        if response.get("error") or "clean_text" not in response:
            raise RuntimeError("document refused; no latency claim is allowed")
        # Output is consumed and discarded: it can include reversible PII mappings.
        return {"id": doc.id, "input_bytes": len(doc.text.encode()), "e2e_ns": elapsed}

    def close(self) -> list[dict]:
        self.child.stdin.close()
        try:
            self.child.wait(timeout=self.timeout)
        except subprocess.TimeoutExpired:
            self.child.kill()
            self.child.wait()
            raise RuntimeError("daemon shutdown timeout") from None
        for thread in self.threads:
            thread.join(timeout=self.timeout)
        if self.child.returncode:
            raise RuntimeError("daemon failed; diagnostics suppressed")
        self.child.stdout.close()
        self.child.stderr.close()
        return list(self.perf.queue)

    def abort(self):
        if self.child.poll() is None:
            self.child.kill()
        self.child.wait()
        for thread in self.threads:
            thread.join(timeout=self.timeout)
        self.child.stdin.close()
        self.child.stdout.close()
        self.child.stderr.close()


def run_pass(binary: Path, policy: Path, docs: list[Document], *, warmups: int, dry: bool, timeout: float, per_file: bool, child_env: dict[str, str] | None = None, require_timers: bool = False) -> list[dict]:
    if child_env is None:
        child_env = child_environment()
    if per_file:
        if warmups:
            run_pass(binary, policy, [docs[i % len(docs)] for i in range(warmups)],
                     warmups=0, dry=True, timeout=timeout, per_file=True, child_env=child_env, require_timers=require_timers)
        rows = []
        # This arm includes process/model startup on EVERY file, like one-shot scanners.
        for doc in docs:
            started = time.perf_counter_ns() if not dry else None
            output = subprocess.run([str(binary), "clean", "--policy", str(policy)], input=doc.text,
                                    text=True, capture_output=True, timeout=timeout, cwd=ROOT,
                                    env=child_env)
            elapsed = time.perf_counter_ns() - started if not dry else None
            if output.returncode or "clean_text" not in json.loads(output.stdout):
                raise RuntimeError("one-shot document failed; diagnostics suppressed")
            traces = [json.loads(line[len(PREFIX):]) for line in output.stderr.splitlines() if line.startswith(PREFIX)]
            startup, requests = validate_lifecycle(traces, 1, daemon=False, policy=policy, required=require_timers)
            rows.append({"id": doc.id, "input_bytes": len(doc.text.encode()), "e2e_ns": elapsed,
                         "startup_trace": startup, "startup_e2e_ns": None,
                         "trace": requests[0] if requests else None})
        return rows
    daemon = Daemon(binary, policy, timeout, {**child_env, "GAZE_PERF_WALL": "0" if dry else "1"})
    try:
        if require_timers and not daemon.ready.wait(timeout):
            raise ValueError("required lifecycle startup trace missing")
        for i in range(warmups):
            daemon.request(docs[i % len(docs)], i, timed=False)
        rows = [daemon.request(doc, warmups + i, timed=not dry) for i, doc in enumerate(docs)]
        traces = daemon.close()
    except BaseException:
        daemon.abort()
        raise
    startup, requests = validate_lifecycle(traces, warmups + len(rows), daemon=True, policy=policy, required=require_timers)
    for i, row in enumerate(rows):
        row["trace"] = requests[warmups + i] if requests else None
        row["startup_trace"] = startup if i == 0 else None
        row["startup_e2e_ns"] = daemon.startup_e2e_ns if i == 0 else None
    return rows


def percentiles(samples: list[int]) -> dict:
    ordered = sorted(samples)
    return {f"p{p}": ordered[max(0, math.ceil(p / 100 * len(ordered)) - 1)] for p in (50, 95, 99)}


def summary(rows: list[dict]) -> dict:
    values = [row["e2e_ns"] for row in rows]
    traced = [r for r in rows if r["trace"] is not None]
    keys = sorted({key for row in traced for key in row["trace"]["metrics"]})
    def stages(population):
        return {key: percentiles([r["trace"]["metrics"].get(key, {}).get("ns", 0) for r in population]) for key in keys} if population else None
    ids = sorted({row["id"] for row in rows})
    startups = [r["startup_trace"] for r in rows if r.get("startup_trace") is not None]
    cold = [r["startup_e2e_ns"] for r in rows if r.get("startup_e2e_ns") is not None]
    return {"documents": len(rows), "e2e_ns": percentiles(values),
            "startup_e2e_ns": percentiles(cold) if cold else None,
            "startup_stages_ns": {key: percentiles([t["metrics"].get(key, {}).get("ns", 0) for t in startups]) for key in sorted(LIFECYCLE)} if startups else None,
            "startup_invocations": {key: sum(t["metrics"].get(key, {}).get("invocations", 0) for t in startups) for key in sorted(LIFECYCLE)} if startups else None,
            "startup_total_ns": sum(t["metrics"]["process_to_ready_ns"]["ns"] for t in startups) if startups else None,
            "processing_total_ns": sum(r["trace"]["metrics"]["request_ns"]["ns"] for r in traced) if traced else None,
            "files_per_minute": len(values) * 60e9 / sum(values),
            "per_document": {id: {"e2e_ns": percentiles([r["e2e_ns"] for r in rows if r["id"] == id]),
                                   "stages_ns": stages([r for r in traced if r["id"] == id])} for id in ids},
            "stages_ns": stages(traced),
            "recognizer_scope": "process-local ordinals; inspect individual traces",
            "timer_documents": len(traced)}


def publishable(probes: list[dict], *, dry: bool, passes: int, warmups: int) -> bool:
    return bool(probes) and not dry and passes >= 2 and warmups >= 1 and all(p["load"][0] < quiet_threshold() for p in probes)


def parse_args(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    for side, default in (("a", "v0.16.0"), ("b", "v0.17.0")):
        p.add_argument(f"--binary-{side}", type=Path)
        p.add_argument(f"--rev-{side}", default=default)
        p.add_argument(f"--binary-kind-{side}", choices=["provided", "published-release", "local-timer"], default="provided")
        p.add_argument(f"--timer-patch-{side}", type=Path)
    p.add_argument("--build-timers", action="store_true", help="revisions must already contain perf-timers")
    p.add_argument("--common-policy", type=Path, required=True)
    p.add_argument("--shipped-policy-a", type=Path)
    p.add_argument("--shipped-policy-b", type=Path)
    p.add_argument("--ner-model-dir", type=Path, required=True)
    p.add_argument("--corpus-rev", default="v0.17.0")
    p.add_argument("--corpus-subdir", default="crates/gaze-recognizers/testdata/coverage-loop/corpus")
    p.add_argument("--tree-rev", default="5ac6c06")
    p.add_argument("--passes", type=int, default=4)
    p.add_argument("--warmups", type=int, default=2)
    p.add_argument("--seed", type=int, default=20260710)
    p.add_argument("--timeout", type=float, default=120)
    mode = p.add_mutually_exclusive_group(required=True)
    mode.add_argument("--dry-run", action="store_true")
    mode.add_argument("--measure", action="store_true", help="requires exclusive Solo bench-machine lease")
    p.add_argument("--out", type=Path, required=True)
    args = p.parse_args(argv)
    if args.passes < 1 or args.warmups < 0 or args.timeout <= 0:
        p.error("passes/timeout must be positive; warmups must be nonnegative")
    return args


def main(argv=None) -> int:
    args = parse_args(argv)
    args.out = args.out.resolve()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    assets = args.out.parent / (args.out.stem + "-policies")
    assets.mkdir(exist_ok=True)
    binaries, revisions = {}, {}
    for side in ("a", "b"):
        path = getattr(args, f"binary_{side}")
        if path:
            binaries[side], revisions[side] = path.resolve(), commit(getattr(args, f"rev_{side}"))
        else:
            binaries[side], revisions[side] = build_binary(getattr(args, f"rev_{side}"), timers=args.build_timers)
    hashes = {side: file_sha(path) for side, path in binaries.items()}
    if hashes["a"] == hashes["b"]:
        raise ValueError("identical binaries refused")
    child_env = child_environment()
    common = args.common_policy.resolve()
    vernio = vernio_policy(assets / "vernio.toml", args.ner_model_dir.resolve())
    shipped = {}
    for side in ("a", "b"):
        provided = getattr(args, f"shipped_policy_{side}")
        shipped[side] = provided.resolve() if provided else assets / f"shipped-{side}.toml"
        if provided is None:
            command([str(binaries[side]), "setup", "--non-interactive", "--force", "--policy-out", str(shipped[side])], env=child_env)
    arms = {"common": {"a": common, "b": common}, "shipped": shipped,
            "vernio": {"a": vernio, "b": vernio}, "source-tree": {"a": vernio, "b": vernio},
            "source-tree-daemon": {"a": vernio, "b": vernio}}
    corpus_rev, corpus = documents(args.corpus_rev, args.corpus_subdir, args.seed)
    tree_rev, tree = documents(args.tree_rev, ".", args.seed, tree=True)
    model_dirs = sorted({p for policies in arms.values() for path in policies.values() for p in policy_models(path)})
    models = model_inventory(model_dirs)
    policy_hashes = {name: {side: file_sha(path) for side, path in policies.items()} for name, policies in arms.items()}
    receipt = {"specification": SPEC, "runner_sha256": RUNNER_SHA256, "dry_run": args.dry_run, "publishable": False,
               "verdict": "NOT A MEASUREMENT" if args.dry_run else "pending validation",
               "binary_sha256": hashes, "declared_revisions": revisions,
               "binary_kind": {side: getattr(args, f"binary_kind_{side}") for side in ("a", "b")},
               "timer_patch_sha256": {side: file_sha(getattr(args, f"timer_patch_{side}")) if getattr(args, f"timer_patch_{side}") else None for side in ("a", "b")},
               "model_inventory": models,
               "child_environment_sha256": sha(json.dumps(child_env, sort_keys=True).encode()),
               "policies": policy_hashes, "seed": args.seed,
               "passes": 1 if args.dry_run else args.passes, "warmups": args.warmups,
               "quiet_load1_threshold": quiet_threshold(), "quiet_threshold_source": "scripts/bench/cli-latency.py",
               "inputs": {"corpus": {"revision": corpus_rev, "documents": [{"id": d.id, "sha256": d.digest} for d in corpus]},
                          "source-tree": {"revision": tree_rev, "documents": [{"id": d.id, "sha256": d.digest} for d in tree]}},
               "probes": [host()], "arms": {}}
    rng = random.Random(args.seed)
    first = rng.choice(["ab", "ba"])
    for name, policies in arms.items():
        docs = tree if name in ("source-tree", "source-tree-daemon") else corpus
        if args.dry_run:
            docs = docs[:5]
            if len(docs) != 5:
                raise ValueError("dry run requires five documents per arm")
        runs = []
        for index in range(receipt["passes"]):
            order = first if index % 2 == 0 else first[::-1]
            rows = {}
            for side in order:
                receipt["probes"].append(host())
                rows[side] = run_pass(binaries[side], policies[side], docs, warmups=args.warmups,
                                      dry=args.dry_run, timeout=args.timeout, per_file=name == "source-tree", child_env=child_env,
                                      require_timers=args.build_timers or getattr(args, f"binary_kind_{side}") == "local-timer")
                receipt["probes"].append(host())
            runs.append({"order": order, "sides": rows})
        arm = {"process_mode": "one-shot-per-file" if name == "source-tree" else "warm-daemon", "input_sha256": input_identity(docs), "input_bytes": sum(len(d.text.encode()) for d in docs), "runs": runs}
        if not args.dry_run:
            arm["summary"] = {side: summary([r for run in runs for r in run["sides"][side]]) for side in ("a", "b")}
            arm["paired_delta_ns_b_minus_a"] = {doc.id: percentiles([
                next(r["e2e_ns"] for r in run["sides"]["b"] if r["id"] == doc.id) -
                next(r["e2e_ns"] for r in run["sides"]["a"] if r["id"] == doc.id)
                for run in runs]) for doc in docs}
        receipt["arms"][name] = arm
    if (receipt["arms"]["source-tree"]["input_sha256"] != receipt["arms"]["source-tree-daemon"]["input_sha256"]
            or policy_hashes["source-tree"] != policy_hashes["source-tree-daemon"]):
        raise ValueError("source-tree arms must have identical ordered inputs and policies")
    receipt["probes"].append(host())
    if runner_sha256() != RUNNER_SHA256:
        raise ValueError("runner changed during capture")
    if hashes != {side: file_sha(path) for side, path in binaries.items()}:
        raise ValueError("binary changed during capture")
    if policy_hashes != {name: {side: file_sha(path) for side, path in policies.items()} for name, policies in arms.items()}:
        raise ValueError("policy changed during capture")
    if models != model_inventory(model_dirs):
        raise ValueError("model changed during capture")
    receipt["instrumentation_parity"] = all(
        [row["trace"] is not None for row in run["sides"]["a"]] ==
        [row["trace"] is not None for row in run["sides"]["b"]]
        for arm in receipt["arms"].values() for run in arm["runs"])
    receipt["publishable"] = receipt["instrumentation_parity"] and publishable(
        receipt["probes"], dry=args.dry_run, passes=args.passes, warmups=args.warmups)
    if not args.dry_run:
        receipt["verdict"] = "valid" if receipt["publishable"] else "NOT PUBLISHABLE: host load, warmup, pass or instrumentation parity gate failed"
    args.out.write_text(json.dumps(receipt, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
