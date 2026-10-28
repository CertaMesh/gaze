#!/usr/bin/env python3
"""Run the remaining PR ritual and record per-command exits and output tails."""
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
    OUTPUT.mkdir(parents=True, exist_ok=True)
    report = {"schema": "gaze-pr-ritual-v1", "source_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "started_at": now(),
              "environment": {key: environment[key] for key in ("CARGO_BUILD_JOBS", "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG", "CARGO_INCREMENTAL", "RUSTUP_TOOLCHAIN", "GAZE_DISABLE_OS_KEYSTORE")},
              "commands": [], "planned_commands": len(commands), "completed": False}

    def save():
        temporary = OUTPUT / "summary.json.tmp"
        temporary.write_text(json.dumps(report, indent=2) + "\n")
        temporary.replace(OUTPUT / "summary.json")

    save()
    for index, (name, command) in enumerate(commands, 1):
        size, free = resources()
        if size >= 13.5 * GIB or free < 102 * GIB:
            raise RuntimeError("insufficient headroom for the 15 GiB target cap and 100 GiB disk floor")
        print(f"PR RITUAL {index}/{len(commands)}: {name}", flush=True)
        log = OUTPUT / f"{index:02}-{name}.log"
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
    report["completed"] = True
    report["finished_at"] = now()
    save()
    print(f"PR RITUAL {len(commands)}/{len(commands)}: all commands passed", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
