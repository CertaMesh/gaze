import collections
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import classify_changes as changes
import required_checks


POLICY_FILES = ("scripts/ci/classify_changes.py", "scripts/ci/test_classify_changes.py")


def tracked_markdown_consumers(repo: Path, head: str) -> dict[str, set[str]]:
    """Conservatively resolve quoted Markdown filenames, including joined paths.

    A bare filename can resolve to more than one tracked file. Keep every match
    rather than assuming which root a metadata/fixture helper uses. The policy
    inventory and its synthetic fixtures are not themselves Markdown readers.
    """
    tracked = subprocess.check_output(
        ["git", "ls-tree", "-r", "--name-only", "-z", head], cwd=repo,
    ).split(b"\0")
    by_name = collections.defaultdict(set)
    for path in tracked:
        if path.endswith(b".md"):
            name = os.fsdecode(path)
            by_name[Path(name).name].add(name)
    sources = subprocess.run(
        ["git", "grep", "-I", "-l", "-z", "-F", ".md", head, "--",
         "crates", "scripts", ":!*.md", *(f":!{path}" for path in POLICY_FILES)],
        cwd=repo, capture_output=True, check=False,
    )
    if sources.returncode not in (0, 1):
        raise RuntimeError(sources.stderr.decode(errors="replace"))
    consumers = collections.defaultdict(set)
    for source in sources.stdout.split(b"\0"):
        if not source:
            continue
        source_path = os.fsdecode(source).removeprefix(f"{head}:")
        raw = subprocess.check_output(["git", "show", f"{head}:{source_path}"], cwd=repo)
        for literal in re.findall(rb'''["']([A-Za-z0-9_./-]+\.md)["']''', raw):
            for document in by_name.get(Path(os.fsdecode(literal)).name, ()):
                consumers[document].add(source_path)
    return dict(consumers)


class ChangeClassification(unittest.TestCase):
    def test_every_tracked_markdown_consumer_selects_full_ci(self):
        repo = Path(subprocess.check_output(
            ["git", "rev-parse", "--show-toplevel"], cwd=Path(__file__).parent, text=True,
        ).strip())
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
        consumers = tracked_markdown_consumers(repo, head)
        self.assertTrue({"docs/reference/policy.md", "docs/how-to/rust-library.md",
                         "docs/reference/redaction-classes.md", "CHANGELOG.md"} <= consumers.keys())
        constructed = set()
        for document in consumers:
            result = subprocess.run(
                ["git", "grep", "-q", "-F", "-e", document, head, "--", ":!*.md",
                 *(f":!{path}" for path in POLICY_FILES)], cwd=repo,
            )
            self.assertIn(result.returncode, (0, 1))
            if result.returncode == 1:
                constructed.add(os.fsencode(document))
        self.assertEqual(
            changes.CONSTRUCTED_MARKDOWN_PATHS, constructed,
            "Joined Markdown inputs changed: review and update CONSTRUCTED_MARKDOWN_PATHS",
        )
        event = {"pull_request": {"base": {"sha": head}, "head": {"sha": head}}}
        original_output = subprocess.check_output
        for document, readers in sorted(consumers.items()):
            raw = b":100644 100644 a b M\0" + os.fsencode(document) + b"\0"

            def diff_input(command, *args, **kwargs):
                if command[:2] == ["git", "diff"]:
                    return raw
                return original_output(command, *args, **kwargs)

            # Supply one changed-file record; use the real Git commit trees for
            # reference lookup, including every current include/read target.
            with self.subTest(document=document, readers=sorted(readers)), \
                    patch.object(subprocess, "check_output", side_effect=diff_input):
                self.assertFalse(changes.docs_only("pull_request", event, repo))

    def test_real_merge_base_diffs(self):
        cases = [
            ("md-only", {"README.md": "new text", "docs/new.md": "new"}, True),
            ("mixed", {"README.md": "new", "src/lib.rs": "new"}, False),
            ("github-only", {".github/workflows/test.yml": "new"}, False),
            ("github-markdown", {".github/README.md": "new"}, False),
            ("renamed-md-to-rs", {"README.md": None, "renamed.rs": "base"}, False),
            ("renamed-md", {"README.md": None, "renamed.md": "base"}, False),
            ("copied-md", {"copied.md": "base"}, False),
            ("deleted-md", {"README.md": None}, True),
            ("empty", {}, False),
            ("uppercase-extension", {"README.MD": "new"}, False),
            ("unusual-paths", {"docs/tab\tand\nnewline.md": "new"}, True),
            ("deleted-rs", {"src/lib.rs": None}, False),
            ("policy-md", {"docs/reference/policy.md": "broken policy example"}, False),
            ("changelog", {"CHANGELOG.md": "changed links"}, False),
            ("rust-library", {"docs/how-to/rust-library.md": "changed example"}, False),
            ("redaction-classes", {"docs/reference/redaction-classes.md": "changed rows"}, False),
            ("deleted-coupled-md", {"docs/reference/policy.md": None}, False),
            ("constructed-readme", {"crates/gaze-assembly/README.md": "changed"}, False),
            ("constructed-fixture", {"crates/xtask/fixtures/scrub_public_text/dirty.md": "changed"}, False),
            ("plain-tutorial", {"docs/tutorials/plain.md": "new tutorial"}, True),
        ]
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp)
            # Real signed fixture commits without relying on a developer's signing key.
            key = repo / "fixture-key"
            subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)],
                           check=True, capture_output=True)

            def git(*args):
                return subprocess.check_output(["git", *args], cwd=repo,
                                               stderr=subprocess.PIPE).decode().strip()

            git("init", "-b", "base")
            for option, value in {
                "user.name": "CI Fixture", "user.email": "fixture@example.invalid",
                "gpg.format": "ssh", "user.signingkey": str(key), "commit.gpgsign": "true",
            }.items():
                git("config", option, value)
            files = {
                "README.md": "base", "src/lib.rs": "base code",
                "docs/reference/policy.md": "policy", "CHANGELOG.md": "history",
                "docs/how-to/rust-library.md": "Rust example",
                "docs/reference/redaction-classes.md": "class table",
                "docs/links.md": "[Plain tutorial](docs/tutorials/plain.md)",
                "src/readers.rs": '\n'.join(
                    f'const DOC_{index}: &str = include_str!("../../../{path}");'
                    for index, path in enumerate(("docs/reference/policy.md", "CHANGELOG.md",
                                                  "docs/how-to/rust-library.md",
                                                  "docs/reference/redaction-classes.md"))
                ),
            }
            for path, text in files.items():
                target = repo / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(text)
            git("add", *files)
            git("commit", "-S", "-s", "-m", "Fixture base")
            self.assertIn("gpgsig ", git("cat-file", "-p", "HEAD"))
            base = git("rev-parse", "HEAD")
            for name, edits, expected in cases:
                with self.subTest(name=name):
                    git("checkout", "-B", "candidate", base)
                    for path, text in edits.items():
                        target = repo / path
                        if text is None:
                            target.unlink()
                        else:
                            target.parent.mkdir(parents=True, exist_ok=True)
                            target.write_text(text)
                        git("add", "--", path)
                    git("commit", "-S", "-s", "--allow-empty", "-m", "Fixture candidate")
                    self.assertIn("gpgsig ", git("cat-file", "-p", "HEAD"))
                    head = git("rev-parse", "HEAD")
                    event = {"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}
                    self.assertEqual(changes.docs_only("pull_request", event, repo), expected)
                    event_path = repo / "event.json"
                    event_path.write_text(json.dumps(event))
                    output = subprocess.check_output(
                        [sys.executable, str(Path(changes.__file__).resolve())], cwd=repo,
                        env={**os.environ, "GITHUB_EVENT_NAME": "pull_request",
                             "GITHUB_EVENT_PATH": str(event_path)},
                    )
                    self.assertEqual(output.decode().strip(), f"docs_only={str(expected).lower()}")
                    if name == "md-only":
                        # Advance the base with code: only base...head remains Markdown-only.
                        git("checkout", "-B", "advanced", base)
                        (repo / "src/lib.rs").write_text("base advanced")
                        git("add", "src/lib.rs")
                        git("commit", "-S", "-s", "-m", "Advance base")
                        self.assertIn("gpgsig ", git("cat-file", "-p", "HEAD"))
                        event["pull_request"]["base"]["sha"] = git("rev-parse", "HEAD")
                        self.assertTrue(changes.docs_only("pull_request", event, repo))
                        (repo / "src/lib.rs").write_text('include_str!("../README.md")')
                        git("add", "src/lib.rs")
                        git("commit", "-S", "-s", "-m", "Add base-side Markdown consumer")
                        self.assertIn("gpgsig ", git("cat-file", "-p", "HEAD"))
                        event["pull_request"]["base"]["sha"] = git("rev-parse", "HEAD")
                        self.assertFalse(changes.docs_only("pull_request", event, repo))
                    if name == "plain-tutorial":
                        (repo / "src/lib.rs").write_text('include_str!("docs/tutorials/plain.md")')
                        self.assertTrue(changes.docs_only("pull_request", event, repo),
                                        "lookup must use commit trees, not dirty workspace files")
                        git("checkout", "--", "src/lib.rs")

    def test_non_pr_and_invalid_events_take_full_ci(self):
        event = {"pull_request": {"base": {"sha": "a" * 40}, "head": {"sha": "b" * 40}}}
        for name in ("push", "merge_group", "workflow_dispatch", "schedule", "pull_request_target", ""):
            with self.subTest(event=name), patch.object(subprocess, "check_output") as git:
                self.assertFalse(changes.docs_only(name, event, Path.cwd()))
                git.assert_not_called()
        for invalid in ({}, None, [], {"pull_request": None},
                        {"pull_request": {"base": {"sha": "--help"}, "head": {"sha": "b" * 40}}}):
            with self.subTest(invalid=invalid):
                self.assertFalse(changes.docs_only("pull_request", invalid, Path.cwd()))
        for error in (OSError("git missing"), subprocess.CalledProcessError(128, "git diff")):
            with patch.object(subprocess, "check_output", side_effect=error):
                self.assertFalse(changes.docs_only("pull_request", event, Path.cwd()))

    def test_malformed_diff_and_special_file_modes_take_full_ci(self):
        for raw in (b"", b"M\0README.md", b"M\0README.md\0", b"M\0a\0extra\0",
                    b":120000 120000 a b M\0link.md\0",
                    b":160000 160000 a b M\0submodule.md\0",
                    b":100644 100644 a b R100\0old.md\0new.md\0"):
            with self.subTest(raw=raw):
                self.assertFalse(changes.markdown_paths(raw))

    def test_reference_lookup_errors_take_full_ci(self):
        event = {"pull_request": {"base": {"sha": "a" * 40}, "head": {"sha": "b" * 40}}}
        raw = b":100644 100644 a b M\0guide.md\0"
        for status in (0, 2, -9):
            with patch.object(subprocess, "check_output", return_value=raw), \
                    patch.object(subprocess, "run", return_value=subprocess.CompletedProcess([], status)):
                self.assertFalse(changes.docs_only("pull_request", event, Path.cwd()))
        with patch.object(subprocess, "check_output", return_value=raw), \
                patch.object(subprocess, "run", side_effect=OSError("grep unavailable")):
            self.assertFalse(changes.docs_only("pull_request", event, Path.cwd()))

    def test_cli_errors_emit_false(self):
        for content in (None, "{broken", "null", "{}"):
            with tempfile.TemporaryDirectory() as temp:
                event = Path(temp) / "event.json"
                if content is not None:
                    event.write_text(content)
                output = subprocess.check_output(
                    [sys.executable, str(Path(changes.__file__).resolve())],
                    env={**os.environ, "GITHUB_EVENT_NAME": "pull_request", "GITHUB_EVENT_PATH": str(event)},
                )
                self.assertEqual(output, b"docs_only=false\n")


class RequiredContexts(unittest.TestCase):
    def test_full_and_docs_results_reject_any_missing_or_failed_owner(self):
        for check, owners in required_checks.OWNERS.items():
            for docs in (False, True):
                needs = {owner: {"result": "skipped" if docs else "success"} for owner in owners}
                needs["changes"] = {"result": "success", "outputs": {"docs_only": str(docs).lower()}}
                if check == "xtask":
                    needs["test"] = {"result": "success"}
                    needs["markdown-gates"] = {"result": "success" if docs else "skipped"}
                required_checks.require_success(check, needs)
                for owner in needs:
                    for result in ("failure", "cancelled", "skipped", "success", ""):
                        if result == needs[owner]["result"]:
                            continue
                        broken = json.loads(json.dumps(needs))
                        broken[owner]["result"] = result
                        with self.subTest(check=check, docs=docs, owner=owner, result=result):
                            with self.assertRaises(ValueError):
                                required_checks.require_success(check, broken)
                    missing = {key: value for key, value in needs.items() if key != owner}
                    with self.assertRaises((KeyError, ValueError)):
                        required_checks.require_success(check, missing)
                extra = {**needs, "unregistered": {"result": "success"}}
                with self.assertRaises(ValueError):
                    required_checks.require_success(check, extra)


if __name__ == "__main__":
    unittest.main()
