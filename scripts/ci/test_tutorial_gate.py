import os
from pathlib import Path
import tempfile
import unittest

import tutorial_gate as gate


class OutputContract(unittest.TestCase):
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
    def test_baseline_and_three_required_mutations(self):
        binary = Path(os.environ["TUTORIAL_GATE_BINARY"]).resolve()
        first = (gate.ROOT / "docs/tutorials/first-pseudonymization.md").read_text()
        mcp = (gate.ROOT / "docs/tutorials/mcp-coding-assistant.md").read_text()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            page = root / "tutorial.md"
            page.write_text(first)
            self.assertGreater(gate.run_tutorial(page, root, root, binary)["executed"], 0)
            mutations = [
                (first.replace("Byte-exact restore: OK\n```", "WRONG OUTPUT\n```"), "stdout mismatch"),
                (first.replace("gaze clean <", "gaze nonexistent-tutorial-command <"), "shell failed"),
                (gate.MANUAL.sub("", mcp), "needs a manual marker"),
            ]
            for content, failure in mutations:
                with self.subTest(failure=failure):
                    page.write_text(content)
                    with self.assertRaisesRegex(gate.GateError, failure):
                        gate.run_tutorial(page, root, root, binary)


if __name__ == "__main__":
    unittest.main()
