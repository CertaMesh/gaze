"""Select the Markdown-only PR lane; any uncertainty selects full CI."""

import json
import os
from pathlib import Path
import re
import subprocess
import sys


def markdown_diff(raw: bytes) -> bool:
    # Raw, NUL-delimited records preserve unusual filenames and expose file modes.
    if not raw or not raw.endswith(b"\0"):
        return False
    fields = raw[:-1].split(b"\0")
    if len(fields) % 2:
        return False
    for header, path in zip(fields[::2], fields[1::2]):
        match = re.fullmatch(
            rb":(000000|100644|100755) (000000|100644|100755) "
            rb"[0-9a-f]+ [0-9a-f]+ ([AMD])", header
        )
        # Renames/copies, type changes, submodules and symlinks take full CI.
        if not match or not path.endswith(b".md") or path.startswith(b".github/"):
            return False
    return True


def docs_only(event_name: str, event: dict, repo: Path) -> bool:
    if event_name != "pull_request":
        return False
    try:
        pr = event["pull_request"]
        base, head = pr["base"]["sha"], pr["head"]["sha"]
        if not all(isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{40}", sha)
                   for sha in (base, head)):
            return False
        raw = subprocess.check_output(
            ["git", "diff", "--raw", "-z", "--no-abbrev", "--find-renames",
             "--find-copies", "--find-copies-harder", f"{base}...{head}", "--"],
            cwd=repo, stderr=subprocess.PIPE,
        )
        return markdown_diff(raw)
    except (KeyError, TypeError, OSError, subprocess.CalledProcessError):
        return False


def main() -> None:
    result = False
    try:
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        result = docs_only(os.environ.get("GITHUB_EVENT_NAME", ""), event, Path.cwd())
    except (KeyError, OSError, ValueError):
        pass
    print(f"docs_only={str(result).lower()}")


if __name__ == "__main__":
    main()
