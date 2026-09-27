#!/usr/bin/env python3
"""Measure how `gaze clean` time grows with input size (todo 3895).

The input is synthetic German order lines (no real PII):

    Zeile <n>: Bestellung 1500 Euro an 1010 Wien, NSW

`<n>` counts from 0, so every line from 10,000 on carries a distinct five-digit
number that `postal.de` tokenizes under `de-DE`: the manifest grows with the
input. Under `de-AT` the four-digit codes after a cue or before a city are
`postal.at_ch` candidates on every line: the candidate pool grows with the
input. Before the fix, 1 MB took 20 s under `de-DE` and time roughly tripled
per doubling under `de-AT`.

Each binary cleans each size `--repeat` times; the table reports the median
wall clock and the ratio to the next smaller size (linear is 2.0). Timing is a
claim only on a quiet host, so the 1-minute load average is printed.

With two binaries, `--compare` also diffs their outputs at every size and
locale: `clean_text`, `entries` (the manifest), `stats` and `leak_report` must
match once the random per-session token prefix is normalised, and each
output's `session_blob` must restore `clean_text` to the input exactly. The
first binary's outputs are the reference.

Usage (from the repository root):

    python3 scripts/bench/clean_scaling.py \\
        --binary base=<gaze at main> --binary cand=target/release/gaze \\
        --sizes 0.5,1,2,4 --locale de-DE --locale de-AT --compare
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path

LINE = "Zeile {n}: Bestellung 1500 Euro an 1010 Wien, NSW \n"
MIB = 1024 * 1024
SESSION_HEX = re.compile(r"\b([0-9a-f]{8})(?=:)")


def make_input(mib: float) -> str:
    lines, size, n = [], 0, 0
    while size < mib * MIB:
        line = LINE.format(n=n)
        lines.append(line)
        size += len(line)
        n += 1
    return "".join(lines)


def clean(binary: str, text: str, locale: str, policy: str | None) -> tuple[float, dict]:
    cmd = [binary, "clean", "--locale", locale]
    if policy:
        cmd += ["--policy", policy]
    start = time.perf_counter()
    done = subprocess.run(cmd, input=text.encode(), capture_output=True, check=False)
    elapsed = time.perf_counter() - start
    if done.returncode != 0:
        sys.exit(f"{binary} clean failed ({done.returncode}): {done.stderr.decode()[:400]}")
    return elapsed, json.loads(done.stdout)


def restore(binary: str, output: dict) -> str:
    request = json.dumps({"session_blob": output["session_blob"], "text": output["clean_text"]})
    done = subprocess.run(
        [binary, "restore"], input=request.encode(), capture_output=True, check=False
    )
    if done.returncode != 0:
        sys.exit(f"{binary} restore failed ({done.returncode}): {done.stderr.decode()[:400]}")
    return json.loads(done.stdout)["text"]


def normalised(output: dict) -> str:
    """The comparable part of one output, with the session prefix fixed."""
    comparable = {key: output.get(key) for key in ("clean_text", "stats", "leak_report")}
    # The manifest is emitted in hash-map order, which differs per process.
    comparable["entries"] = sorted(
        output.get("entries") or [], key=lambda entry: json.dumps(entry, sort_keys=True)
    )
    body = json.dumps(
        comparable,
        sort_keys=True,
        ensure_ascii=False,
    )
    prefixes = set(SESSION_HEX.findall(output["clean_text"]))
    if len(prefixes) > 1:
        sys.exit(f"more than one session prefix in clean_text: {sorted(prefixes)[:4]}")
    for prefix in prefixes:
        body = body.replace(prefix, "SESSION")
    return body


def host() -> str:
    chip = platform.processor() or platform.machine()
    if sys.platform == "darwin":
        chip = subprocess.run(
            ["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True
        ).stdout.strip() or chip
    return f"{chip}, {os.cpu_count()} CPUs, {platform.system()} {platform.release()}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--binary", action="append", required=True, help="label=path")
    parser.add_argument("--sizes", default="0.5,1,2,4", help="MiB, comma separated")
    parser.add_argument("--locale", action="append", help="repeatable; default de-DE")
    parser.add_argument("--policy", help="optional policy.toml for every run")
    parser.add_argument("--repeat", type=int, default=3)
    parser.add_argument("--compare", action="store_true")
    args = parser.parse_args()

    binaries = [entry.split("=", 1) for entry in args.binary]
    sizes = [float(size) for size in args.sizes.split(",")]
    locales = args.locale or ["de-DE"]
    print(f"host: {host()}; load average (1 min) {os.getloadavg()[0]:.2f}")
    print(f"policy: {args.policy or 'none (bundled core)'}; median of {args.repeat}\n")
    print("| locale | MiB | " + " | ".join(f"{label} s (×)" for label, _ in binaries) + " |")
    print("|---|---|" + "---|" * len(binaries))

    mismatches = 0
    for locale in locales:
        previous: dict[str, float] = {}
        for mib in sizes:
            text = make_input(mib)
            cells, reference = [], None
            for label, path in binaries:
                times, output = [], None
                for _ in range(args.repeat):
                    elapsed, output = clean(path, text, locale, args.policy)
                    times.append(elapsed)
                median = statistics.median(times)
                ratio = f" ({median / previous[label]:.1f}×)" if label in previous else ""
                previous[label] = median
                cells.append(f"{median:.2f}{ratio}")
                if args.compare:
                    if restore(path, output) != text:
                        print(f"RESTORE MISMATCH: {label} {locale} {mib} MiB", file=sys.stderr)
                        mismatches += 1
                    body = normalised(output)
                    if reference is None:
                        reference = body
                    elif body != reference:
                        print(f"OUTPUT MISMATCH: {label} {locale} {mib} MiB", file=sys.stderr)
                        mismatches += 1
            print(f"| {locale} | {mib:g} | " + " | ".join(cells) + " |", flush=True)
    if args.compare:
        print(f"\ncompare: {'identical' if mismatches == 0 else f'{mismatches} mismatches'}")
    return 1 if mismatches else 0


if __name__ == "__main__":
    raise SystemExit(main())
