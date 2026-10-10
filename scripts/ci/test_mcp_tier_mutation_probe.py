import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
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
        stub.write_text('''#!/bin/bash
mutated=1
while IFS= read -r line; do
    if [[ "$line" == *'cfg(feature'* ]]; then mutated=0; fi
done < crates/gaze-mcp-core/src/tools/mod.rs
if [ "$mutated" -eq 1 ]; then
    : >"${READY_FILE:-mutated-ready}"
    if [ -n "${SIGNAL_CARGO:-}" ]; then
        kill -"$SIGNAL_CARGO" "$PROBE_OWNER_PID"
    fi
    if [ -n "${DIRTY_LIB:-}" ]; then
        printf '// unrelated edit\\n' >> crates/gaze-mcp-core/src/lib.rs
    fi
    if [ -n "${RESTORE_FAIL:-}" ]; then : > restore-fail; fi
fi
if [ -n "${GATE_FAIL:-}" ]; then exit 7; fi
exit "$mutated"
''')
        stub.chmod(0o755)
        cp_stub = stub.parent / "cp"
        cp_stub.write_text('#!/bin/bash\n' + r'''
args=("$@")
source="${args[${#args[@]}-2]}"
destination="${args[${#args[@]}-1]}"
if [ -f restore-fail ] && [[ "$source" == *gaze-tier-probe.* ]]; then
    case "${RESTORE_FAIL:-}" in
        partial) printf 'partial snapshot' >"$destination"; exit 1 ;;
        missing) "REAL_RM" -f "$source"; exit 1 ;;
        rename) ;;
        *) exit 1 ;;
    esac
fi
exec "REAL_CP" "$@"
'''.replace("REAL_RM", shutil.which("rm")).replace("REAL_CP", shutil.which("cp")))
        cp_stub.chmod(0o755)
        mv_stub = stub.parent / "mv"
        mv_stub.write_text(
            '#!/bin/bash\n'
            'if [ "${RESTORE_FAIL:-}" = rename ]; then exit 1; fi\n'
            f'exec "{shutil.which("mv")}" "$@"\n'
        )
        mv_stub.chmod(0o755)
        self.env = dict(os.environ, TMPDIR=str(self.root),
                        PATH=str(stub.parent) + os.pathsep + os.environ["PATH"])

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, timeout=60,
                                       env=dict(os.environ, GIT_OPTIONAL_LOCKS="0"))

    def snapshot(self):
        return ((self.root / ".git/index").read_bytes(),
                tuple((self.root / p).read_bytes() for p in TARGETS))

    def run_probe(self, *cases, **env):
        return subprocess.run(["bash", "-c", 'export PROBE_OWNER_PID=$$; exec bash "$@"',
                               "probe", str(SCRIPT), *cases], cwd=self.root,
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
        self.assertEqual(list(self.root.rglob(".gaze-tier-probe.*")), [])

    def install_command_wrappers(self, control):
        # Include every external command in the script, including reporting
        # pipelines and command substitutions. Count *invocations*, not just
        # command names, so both files and both deletion phases are exercised.
        commands = ("dirname", "git", "mkdir", "mktemp", "grep", "cp", "cat",
                    "rm", "mv", "cargo", "touch", "head", "sed")
        for command in commands:
            wrapper = self.root / "bin" / command
            if wrapper.exists():
                real = control / (command + "-real")
                shutil.copyfile(wrapper, real)
                real.chmod(0o755)
            else:
                real = Path(shutil.which(command))
            wrapper.write_text('#!/bin/bash\n' + f"command={command!r}\nreal={str(real)!r}\n" + r'''
count=0
counter="$CONTROL/$command.count"
if [ -f "$counter" ]; then read -r count <"$counter"; fi
count=$((count + 1))
printf '%s\n' "$count" >"$counter"
"$real" "$@"
code=$?
printf '%s|%s|%s\n' "$command" "$count" "$*" >>"$CONTROL/trace"
if [ "$command:$count" = "${SIGNAL_AT:-}" ]; then
    printf '%s\n' "$command:$count" >"$CONTROL/fired"
    if [ "${SIGNAL_RECIPIENT:-parent}" = owner ]; then
        kill -"$SIGNAL_NUMBER" "$PROBE_OWNER_PID"
    else
        kill -"$SIGNAL_NUMBER" "$PPID"
    fi
fi
exit "$code"
''')
            wrapper.chmod(0o755)

    def test_external_command_interrupt_matrix(self):
        with tempfile.TemporaryDirectory() as workspace:
            control = Path(workspace)
            self.install_command_wrappers(control)
            env = dict(CONTROL=workspace, TIER_PROBE_LOG_DIR=str(control / "logs"),
                       READY_FILE=str(control / "ready"))

            def reset_trace():
                for path in control.glob("*.count"):
                    path.unlink()
                for name in ("trace", "fired"):
                    (control / name).unlink(missing_ok=True)

            cells = 0
            for crlf in (False, True):
                if crlf:
                    self.make_clean_crlf_checkout()
                for case in ("deep-path", "full-surface"):
                    # The second scenario enters EXIT cleanup with owned
                    # mutations, then injects an additional signal there.
                    for cleanup in (False, True):
                        scenario = dict(env)
                        if cleanup:
                            scenario["SIGNAL_CARGO"] = str(int(signal.SIGTERM))
                        reset_trace()
                        before = self.snapshot()
                        status = self.git("status", "--porcelain", "--ignored", "--untracked-files=all")
                        result = self.run_probe(case, **scenario)
                        self.assertEqual(result.returncode, 143 if cleanup else 0,
                                         result.stdout + result.stderr)
                        self.assertEqual(self.snapshot(), before)
                        calls = [line.split("|", 2) for line in (control / "trace").read_text().splitlines()]
                        # These calls precede trap installation and all mutation.
                        calls = [call for call in calls if call[:2] not in
                                 (["dirname", "1"], ["git", "1"], ["mkdir", "1"])]
                        if cleanup:
                            # Only the critical cleanup suffix; ordinary
                            # boundaries are already covered by the first run.
                            calls = calls[next(i for i, call in enumerate(calls)
                                               if call[:2] == ["cargo", "2"]) + 1:]
                        # A shortened trace must not silently reduce coverage.
                        expected_calls = {
                            ("deep-path", False): 22, ("full-surface", False): 34,
                            ("deep-path", True): 5, ("full-surface", True): 9,
                        }
                        self.assertEqual(len(calls), expected_calls[case, cleanup])
                        for command, occurrence, args in calls:
                            for sig in (signal.SIGINT, signal.SIGTERM):
                                # Direct parent tests also hit subshells (and
                                # protected children); owner tests prove that
                                # the public shell preserves the signal exit.
                                for recipient in ("parent", "owner"):
                                    cells += 1
                                    with self.subTest(crlf=crlf, case=case, cleanup=cleanup,
                                                      command=command, occurrence=occurrence,
                                                      signal=sig, recipient=recipient):
                                        reset_trace()
                                        result = self.run_probe(
                                            case, **scenario, SIGNAL_AT=f"{command}:{occurrence}",
                                            SIGNAL_NUMBER=str(int(sig)), SIGNAL_RECIPIENT=recipient,
                                        )
                                        self.assertTrue((control / "fired").exists(), args)
                                        expected = 143 if cleanup else 128 + sig
                                        if recipient == "owner":
                                            self.assertEqual(result.returncode, expected,
                                                             result.stdout + result.stderr)
                                        else:
                                            # A signal to a protected child is
                                            # deliberately ignored; a reporting
                                            # subshell may instead fail the gate.
                                            self.assertIn(result.returncode, (0, 1, 2, 130, 143),
                                                          result.stdout + result.stderr)
                                        self.assertEqual(self.snapshot(), before)
                                        self.assert_snapshots_removed()
                                        self.assertEqual(
                                            self.git("status", "--porcelain", "--ignored", "--untracked-files=all"),
                                            status,
                                        )
                        print(f"matrix scenario crlf={crlf} case={case} cleanup={cleanup}: {cells} cells", flush=True)
            self.assertEqual(cells, 560)
            print(f"interrupt matrix: {cells} cells exercised", flush=True)

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
                before = self.snapshot()
                result = self.run_probe("full-surface", SIGNAL_CARGO=str(int(sig)))
                self.assertEqual(result.returncode, 128 + sig, result.stdout + result.stderr)
                self.assertEqual(self.snapshot(), before)
                self.assert_snapshots_removed()

    def test_restore_failure_preserves_sources_and_recovery_snapshots(self):
        before = self.snapshot()
        for mode in ("unreadable", "partial", "missing", "rename"):
            with self.subTest(mode=mode):
                result = self.run_probe("deep-path", RESTORE_FAIL=mode)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(b"FATAL: failed to restore", result.stderr)
                after_index, after_sources = self.snapshot()
                self.assertEqual(after_index, before[0])
                self.assertEqual(after_sources, (b"pub mod example;\n", before[1][1]))
                snapshots = list(self.root.glob("gaze-tier-probe.*"))
                self.assertEqual(len(snapshots), 1)
                self.assertIn(str(snapshots[0]).encode(), result.stderr)
                if mode != "missing":
                    self.assertEqual((snapshots[0] / TARGETS[0]).read_bytes(), before[1][0])
                # Failure intentionally retains recovery data. Reset only this
                # disposable fixture before the next independent fault.
                for path, contents in zip(TARGETS, before[1]):
                    (self.root / path).write_bytes(contents)
                shutil.rmtree(snapshots[0])
                for directory in self.root.rglob(".gaze-tier-probe.*"):
                    shutil.rmtree(directory)

    def test_atomic_restore_preserves_source_modes(self):
        modes = (0o640, 0o600)
        for path, mode in zip(TARGETS, modes):
            (self.root / path).chmod(mode)
        result = self.run_probe("full-surface")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(tuple((self.root / path).stat().st_mode & 0o777 for path in TARGETS), modes)

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
