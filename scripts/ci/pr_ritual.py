#!/usr/bin/env python3
"""Run the remaining PR ritual and record per-command exits and output tails."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "target/separator-checks/pr-ritual"
GIB = 1024 ** 3


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def resources():
    free = shutil.disk_usage(ROOT).free
    measured = subprocess.run(["du", "-sk", str(ROOT / "target")], capture_output=True,
                              text=True)
    # Cargo may remove temporary files while du walks them; its total remains usable.
    fields = measured.stdout.split()
    if measured.returncode not in (0, 1) or not fields or not fields[0].isdigit():
        raise RuntimeError(f"cannot measure target size: {measured.stderr.strip()}")
    size = int(fields[0]) * 1024
    return size, free


def safe_tail(path):
    value = "\n".join(path.read_text(errors="replace").splitlines()[-45:])
    return value.replace(str(ROOT), "<worktree>").replace(str(Path.home()), "<home>")


def stop_child(child):
    try:
        os.killpg(child.pid, signal.SIGINT)
    except ProcessLookupError:
        pass
    try:
        return child.wait(timeout=30)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        return child.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xtask-only", action="store_true",
                        help="run the thirteen xtask gates with a separate evidence record")
    parser.add_argument("--gate", action="append", default=[],
                        help="run only this named xtask gate; may be repeated")
    parser.add_argument("--matrix-tail", action="store_true",
                        help="complete a resource-paused full matrix with fresh workspace suites")
    parser.add_argument("--dry-run", action="store_true", help="validate and print the command plan")
    args = parser.parse_args()
    if args.matrix_tail and (args.gate or args.xtask_only):
        parser.error("matrix tail cannot be combined with gate selection")
    if os.environ.get("CARGO_TARGET_DIR"):
        raise RuntimeError("use cwd-relative Cargo targets")
    environment = dict(os.environ, CARGO_BUILD_JOBS="2", CARGO_PROFILE_DEV_DEBUG="0",
                       CARGO_PROFILE_TEST_DEBUG="0", CARGO_INCREMENTAL="0", CARGO_TERM_COLOR="never",
                       RUSTUP_TOOLCHAIN="1.96.0", GAZE_DISABLE_OS_KEYSTORE="1")
    environment["RUSTC"] = str(Path.home() / ".cargo/bin/rustc")
    environment["RUSTDOC"] = str(Path.home() / ".cargo/bin/rustdoc")
    section = (ROOT / "CONTRIBUTING.md").read_text().split("## Run the PR checks", 1)[1].split("```bash", 1)[1].split("```", 1)[0]
    gates = re.findall(r"^cargo run -p xtask -- ([a-z-]+)$", section, re.M)
    if len(gates) != 13 or len(set(gates)) != 13:
        raise RuntimeError("review the changed PR ritual before running it")
    commands = [("workspace-tests", ["python3", "scripts/anvil/test_nonempty.py", "cargo", "test", "--locked", "--workspace", "--all-features"]),
                ("cargo-deny", ["cargo", "deny", "check"])]
    commands.extend((gate, ["cargo", "run", "--locked", "-p", "xtask", "--", gate]) for gate in gates)
    if args.xtask_only or args.gate:
        commands = commands[2:]
    if args.gate:
        if len(set(args.gate)) != len(args.gate) or set(args.gate) - set(gates):
            parser.error("selected gates must be unique names from CONTRIBUTING.md")
        commands = [(name, command) for name, command in commands if name in args.gate]
    output = (OUTPUT.with_name("pr-ritual-selected") if args.gate else
              OUTPUT.with_name("pr-ritual-xtask") if args.xtask_only else OUTPUT)
    prefix = None
    if args.matrix_tail:
        prior_dir = OUTPUT.with_name("pr-ritual-selected")
        prior = json.loads((prior_dir / "summary.json").read_text())
        stopped = prior["commands"][-1]
        if stopped["name"] != "ci-feature-matrix" or not stopped.get("resource_stop"):
            raise RuntimeError("matrix tail requires an actual resource-paused matrix")
        if stopped["command"] != ["cargo", "run", "--locked", "-p", "xtask", "--", "ci-feature-matrix"]:
            raise RuntimeError("prefix must come from the full, unfiltered matrix")
        source_path = "crates/xtask/src/ci_feature_matrix.rs"
        source = (ROOT / source_path).read_bytes()
        measured_source = subprocess.check_output(
            ["git", "show", f"{prior['source_sha']}:{source_path}"], cwd=ROOT)
        if source != measured_source:
            raise RuntimeError("matrix definition changed after the prefix measurement")
        changes = subprocess.check_output(
            ["git", "diff", "--name-only", prior["source_sha"], "--"], cwd=ROOT,
            text=True).splitlines()
        if set(changes) - {"scripts/ci/pr_ritual.py"}:
            raise RuntimeError("matrix execution inputs changed; rerun its prefix")
        log_bytes = (prior_dir / "02-ci-feature-matrix.log").read_bytes()
        log_text = log_bytes.decode(errors="replace")
        labels = [line.removeprefix("ci_feature_matrix: running ")
                  for line in log_text.splitlines()
                  if line.startswith("ci_feature_matrix: running ")]
        if labels[-1] != "cargo test --workspace --all-features":
            raise RuntimeError("matrix stopped before the two final workspace suites")
        if "ci_feature_matrix: command failed:" in log_text:
            raise RuntimeError("a failed matrix command cannot be reused")
        if not re.search(r'args: &\["test", "--workspace", "--all-features"\],\s*},\s*DEFAULT_WORKSPACE_SUITE,\s*\];', source.decode()):
            raise RuntimeError("review the changed matrix tail definition")
        prefix = {"source_sha": prior["source_sha"],
                  "matrix_source_sha256": hashlib.sha256(source).hexdigest(),
                  "log_sha256": hashlib.sha256(log_bytes).hexdigest(),
                  "observed_steps": labels, "interrupted_step": labels[-1],
                  "proof": "The unchanged matrix returns on the first nonzero child; reaching the interrupted step proves preceding steps passed. Neither final workspace suite is credited until rerun."}
        # Use the matrix's own environment whitelist and fixed build settings.
        whitelist = source.decode().split("for var in [", 1)[1].split("] {", 1)[0]
        names = re.findall(r'"([A-Z_]+)"', whitelist)
        if len(names) < 10:
            raise RuntimeError("review the changed matrix environment recipe")
        environment = {name: environment[name] for name in names if name in environment}
        environment.update(GAZE_DISABLE_OS_KEYSTORE="1", CARGO_BUILD_JOBS="2",
                           CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0",
                           CARGO_PROFILE_TEST_DEBUG="0")
        environment.setdefault("CARGO_HOME", str(ROOT / "target/ci-feature-matrix-cargo-home"))
        commands = [
            ("matrix-all-features", ["python3", "scripts/anvil/test_nonempty.py", "cargo",
                                     "test", "--workspace", "--all-features"]),
            ("matrix-default", ["python3", "scripts/anvil/test_nonempty.py", "cargo", "run", "--locked", "-p", "xtask", "--",
                                "ci-feature-matrix", "--partition", "ci-default"]),
        ]
        output = OUTPUT.with_name("pr-ritual-matrix-tail")
    if args.dry_run:
        print(json.dumps({"commands": commands, "matrix_prefix": prefix}, indent=2))
        return 0
    output.mkdir(parents=True, exist_ok=True)
    report = {"schema": "gaze-pr-ritual-v1", "source_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "started_at": now(),
              "environment": {key: environment[key] for key in ("CARGO_BUILD_JOBS", "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG", "CARGO_INCREMENTAL", "RUSTUP_TOOLCHAIN", "GAZE_DISABLE_OS_KEYSTORE")},
              "commands": [], "planned_commands": len(commands), "completed": False}
    if prefix is not None:
        report["matrix_prefix"] = prefix

    def save():
        temporary = output / "summary.json.tmp"
        temporary.write_text(json.dumps(report, indent=2) + "\n")
        temporary.replace(output / "summary.json")

    save()
    for index, (name, command) in enumerate(commands, 1):
        size, free = resources()
        if size >= 13.5 * GIB or free < 102 * GIB:
            raise RuntimeError("insufficient headroom for the 15 GiB target cap and 100 GiB disk floor")
        print(f"PR RITUAL {index}/{len(commands)}: {name}", flush=True)
        log = output / f"{index:02}-{name}.log"
        started = now()
        resource_stop = None
        with log.open("wb") as stream:
            child = subprocess.Popen(command, cwd=ROOT, env=environment, stdout=stream,
                                     stderr=subprocess.STDOUT, start_new_session=True)
            next_resource = time.monotonic()
            next_progress = time.monotonic() + 30
            try:
                while child.poll() is None:
                    if time.monotonic() >= next_resource:
                        size, free = resources()
                        next_resource = time.monotonic() + 5
                        if size >= 13.5 * GIB or free < 102 * GIB:
                            resource_stop = "stopped early to preserve target/disk headroom"
                            break
                    if time.monotonic() >= next_progress:
                        print(f"PR RITUAL {index}/{len(commands)} active: target {size/GIB:.2f}/15 GiB; disk {free/GIB:.2f}/100 GiB", flush=True)
                        next_progress = time.monotonic() + 30
                    time.sleep(1)
            except BaseException as error:
                resource_stop = f"monitor failed: {type(error).__name__}: {error}"
            finally:
                code = stop_child(child) if child.poll() is None else child.wait()
        entry = {"name": name, "command": command, "started_at": started, "finished_at": now(),
                 "exit_code": code, "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest(), "tail": safe_tail(log)}
        if resource_stop:
            entry["resource_stop"] = resource_stop
        report["commands"].append(entry)
        save()
        print(f"PR RITUAL {index}/{len(commands)}: {name} exit={code}\n{entry['tail']}", flush=True)
        if code or resource_stop:
            return code or 1
        if args.matrix_tail and index == 1:
            cleanup_log = output / "cleanup-between-suites.log"
            with cleanup_log.open("wb") as stream:
                cleaned = subprocess.run(["cargo", "clean", "--profile", "dev"], cwd=ROOT,
                                         env=environment, stdout=stream, stderr=subprocess.STDOUT)
            report["between_suites_cleanup"] = {
                "exit_code": cleaned.returncode, "tail": safe_tail(cleanup_log),
                "log_sha256": hashlib.sha256(cleanup_log.read_bytes()).hexdigest()}
            save()
            if cleaned.returncode:
                return cleaned.returncode
    report["completed"] = True
    report["finished_at"] = now()
    save()
    print(f"PR RITUAL {len(commands)}/{len(commands)}: all commands passed", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
