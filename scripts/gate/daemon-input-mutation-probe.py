#!/usr/bin/env python3
"""Prove daemon frame tests reject the previous unbounded lines() reader.

Commit the reader first. Run under the workspace build lock with Rust 1.96.0.
The source is restored even if the probe fails. Logs use synthetic input only.
"""

import argparse
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/gaze-cli/src/commands/daemon/input.rs"
PREFIX = "commands::daemon::input::tests::"
TESTS = {
    "daemon_frame_at_cap_minus_one_and_cap_are_admitted",
    "daemon_frame_cap_plus_one_refuses_once_and_closes",
    "daemon_frame_without_lf_stops_after_cap_plus_fixed_read_buffer",
    "daemon_frames_preserve_lines_utf8_crlf_empty_and_final_eof",
    "daemon_frames_invalid_utf8_closes_without_processing_later_frames",
    "daemon_frame_limit_counts_cr_but_excludes_lf",
    "daemon_frame_channel_backpressure_preserves_order",
}
KILLED_BY = {
    "daemon_frame_cap_plus_one_refuses_once_and_closes",
    "daemon_frame_without_lf_stops_after_cap_plus_fixed_read_buffer",
    "daemon_frame_limit_counts_cr_but_excludes_lf",
}
UNBOUNDED = """pub(super) fn read_frames(reader: impl BufRead, sender: SyncSender<Frame>, _limit: usize) {
    for line in reader.lines() {
        match line {
            Ok(line) => {
                if sender.send(Frame::Line(line)).is_err() {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}
"""


def run(output_dir: Path, tag: str, failed: set[str]) -> None:
    command = [
        "cargo", "test", "-p", "gaze-cli", "--all-features", "--bin", "gaze",
        PREFIX, "--", "--nocapture", "--test-threads=2",
    ]
    log = output_dir / f"{tag}.log"
    with log.open("w") as handle:
        result = subprocess.run(command, cwd=ROOT, stdout=handle, stderr=subprocess.STDOUT)
    text = log.read_text()
    outcomes = re.findall(r"^test " + re.escape(PREFIX) + r"(\w+) \.\.\. (ok|FAILED)$", text, re.M)
    passed = {name for name, outcome in outcomes if outcome == "ok"}
    actual_failed = {name for name, outcome in outcomes if outcome == "FAILED"}
    expected_exit = 101 if failed else 0
    if (
        result.returncode != expected_exit
        or len(outcomes) != len(TESTS)
        or actual_failed != failed
        or passed != TESTS - failed
        or (failed and "assertion `left == right` failed" not in text)
    ):
        raise RuntimeError(f"{tag}: expected executed assertions were not observed; inspect {log}")
    print(f"{tag}: {len(passed)} named tests passed, {len(failed)} named assertions failed", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "target/daemon-input-mutation")
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    subprocess.run(["git", "diff", "--exit-code", "HEAD", "--", str(SOURCE)], cwd=ROOT, check=True)
    original = SOURCE.read_text()
    committed = subprocess.check_output(["git", "show", f"HEAD:{SOURCE.relative_to(ROOT)}"], cwd=ROOT).decode()
    if original != committed:
        raise RuntimeError("commit the reader before mutation")
    start = original.index("pub(super) fn read_frames(")
    end = original.index("\nstruct FrameReader", start)
    run(args.output_dir, "baseline", set())
    try:
        SOURCE.write_text(original[:start] + UNBOUNDED + original[end:])
        run(args.output_dir, "unbounded-lines", KILLED_BY)
    finally:
        SOURCE.write_text(original)
    run(args.output_dir, "restored", set())


if __name__ == "__main__":
    main()
