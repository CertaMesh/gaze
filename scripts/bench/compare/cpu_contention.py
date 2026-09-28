"""Sample CPU outside the measured comparator process tree."""

from __future__ import annotations

import os
import subprocess
import threading
import time


def foreign_cpu(process_table: str, root_pid: int, ignored_pid: int = -1) -> tuple[float, int]:
    processes: dict[int, tuple[int, float]] = {}
    for line in process_table.splitlines():
        fields = line.split()
        if len(fields) != 3:
            continue
        try:
            pid, parent = int(fields[0]), int(fields[1])
            cpu = float(fields[2])
        except ValueError:
            continue
        processes[pid] = (parent, cpu)

    def belongs_to_measurement(pid: int) -> bool:
        seen = set()
        while pid in processes and pid not in seen:
            if pid == root_pid:
                return True
            seen.add(pid)
            pid = processes[pid][0]
        return False

    foreign = [cpu for pid, (_, cpu) in processes.items()
               if pid != ignored_pid and cpu >= 5.0 and not belongs_to_measurement(pid)]
    return round(sum(foreign), 1), len(foreign)


class ForeignCpuSampler:
    def __init__(self, interval_seconds: float = 5.0) -> None:
        self.interval_seconds = interval_seconds
        self.root_pid = os.getpid()
        self.samples: list[dict[str, float | int]] = []
        self.errors: list[str] = []
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._started = 0.0
        self.load1_before = 0.0
        self.load1_after = 0.0

    def _sample(self) -> None:
        try:
            process = subprocess.Popen(
                ["ps", "-axo", "pid=,ppid=,%cpu="],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            try:
                output, error = process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.communicate()
                raise
            if process.returncode != 0:
                raise RuntimeError(f"ps exited {process.returncode}: {error.strip()[:100]}")
            cpu, count = foreign_cpu(output, self.root_pid, process.pid)
            self.samples.append({
                "elapsed_seconds": round(time.monotonic() - self._started, 1),
                "foreign_cpu_percent": cpu,
                "foreign_busy_processes": count,
                "load1": round(os.getloadavg()[0], 2),
            })
        except (OSError, subprocess.TimeoutExpired, RuntimeError) as error:
            self.errors.append(type(error).__name__)

    def _run(self) -> None:
        self._sample()
        while not self._stop.wait(self.interval_seconds):
            self._sample()

    def __enter__(self) -> ForeignCpuSampler:
        self._started = time.monotonic()
        self.load1_before = round(os.getloadavg()[0], 2)
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()
        return self

    def __exit__(self, *_exception: object) -> None:
        self._stop.set()
        assert self._thread is not None
        self._thread.join(timeout=15)
        self.load1_after = round(os.getloadavg()[0], 2)

    def result(self) -> dict:
        return {
            "foreign_cpu_samples": self.samples,
            "sample_errors": self.errors,
            "load1_before_after": [self.load1_before, self.load1_after],
            "contended": any(sample["foreign_cpu_percent"] > 100 for sample in self.samples),
            "valid": bool(self.samples) and not self.errors,
        }
