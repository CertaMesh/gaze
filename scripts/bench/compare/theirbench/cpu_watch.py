#!/usr/bin/env python3
"""Foreign-CPU contention sampler (STEER 3 amendment).

Contention is CPU used by processes outside the measured process tree; the
measured tool's own threads never count. A sample above 100 % (one core)
marks the run contended. load1 is kept as a diagnostic only.

    cpu_watch.py --out watch.json -- <command ...>
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import threading
from typing import Sequence

CONTENDED_PERCENT = 100.0
NOISE_PERCENT = 5.0


def processes() -> list[tuple[int, int, float, str]]:
    output = subprocess.run(["ps", "-axo", "pid=,ppid=,%cpu=,comm="],
                            capture_output=True, text=True, check=True).stdout
    rows = []
    for line in output.splitlines():
        parts = line.split(None, 3)
        if len(parts) >= 3:
            rows.append((int(parts[0]), int(parts[1]), float(parts[2]), parts[3] if len(parts) > 3 else ""))
    return rows


def foreign_cpu(rows: Sequence[tuple[int, int, float, str]], root: int) -> tuple[float, list[list[object]]]:
    """Sum CPU outside `root`'s subtree (and outside this sampler), ignoring idle noise."""
    children: dict[int, list[int]] = {}
    for pid, ppid, _, _ in rows:
        children.setdefault(ppid, []).append(pid)
    own, stack = set(), [root, os.getpid()]
    while stack:
        pid = stack.pop()
        if pid not in own:
            own.add(pid)
            stack.extend(children.get(pid, ()))
    foreign = [(pid, cpu, name) for pid, _, cpu, name in rows if pid not in own and cpu >= NOISE_PERCENT]
    top = sorted(foreign, key=lambda item: -item[1])[:3]
    return round(sum(cpu for _, cpu, _ in foreign), 1), [[os.path.basename(name), cpu] for _, cpu, name in top]


class ForeignCpuWatch:
    def __init__(self, root: int | None = None, interval: float = 10.0) -> None:
        self.root = root if root is not None else os.getpid()
        self.interval = interval
        self.samples: list[dict[str, object]] = []
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)

    def sample(self) -> None:
        total, top = foreign_cpu(processes(), self.root)
        self.samples.append({"foreign_cpu_percent": total, "top": top, "load1": round(os.getloadavg()[0], 2)})

    def _run(self) -> None:
        while not self._stop.wait(self.interval):
            self.sample()

    def __enter__(self) -> "ForeignCpuWatch":
        self.sample()
        self._thread.start()
        return self

    def __exit__(self, *_: object) -> None:
        self._stop.set()
        self._thread.join()
        self.sample()

    def result(self) -> dict[str, object]:
        peak = max(float(s["foreign_cpu_percent"]) for s in self.samples)
        return {"foreign_cpu_percent_max": peak, "contended": peak > CONTENDED_PERCENT,
                "threshold_percent": CONTENDED_PERCENT, "samples": self.samples}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True)
    parser.add_argument("--interval", type=float, default=10.0)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    process = subprocess.Popen(command)
    with ForeignCpuWatch(process.pid, args.interval) as watch:
        code = process.wait()
    result = {**watch.result(), "exit_code": code}
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(result, handle, indent=2)
    print(f"CPU_WATCH contended={result['contended']} foreign_max={result['foreign_cpu_percent_max']}%",
          file=sys.stderr)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
