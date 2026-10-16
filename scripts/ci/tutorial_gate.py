#!/usr/bin/env python3
"""Execute tutorial shell blocks and compare their documented stdout exactly."""

import argparse
from dataclasses import dataclass
import difflib
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[2]
PLACEHOLDERS = {
    "session_hex": r"[0-9a-f]{8}",
    "session_a": r"[0-9a-f]{8}",
    "session_b": r"[0-9a-f]{8}",
    "original_address": re.escape("alice@example.invalid"),
}
MANUAL = re.compile(r'<!-- tutorial-gate: manual reason="([^"\n]+)" -->')
START = "<!-- tutorial-gate: start=checkout -->"
ACCOUNT_COMMAND = re.compile(
    r"(?m)^\s*(?:(?:command|env)\s+|[A-Za-z_][A-Za-z0-9_]*=\S+\s+)*claude(?:\s|$)"
)


class GateError(ValueError):
    pass


def manual_allowlist():
    entries = json.loads(Path(__file__).with_name("tutorial_manual_steps.json").read_text())
    if not isinstance(entries, list):
        raise GateError("manual-step allowlist must be a list")
    seen = set()
    for entry in entries:
        if (not isinstance(entry, dict) or set(entry) != {"page", "sha256", "reason"}
                or not all(isinstance(value, str) and value.strip() for value in entry.values())
                or Path(entry["page"]).name != entry["page"]
                or not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"])):
            raise GateError("invalid manual-step allowlist entry")
        key = (entry["page"], entry["sha256"])
        if key in seen:
            raise GateError("duplicate manual-step approval")
        seen.add(key)
    return entries


@dataclass
class Block:
    line: int
    command: str
    expected: str | None = None
    manual: str | None = None


def expectation_pattern(expected: str, bindings: dict[str, str]):
    parts = []
    names = set()
    end = 0
    for match in re.finditer(r"\{([^{}\n]*)\}", expected):
        parts.append(re.escape(expected[end:match.start()]))
        name = match[1]
        if name not in PLACEHOLDERS:
            raise GateError(f"unknown output placeholder {{{name}}}")
        if name in bindings:
            parts.append(re.escape(bindings[name]))
        elif name in names:
            parts.append(f"(?P={name})")
        else:
            names.add(name)
            parts.append(f"(?P<{name}>{PLACEHOLDERS[name]})")
        end = match.end()
    parts.append(re.escape(expected[end:]))
    return re.compile("".join(parts))


def parse(markdown: str, page_name: str = "", approved_steps=()) -> tuple[list[Block], bool]:
    blocks = []
    pending_manual = None
    previous = None
    approved = {(entry["sha256"], entry["reason"]) for entry in approved_steps
                if entry["page"] == page_name}
    seen_manual = set()
    lines = markdown.splitlines(keepends=True)
    i = 0
    while i < len(lines):
        line = lines[i].rstrip("\n")
        marker = MANUAL.fullmatch(line)
        if marker:
            if pending_manual:
                raise GateError("unused manual marker")
            pending_manual = marker[1]
        elif line.startswith("<!-- tutorial-gate:") and line != START:
            raise GateError(f"invalid tutorial-gate marker at line {i + 1}")
        elif line.startswith("```"):
            language = line[3:].strip()
            number = i + 1
            i += 1
            body = []
            while i < len(lines) and lines[i].rstrip("\n") != "```":
                body.append(lines[i])
                i += 1
            if i == len(lines):
                raise GateError(f"unclosed fence at line {number}")
            content = "".join(body)
            if language == "sh":
                account = ACCOUNT_COMMAND.search(content)
                if account and not pending_manual:
                    raise GateError(f"account-bound claude command at line {number} needs a manual marker")
                if pending_manual:
                    key = (hashlib.sha256(content.encode()).hexdigest(), pending_manual)
                    if key not in approved:
                        raise GateError(f"manual marker at line {number} is not approved for this page and block")
                    if not account:
                        raise GateError("approved manual block must invoke an account command")
                    if key in seen_manual:
                        raise GateError("approved manual block appears more than once")
                    seen_manual.add(key)
                blocks.append(Block(number, content, manual=pending_manual))
                pending_manual = None
                previous = blocks[-1]
            elif language == "text":
                if previous is None:
                    raise GateError(f"expected output at line {number} has no preceding sh block")
                expectation_pattern(content, {})
                previous.expected = content
                previous = None
            else:
                previous = None
        elif pending_manual and line.strip():
            raise GateError("manual marker must be immediately before its sh fence")
        i += 1
    if pending_manual:
        raise GateError("unused manual marker")
    if seen_manual != approved:
        raise GateError("approved manual step is missing or changed")
    if not blocks or all(block.manual for block in blocks):
        raise GateError("tutorial must execute at least one sh block")
    return blocks, START in markdown.splitlines()


def compare(expected: str, actual: str, bindings: dict[str, str]):
    matched = expectation_pattern(expected, bindings).fullmatch(actual)
    if not matched:
        diff = "".join(difflib.unified_diff(
            expected.splitlines(keepends=True), actual.splitlines(keepends=True),
            fromfile="documented stdout", tofile="actual stdout",
        ))
        raise GateError(f"stdout mismatch\n{diff}")
    bindings.update(matched.groupdict())


def checkout_files(root: Path, destination: Path):
    """Copy the current tracked source, including uncommitted edits, without target/.git."""
    paths = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).decode().split("\0")
    for name in filter(None, paths):
        source = root / name
        if source.is_file():
            target = destination / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)


def run_tutorial(page: Path, root: Path, scratch: Path, gaze: Path, timeout: int = 600):
    blocks, checkout = parse(page.read_text(), page.name, manual_allowlist())
    started = time.monotonic()
    # Keep cargo new outside the repository's workspace ancestry. Otherwise it
    # silently adds the tutorial project to the real workspace Cargo.toml.
    with tempfile.TemporaryDirectory(prefix="gaze-tutorial-" + page.stem + "-") as temp:
        work = Path(temp)
        cwd = work / "work"
        cwd.mkdir()
        if checkout:
            checkout_files(root, cwd)
        home = work / "home"
        home.mkdir()
        binary_dir = work / "bin"
        binary_dir.mkdir()
        (binary_dir / "gaze").symlink_to(gaze)
        account_shim = binary_dir / "claude"
        account_attempt = work / "account-attempt"
        account_shim.write_text(
            "#!/bin/sh\nprintf '%s\\n' 'unmarked account command: claude' >&2\n"
            f": > {shlex.quote(str(account_attempt))}\nexit 126\n"
        )
        account_shim.chmod(0o700)
        env = os.environ.copy()
        env.update({
            "PATH": str(binary_dir) + os.pathsep + env["PATH"],
            "HOME": str(home), "XDG_CONFIG_HOME": str(home / "config"),
            "XDG_DATA_HOME": str(home / "data"), "XDG_CACHE_HOME": str(home / "cache"),
            # Keep Cargo's already downloaded registry and selected toolchain.
            "CARGO_HOME": os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")),
            "RUSTUP_HOME": os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup")),
            "CARGO_TARGET_DIR": str(scratch / "cargo"),
        })
        # cargo add is the sole adapted tutorial command: test the PR's crates,
        # never a published version. All other arguments go to the real Cargo.
        source = shlex.quote(str(root))
        script = ["set -euo pipefail\n", f"""cargo() {{
    if [[ "$#" == 3 && "$1" == add && "$2" == gaze-pii && "$3" == gaze-assembly ]]; then
        command cargo add gaze-pii --path {source}/crates/gaze
        command cargo add gaze-assembly --path {source}/crates/gaze-assembly
    else
        command cargo "$@"
    fi
}}
"""]
        completed = work / "completed"
        for index, block in enumerate(blocks):
            if block.manual:
                print(f"SKIP {page.name}:{block.line}: {block.manual}", flush=True)
                continue
            script += [f"exec > {shlex.quote(str(work / f'{index}.stdout'))}\n", block.command,
                       f"printf '%s\\n' {index} >> {shlex.quote(str(completed))}\n"]
        process = subprocess.Popen(
            ["bash", "--noprofile", "--norc"], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=cwd, env=env, start_new_session=True,
        )
        try:
            _, errors = process.communicate("".join(script).encode(), timeout=timeout)
        except subprocess.TimeoutExpired as error:
            raise GateError(f"{page.name}: exceeded {timeout}s") from error
        finally:
            # Includes descendants if a tutorial forgets its cleanup or times out.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
        done = set(completed.read_text().splitlines()) if completed.exists() else set()
        executable = [i for i, block in enumerate(blocks) if not block.manual]
        if account_attempt.exists():
            raise GateError(f"{page.name}: unmarked account command: claude")
        if process.returncode or done != set(map(str, executable)):
            failed = next((blocks[i].line for i in executable if str(i) not in done), "exit")
            raise GateError(f"{page.name}:{failed}: shell failed ({process.returncode})\n{errors.decode(errors='replace')}")
        bindings = {}
        checked = 0
        for index in executable:
            block = blocks[index]
            if block.expected is not None:
                try:
                    compare(block.expected, (work / f"{index}.stdout").read_bytes().decode("utf-8"), bindings)
                except GateError as error:
                    raise GateError(f"{page.name}:{block.line}: {error}") from error
                checked += 1
        result = {"tutorial": page.name, "executed": len(executable), "checked": checked,
                  "skipped": sum(bool(b.manual) for b in blocks),
                  "seconds": round(time.monotonic() - started, 3)}
        print(json.dumps(result), flush=True)
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--gaze", type=Path, required=True, help="fresh PR-head gaze binary with mcp + proxy")
    parser.add_argument("--page", help="run one tutorial filename, for local diagnosis")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    scratch = root / "target/tutorial-gate"
    scratch.mkdir(parents=True, exist_ok=True)
    pages = sorted((root / "docs/tutorials").glob("*.md"))
    pages = [page for page in pages if page.name != "README.md"]
    if not {entry["page"] for entry in manual_allowlist()} <= {page.name for page in pages}:
        parser.error("manual-step allowlist names a missing tutorial")
    if args.page:
        pages = [page for page in pages if page.name == args.page]
    if not pages:
        parser.error("no tutorials selected")
    started = time.monotonic()
    report = {"tutorials": []}
    try:
        for page in pages:
            report["tutorials"].append(run_tutorial(page, root, scratch, args.gaze.resolve()))
    except (GateError, OSError) as error:
        parser.exit(1, f"tutorial gate: {error}\n")
    report["seconds"] = round(time.monotonic() - started, 3)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"PASS {len(pages)} tutorials in {report['seconds']}s")


if __name__ == "__main__":
    main()
