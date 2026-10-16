"""Select the Markdown-only PR lane; any uncertainty selects full CI."""

import json
import os
from pathlib import Path
import re
import subprocess


# Paths assembled by reader helpers rather than written as full repository paths.
# test_classify_changes inventories tracked consumers and pins this set.
CONSTRUCTED_MARKDOWN_PATHS = frozenset({
    b'crates/gaze-assembly/README.md',
    b'crates/gaze-audit/README.md',
    b'crates/gaze-cli/README.md',
    b'crates/gaze-document/README.md',
    b'crates/gaze-inspection/README.md',
    b'crates/gaze-mcp-bridge/README.md',
    b'crates/gaze-mcp-core/README.md',
    b'crates/gaze-mcp-rmcp/README.md',
    b'crates/gaze-model-setup/README.md',
    b'crates/gaze-proxy-dashboard/README.md',
    b'crates/gaze-proxy/README.md',
    b'crates/gaze-recognizers/README.md',
    b'crates/gaze-recognizers/assets/ner/README.md',
    b'crates/gaze-recognizers/testdata/coverage-loop/README.md',
    b'crates/gaze-token-bridge/README.md',
    b'crates/gaze-types/README.md',
    b'crates/gaze/README.md',
    b'crates/gaze/testdata/ner/README.md',
    b'crates/xtask/README.md',
    b'crates/xtask/fixtures/scrub_public_text/allowed_urls.md',
    b'crates/xtask/fixtures/scrub_public_text/allowlisted_host_free_path.md',
    b'crates/xtask/fixtures/scrub_public_text/clean.md',
    b'crates/xtask/fixtures/scrub_public_text/comparison_source_allowed.md',
    b'crates/xtask/fixtures/scrub_public_text/comparison_source_fragment.md',
    b'crates/xtask/fixtures/scrub_public_text/comparison_source_query.md',
    b'crates/xtask/fixtures/scrub_public_text/comparison_source_unlisted.md',
    b'crates/xtask/fixtures/scrub_public_text/dirty.md',
    b'crates/xtask/fixtures/scrub_public_text/lookalike_url.md',
    b'crates/xtask/fixtures/scrub_public_text/other_url.md',
    b'crates/xtask/fixtures/scrub_public_text/suffix_host_url.md',
    b'docs/README.md',
    b'docs/explanation/README.md',
    b'docs/how-to/README.md',
    b'docs/reference/README.md',
    b'docs/reference/benchmarks/gates/dictionary-cache-identity/README.md',
    b'docs/tutorials/README.md',
    b'lint/dylint/README.md',
    b'scripts/README.md',
    b'scripts/bench/compare/README.md',
    b'scripts/bench/compare/theirbench/README.md',
    b'scripts/bench/fixtures/jwt_payload/README.md',
})


def markdown_paths(raw: bytes) -> list[bytes]:
    # Raw, NUL-delimited records preserve unusual filenames and expose file modes.
    if not raw or not raw.endswith(b"\0"):
        return []
    fields = raw[:-1].split(b"\0")
    if len(fields) % 2:
        return []
    for header, path in zip(fields[::2], fields[1::2]):
        match = re.fullmatch(
            rb":(000000|100644|100755) (000000|100644|100755) "
            rb"[0-9a-f]+ [0-9a-f]+ ([AMD])", header
        )
        # Renames/copies, type changes, submodules and symlinks take full CI.
        if not match or not path.endswith(b".md") or path.startswith(b".github/"):
            return []
    return fields[1::2]


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
        paths = markdown_paths(raw)
        if not paths or CONSTRUCTED_MARKDOWN_PATHS.intersection(paths):
            return False
        patterns = [argument for path in paths for argument in ("-e", os.fsdecode(path))]
        references = subprocess.run(
            ["git", "grep", "-q", "-F", *patterns, head, base, "--", ":!*.md"],
            cwd=repo, stderr=subprocess.PIPE,
        )
        # Only grep's explicit "no match" permits the fast path. The base also
        # covers a reader added after this PR branched, before its merge.
        return references.returncode == 1
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
