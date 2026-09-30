#!/usr/bin/env python3
"""Fail-closed binary ownership, libtest equivalence, and executed-test evidence."""

import argparse
from collections import Counter
import json
import os
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
ROSTER = ROOT / "scripts/ci/test-shards.json"
SHARDS = ("cli", "runtime", "gates")


def load_roster(path=ROSTER):
    roster = json.loads(path.read_text())
    if set(roster) != set(SHARDS):
        raise ValueError("roster must contain exactly the configured shards")
    owners = Counter(binary for binaries in roster.values() for binary in binaries)
    if not owners or any(count != 1 for count in owners.values()):
        raise ValueError("each test binary must have exactly one owner")
    if any(not binaries for binaries in roster.values()):
        raise ValueError("each shard must own at least one binary")
    return roster


def validate_inventory(inventory, roster):
    suites = inventory["rust-suites"]
    expected = {binary for binaries in roster.values() for binary in binaries}
    if set(suites) != expected:
        raise ValueError(
            f"unassigned binaries: {sorted(set(suites) - expected)}; "
            f"stale assignments: {sorted(expected - set(suites))}"
        )
    if any(suite["status"] != "listed" for suite in suites.values()):
        raise ValueError("every binary must be successfully listed, including empty binaries")


def filterset(roster, shard):
    return " | ".join(f"binary_id(={binary})" for binary in roster[shard])


def cargo_binary_id(message, packages):
    target = message["target"]
    package = packages[message["package_id"]]
    kind = target["kind"][0]
    if kind == "lib":
        return package
    if kind == "bin":
        return f"{package}::bin/{target['name']}"
    if kind == "test":
        return f"{package}::{target['name']}"
    raise ValueError(f"unexpected test harness kind: {kind}")


def libtest_names(executable, cwd, ignored=False):
    args = [executable, "--list", "--format", "terse"]
    if ignored:
        args.append("--ignored")
    result = subprocess.run(args, cwd=cwd, check=True, text=True, capture_output=True)
    names = set()
    for line in result.stdout.splitlines():
        if line.endswith(": test"):
            names.add(line.removesuffix(": test"))
        elif line.endswith(": benchmark"):
            # libtest executes #[bench] functions once in test mode.
            names.add(line.removesuffix(": benchmark"))
        elif line.strip():
            raise ValueError(f"unexpected libtest listing: {line!r}")
    return names


def normalized_inventory(inventory):
    return {
        binary: {name: case["ignored"] for name, case in suite["testcases"].items()}
        for binary, suite in inventory["rust-suites"].items()
    }


def collect(output):
    output.mkdir(parents=True, exist_ok=True)
    roster = load_roster()
    # Cargo is the independent baseline, using the original workspace selection.
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT
    ))
    packages = {package["id"]: package["name"] for package in metadata["packages"]}
    cargo = subprocess.run(
        ["cargo", "test", "--workspace", "--all-features", "--no-run", "--message-format=json"],
        cwd=ROOT, check=True, text=True, stdout=subprocess.PIPE,
    )
    artifacts = {}
    for line in cargo.stdout.splitlines():
        message = json.loads(line)
        if (message.get("reason") == "compiler-artifact"
                and message["profile"]["test"] and message.get("executable")):
            binary = cargo_binary_id(message, packages)
            if binary in artifacts:
                raise ValueError(f"duplicate Cargo test binary: {binary}")
            artifacts[binary] = message["executable"]
    inventory = json.loads(subprocess.check_output(
        ["cargo", "nextest", "list", "--workspace", "--all-features", "--message-format", "json"],
        cwd=ROOT,
    ))
    validate_inventory(inventory, roster)
    suites = inventory["rust-suites"]
    if set(artifacts) != set(suites):
        raise ValueError("Cargo and nextest must discover exactly the same test binaries")
    baseline = {}
    for binary, executable in sorted(artifacts.items()):
        suite = suites[binary]
        names = libtest_names(executable, suite["cwd"])
        ignored = libtest_names(executable, suite["cwd"], ignored=True)
        baseline[binary] = {name: name in ignored for name in sorted(names)}
    candidate = normalized_inventory(inventory)
    if baseline != candidate:
        raise ValueError("Cargo/libtest and nextest names or ignored status differ")
    (output / "before.json").write_text(json.dumps(baseline, indent=2, sort_keys=True) + "\n")
    (output / "after.json").write_text(json.dumps(candidate, indent=2, sort_keys=True) + "\n")
    (output / "ownership.json").write_text(json.dumps(roster, indent=2) + "\n")
    print(f"Coverage: {len(baseline)} binaries, "
          f"{sum(len(tests) for tests in baseline.values())} names, identical ignored status")


def executed_tests(path):
    timings = {}
    for suite in ET.parse(path).getroot().findall("testsuite"):
        for test in suite.findall("testcase"):
            if test.find("skipped") is not None:
                raise ValueError("a selected test was skipped")
            if test.find("failure") is not None or test.find("error") is not None:
                raise ValueError("a selected test failed")
            identity = (suite.attrib["name"], test.attrib["name"])
            if identity in timings:
                raise ValueError(f"a test executed more than once: {identity}")
            timings[identity] = float(test.attrib["time"])
    return timings


def prove(evidence, output):
    roster = load_roster()
    combined = {}
    baseline = None
    for shard in SHARDS:
        directory = evidence / shard
        before = json.loads((directory / "before.json").read_text())
        after = json.loads((directory / "after.json").read_text())
        if before != after or (baseline is not None and before != baseline):
            raise ValueError("shards disagree on before/after inventories")
        baseline = before
        if json.loads((directory / "ownership.json").read_text()) != roster:
            raise ValueError("shard used a different ownership roster")
        expected = {
            (binary, name)
            for binary in roster[shard]
            for name, ignored in before[binary].items() if not ignored
        }
        actual = executed_tests(directory / "junit.xml")
        if set(actual) != expected:
            raise ValueError(f"{shard}: missing {sorted(expected - set(actual))}; "
                             f"unexpected {sorted(set(actual) - expected)}")
        if set(combined) & set(actual):
            raise ValueError("a test ran in more than one shard")
        combined.update(actual)
    output.mkdir(parents=True, exist_ok=True)
    summary = [
        f"Coverage proof: {len(baseline)} binaries, {len(combined)} non-ignored tests, "
        "each executed exactly once. All names and ignored flags match Cargo/libtest.",
        "", "Top 20 workspace tests (nextest wall seconds, includes process startup):", "",
    ]
    for (binary, name), seconds in sorted(combined.items(), key=lambda row: row[1], reverse=True)[:20]:
        summary.append(f"- {seconds:.3f}s `{binary}::{name}`")
    report = "\n".join(summary) + "\n"
    (output / "summary.md").write_text(report)
    (output / "timings.json").write_text(json.dumps([
        {"binary": binary, "test": name, "seconds": seconds}
        for (binary, name), seconds in sorted(combined.items())
    ], indent=2) + "\n")
    print(report)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as stream:
            stream.write(report)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    listing = commands.add_parser("collect")
    listing.add_argument("--output", type=Path, required=True)
    selection = commands.add_parser("filter")
    selection.add_argument("shard", choices=SHARDS)
    proof = commands.add_parser("prove")
    proof.add_argument("--evidence", type=Path, required=True)
    proof.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "collect":
        collect(args.output)
    elif args.command == "filter":
        print(filterset(load_roster(), args.shard))
    else:
        prove(args.evidence, args.output)


if __name__ == "__main__":
    main()
