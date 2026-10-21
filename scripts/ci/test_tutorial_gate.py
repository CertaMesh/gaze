import os
import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import tutorial_gate as gate


class OutputContract(unittest.TestCase):
    def test_only_pinned_account_blocks_can_be_manual(self):
        command = "claude --print hi\n"
        approved = [{"page": "account.md", "sha256": hashlib.sha256(command.encode()).hexdigest(),
                     "reason": "Account"}]
        markdown = ('```sh\nprintf "setup\\n"\n```\n'
                    '<!-- tutorial-gate: manual reason="Account" -->\n```sh\n' + command + '```\n')
        blocks, _ = gate.parse(markdown, "account.md", approved)
        self.assertEqual(sum(bool(block.manual) for block in blocks), 1)
        for content, name in ((markdown, "other.md"),
                              (markdown.replace("--print hi", "--print changed"), "account.md"),
                              (markdown.replace('reason="Account"', 'reason="Different"'), "account.md"),
                              ('```sh\nprintf "setup\\n"\n```\n', "account.md")):
            with self.subTest(name=name, content=content), self.assertRaises(gate.GateError):
                gate.parse(content, name, approved)
        ordinary = "printf 'ordinary\\n'\n"
        approved[0]["sha256"] = hashlib.sha256(ordinary.encode()).hexdigest()
        with self.assertRaisesRegex(gate.GateError, "must invoke an account command"):
            gate.parse(markdown.replace(command, ordinary), "account.md", approved)

    def test_account_wrappers_and_runtime_indirection_never_call_real_cli(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            fake = fake_bin / "claude"
            fake.write_text('#!/bin/sh\n: > "$ACCOUNT_CALLED"\n')
            fake.chmod(0o700)
            called = root / "called"
            page = root / "wrappers.md"
            calls = ["command claude --print hi", "env claude --print hi", "DEMO=x claude --print hi",
                     'account=claude\ncommand "$account" --print hi',
                     'account=claude\nenv "$account" --print hi',
                     'account=claude\nDEMO=x "$account" --print hi',
                     'account=claude\n"$account" --print hi || true']
            with patch.dict(os.environ, {"PATH": str(fake_bin) + os.pathsep + os.environ["PATH"],
                                         "ACCOUNT_CALLED": str(called)}):
                for call in calls:
                    with self.subTest(call=call):
                        page.write_text('```sh\n' + call + '\n```\n')
                        with self.assertRaisesRegex(gate.GateError, "needs a manual marker|unmarked account command"):
                            gate.run_tutorial(page, root, root, Path("/bin/true"))
                        self.assertFalse(called.exists(), "account executable must never run")

    def test_checked_background_failure_fails_the_page(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            page = root / "background.md"
            page.write_text('```sh\nfalse >/dev/null 2>&1 &\njob_pid=$!\nwait "$job_pid"\n'
                            'printf "done\\n"\n```\n```text\ndone\n```\n')
            with self.assertRaisesRegex(gate.GateError, "shell failed"):
                gate.run_tutorial(page, root, root, Path("/bin/true"))

    def test_exact_output_and_session_binding_across_blocks(self):
        bindings = {}
        gate.compare("<{session_hex}:Email_1>\n", "<012abcde:Email_1>\n", bindings)
        gate.compare("Again {session_hex}\n", "Again 012abcde\n", bindings)
        for actual in ("Again 87654321\n", "Again 012abcde", " Again 012abcde\n", "Again 012abcde\r\n"):
            with self.subTest(actual=actual), self.assertRaises(gate.GateError):
                gate.compare("Again {session_hex}\n", actual, bindings)

    def test_placeholder_contract_rejects_unknown_names_and_bad_hex(self):
        with self.assertRaisesRegex(gate.GateError, "unknown output placeholder"):
            gate.parse("```sh\necho hi\n```\n```text\n{surprise}\n```\n")
        for value in ("ABCDEF12", "012345678", "abcdefg1"):
            with self.subTest(value=value), self.assertRaises(gate.GateError):
                gate.compare("{session_hex}\n", value + "\n", {})

    def test_manual_marker_and_nonzero_execution_count_are_required(self):
        for markdown in ("# Tutorial\n", '```sh\nclaude --print hi\n```\n',
                         '<!-- tutorial-gate: manual reason="Account" -->\n```sh\nclaude\n```\n'):
            with self.subTest(markdown=markdown), self.assertRaises(gate.GateError):
                gate.parse(markdown)

    def test_persistent_shell_and_failures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            page = root / "tutorial.md"
            page.write_text('```sh\nmkdir demo\ncd demo\nvalue=retained\n```\n'
                            '```sh\nprintf "%s\\n" "$value"\npwd > cwd.txt\n```\n'
                            '```text\nretained\n```\n')
            result = gate.run_tutorial(page, root, root, Path("/bin/true"))
            self.assertEqual((result["executed"], result["checked"]), (2, 1))
            page.write_text('```sh\nfalse\nprintf "must not run\\n"\n```\n')
            with self.assertRaisesRegex(gate.GateError, "shell failed"):
                gate.run_tutorial(page, root, root, Path("/bin/true"))
            page.write_text('```sh\nprintf "retained\\r\\n"\n```\n```text\nretained\n```\n')
            with self.assertRaisesRegex(gate.GateError, "stdout mismatch"):
                gate.run_tutorial(page, root, root, Path("/bin/true"))

    def test_cargo_new_does_not_mutate_source_workspace(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / "Cargo.toml"
            original = '[workspace]\nmembers = []\n'
            manifest.write_text(original)
            page = root / "tutorial.md"
            page.write_text('```sh\ncargo new sample --bin --vcs none\n```\n')
            gate.run_tutorial(page, root, root, Path("/bin/true"))
            self.assertEqual(manifest.read_text(), original)


@unittest.skipUnless(os.environ.get("TUTORIAL_GATE_BINARY"), "set TUTORIAL_GATE_BINARY for real tutorial mutations")
class RealTutorialMutations(unittest.TestCase):
    def test_proxy_rejects_failed_background_mock(self):
        original = (gate.ROOT / "docs/tutorials/proxy-sdk.md").read_text()
        broken = original.replace("python3 mock_openai.py > mock.log 2>&1 &", "false > mock.log 2>&1 &")
        self.assertNotEqual(original, broken)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            page = root / "proxy-sdk.md"
            page.write_text(broken)
            with self.assertRaisesRegex(gate.GateError, "mock server did not start"):
                gate.run_tutorial(page, gate.ROOT, root, Path(os.environ["TUTORIAL_GATE_BINARY"]).resolve())

    def test_baseline_and_real_page_mutations(self):
        binary = Path(os.environ["TUTORIAL_GATE_BINARY"]).resolve()
        first = (gate.ROOT / "docs/tutorials/first-pseudonymization.md").read_text()
        mcp = (gate.ROOT / "docs/tutorials/mcp-coding-assistant.md").read_text()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            page = root / "first-pseudonymization.md"
            page.write_text(first)
            self.assertGreater(gate.run_tutorial(page, root, root, binary)["executed"], 0)
            mutations = [
                (first.replace("Byte-exact restore: OK\n```", "WRONG OUTPUT\n```"), "stdout mismatch"),
                (first.replace("gaze clean --session-out", "gaze nonexistent-tutorial-command --session-out"), "shell failed"),
                (gate.MANUAL.sub("", mcp), "needs a manual marker"),
            ]
            fences = first.split("```sh\n")
            marker = '<!-- tutorial-gate: manual reason="Account" -->\n'
            arbitrary_skips = fences[0] + '```sh\n' + fences[1]
            arbitrary_skips += ''.join(marker + '```sh\n' + fence for fence in fences[2:])
            mutations.append((arbitrary_skips, "not approved"))
            for content, failure in mutations:
                with self.subTest(failure=failure):
                    page.write_text(content)
                    with self.assertRaisesRegex(gate.GateError, failure):
                        gate.run_tutorial(page, root, root, binary)


if __name__ == "__main__":
    unittest.main()
