import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path("scripts/gate/mcp-tier-isolation-mutation-probe.sh")
TARGETS = [Path("crates/gaze-mcp-core/src/tools/mod.rs"),
           Path("crates/gaze-mcp-core/src/lib.rs")]


class MutationProbe(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / SCRIPT).parent.mkdir(parents=True)
        shutil.copyfile(ROOT / SCRIPT, self.root / SCRIPT)
        for path in TARGETS:
            (self.root / path).parent.mkdir(parents=True, exist_ok=True)
            (self.root / path).write_text('#[cfg(feature = "operator-tier")]\npub mod example;\n')
        self.git("init", "-q")
        # Fixture commits are signed too, without depending on a developer key.
        key = self.root / "fixture-signing-key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)],
                       check=True, capture_output=True, timeout=60)
        self.git("config", "gpg.format", "ssh")
        self.git("config", "user.signingkey", str(key))
        self.git("add", *map(str, TARGETS))
        self.git("-c", "user.name=Fixture",
                 "-c", "user.email=fixture@example.invalid", "commit", "-S", "-s", "-qm", "Fixture")
        self.assertIn(b"gpgsig ", self.git("cat-file", "-p", "HEAD"))
        stub = self.root / "bin/cargo"
        stub.parent.mkdir()
        stub.write_text(f'#!{sys.executable}\n' + '''
import os
from pathlib import Path
import time
tools = Path("crates/gaze-mcp-core/src/tools/mod.rs")
mutated = 'cfg(feature' not in tools.read_text()
if mutated:
    Path("mutated-ready").touch()
    if os.environ.get("WAIT"):
        time.sleep(30)
    if os.environ.get("DIRTY_LIB"):
        with Path("crates/gaze-mcp-core/src/lib.rs").open("a") as f:
            f.write("// unrelated edit\\n")
    if os.environ.get("RESTORE_FAIL"):
        Path("restore-fail").touch()
if os.environ.get("GATE_FAIL"):
    raise SystemExit(7)
raise SystemExit(1 if mutated else 0)
''')
        stub.chmod(0o755)
        cat_stub = stub.parent / "cat"
        cat_stub.write_text(
            '#!/usr/bin/env bash\n'
            'if [ -f restore-fail ] && [[ "$1" == *gaze-tier-probe.* ]]; then exit 1; fi\n'
            f'exec "{shutil.which("cat")}" "$@"\n'
        )
        cat_stub.chmod(0o755)
        self.env = dict(os.environ, TMPDIR=str(self.root),
                        PATH=str(stub.parent) + os.pathsep + os.environ["PATH"])

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, timeout=60)

    def snapshot(self):
        return ((self.root / ".git/index").read_bytes(),
                tuple((self.root / p).read_bytes() for p in TARGETS))

    def run_probe(self, *cases, **env):
        return subprocess.run(["bash", str(SCRIPT), *cases], cwd=self.root,
                              env=dict(self.env, **env), capture_output=True, timeout=60)

    def test_dirty_sources_preserve_index_and_worktree(self):
        for path in TARGETS:
            with (self.root / path).open("a") as f:
                f.write("// staged edit\n")
        self.git("add", *map(str, TARGETS))
        for path in TARGETS:
            with (self.root / path).open("a") as f:
                f.write("// unstaged edit\n")
        before = self.snapshot()
        result = self.run_probe()
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(self.snapshot(), before)

    def test_invalid_later_case_makes_no_edits_or_gate_calls(self):
        before = self.snapshot()
        result = self.run_probe("deep-path", "invalid")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(self.snapshot(), before)
        self.assertFalse((self.root / "mutated-ready").exists())
        self.assertFalse((self.root / "target").exists())

    def test_gate_failure_restores_sources(self):
        before = self.snapshot()
        result = self.run_probe("full-surface", GATE_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.snapshot(), before)

    def make_clean_crlf_checkout(self):
        self.git("config", "core.autocrlf", "true")
        for path in TARGETS:
            (self.root / path).unlink()
        self.git("checkout", "--", *map(str, TARGETS))
        self.assertEqual(self.git("status", "--porcelain", "--", *map(str, TARGETS)), b"")
        for path in TARGETS:
            self.assertIn(b"\r\n", (self.root / path).read_bytes())

    def assert_snapshots_removed(self):
        self.assertEqual(list(self.root.glob("gaze-tier-probe.*")), [])
        self.assertEqual(list(self.root.rglob("*.probe-tmp.*")), [])

    def test_preparation_and_write_interrupts_preserve_exact_bytes(self):
        self.assert_preparation_and_write_interrupts()

    def test_clean_crlf_preparation_and_write_interrupts_preserve_exact_bytes(self):
        self.make_clean_crlf_checkout()
        self.assert_preparation_and_write_interrupts()

    def assert_preparation_and_write_interrupts(self):
        # Signal the shell itself after grep prepares the intermediate or cat
        # writes the source, before the shell can remove the intermediate.
        for command in ("grep", "cat"):
            real_command = shutil.which(command)
            wrapper = self.root / "bin" / command
            wrapper.write_text(f'#!{sys.executable}\n' + f'''
import os
from pathlib import Path
import signal
import subprocess
import sys
args = sys.argv[1:]
result = subprocess.run([{real_command!r}, *args])
target = os.environ.get("SIGNAL_TARGET", "")
preparation = {command!r} == "grep" and "-v" in args and target in args
write = ({command!r} == "cat" and args
         and Path(args[0]).name.startswith(Path(target).name + ".probe-tmp."))
if os.environ.get("SIGNAL_STAGE") == {command!r} and (preparation or write):
    os.kill(os.getppid(), int(os.environ["SIGNAL_NUMBER"]))
raise SystemExit(result.returncode)
''')
            wrapper.chmod(0o755)
        with tempfile.TemporaryDirectory() as logs:
            untracked = self.git("ls-files", "--others", "-z")
            for stage in ("grep", "cat"):
                for target in TARGETS:
                    for sig in (signal.SIGINT, signal.SIGTERM):
                        with self.subTest(stage=stage, target=target, signal=sig):
                            before = self.snapshot()
                            result = self.run_probe(
                                "full-surface", SIGNAL_STAGE=stage,
                                SIGNAL_TARGET=str(target), SIGNAL_NUMBER=str(int(sig)),
                                TIER_PROBE_LOG_DIR=logs,
                            )
                            self.assertEqual(result.returncode, 128 + sig,
                                             result.stdout + result.stderr)
                            self.assertEqual(self.snapshot(), before)
                            self.assert_snapshots_removed()
                            self.assertEqual(
                                self.git("ls-files", "--others", "-z"),
                                untracked,
                            )

    def test_clean_crlf_normal_run_preserves_exact_bytes(self):
        self.make_clean_crlf_checkout()
        before = self.snapshot()
        result = self.run_probe("full-surface")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.snapshot(), before)
        self.assert_snapshots_removed()

    def test_clean_crlf_interrupt_preserves_exact_bytes(self):
        self.make_clean_crlf_checkout()
        self.assert_interrupt_restores_sources()

    def test_interrupt_restores_sources(self):
        self.assert_interrupt_restores_sources()

    def assert_interrupt_restores_sources(self):
        for sig in (signal.SIGINT, signal.SIGTERM):
            with self.subTest(signal=sig):
                ready = self.root / "mutated-ready"
                ready.unlink(missing_ok=True)
                before = self.snapshot()
                process = subprocess.Popen(["bash", str(SCRIPT), "full-surface"],
                                           cwd=self.root, env=dict(self.env, WAIT="1"),
                                           stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                           start_new_session=True)
                try:
                    deadline = time.monotonic() + 60
                    while not ready.exists() and time.monotonic() < deadline:
                        time.sleep(0.02)
                    self.assertTrue(ready.exists(), "mutation gate did not start")
                    os.killpg(process.pid, sig)
                    process.communicate(timeout=60)
                    self.assertEqual(process.returncode, 128 + sig)
                    self.assertEqual(self.snapshot(), before)
                    self.assert_snapshots_removed()
                finally:
                    if process.poll() is None:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.communicate()

    def test_restore_failure_is_nonzero(self):
        result = self.run_probe("deep-path", RESTORE_FAIL="1")
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(b"FATAL: failed to restore", result.stderr)

    def test_deep_path_restores_only_owned_file(self):
        index, sources = self.snapshot()
        result = self.run_probe("deep-path", DIRTY_LIB="1")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        after_index, after_sources = self.snapshot()
        self.assertEqual(after_index, index)
        self.assertEqual(after_sources[0], sources[0])
        self.assertEqual(after_sources[1], sources[1] + b"// unrelated edit\n")

    def test_normal_run_preserves_gate_contract(self):
        before = self.snapshot()
        result = self.run_probe()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.snapshot(), before)
        for case in ("deep-path", "full-surface"):
            self.assertIn(f"case={case} baseline=0 mutated=1 reverted=0 -> PASS".encode(),
                          result.stdout)


if __name__ == "__main__":
    unittest.main()
