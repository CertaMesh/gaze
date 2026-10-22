#!/usr/bin/env python3
"""Run a Rust test command and reject success without any executed tests."""

import re
import subprocess
import sys


SUMMARY = re.compile(r"^test result: ok\. (\d+) passed; (\d+) failed;")


def main(argv: list[str]) -> int:
    if not argv:
        print("usage: test_nonempty.py COMMAND [ARG ...]", file=sys.stderr)
        return 2
    passed = 0
    with subprocess.Popen(
        argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True
    ) as child:
        assert child.stdout is not None
        for line in child.stdout:
            print(line, end="", flush=True)
            match = SUMMARY.match(line)
            if match:
                passed += int(match[1])
        status = child.wait()
    if status:
        return status if status > 0 else 128 - status
    if not passed:
        print("Rust test command passed but executed zero tests", file=sys.stderr)
        return 1
    print(f"Verified {passed} passing Rust tests")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
