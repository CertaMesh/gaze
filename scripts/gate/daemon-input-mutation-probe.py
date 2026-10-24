#!/usr/bin/env python3
"""Prove production daemon input bounds, batching, and mutation sensitivity.

Commit the reader first. Compiles the actual dependency-free reader with Rust
1.96.0; daemon integration tests separately cover protocol/pipeline behavior.
The source is restored even if a mutation fails. Inputs are synthetic only.
"""

import argparse
from pathlib import Path
import re
import subprocess
import threading

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/gaze-cli/src/commands/daemon/input.rs"
PREFIX = "input::tests::"
TESTS = {
    "daemon_frame_at_cap_minus_one_and_cap_are_admitted",
    "daemon_frame_cap_plus_one_refuses_once_and_closes",
    "daemon_frame_without_lf_stops_after_cap_plus_fixed_read_buffer",
    "daemon_frames_preserve_lines_utf8_crlf_empty_and_final_eof",
    "daemon_frames_invalid_utf8_closes_without_processing_later_frames",
    "daemon_frame_limit_counts_cr_but_excludes_lf",
    "daemon_frames_write_all_before_read_within_byte_budget",
    "daemon_frame_at_queue_cap_is_admitted_when_empty",
    "daemon_frame_waits_for_remaining_byte_credits_then_succeeds",
    "daemon_frame_channel_backpressure_preserves_order",
    "daemon_empty_frames_consume_byte_credits_and_disconnect_wakes_reader",
    "daemon_terminal_overflow_follows_queued_frames_and_sender_disconnects",
}
KILLED_BY = {
    "daemon_frame_cap_plus_one_refuses_once_and_closes",
    "daemon_frame_without_lf_stops_after_cap_plus_fixed_read_buffer",
    "daemon_frame_limit_counts_cr_but_excludes_lf",
}
BATCH = "daemon_frames_write_all_before_read_within_byte_budget"
UNBOUNDED = """pub(super) fn read_frames(reader: impl BufRead, sender: FrameSender, _limit: usize) {
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
CAPACITY_ONE = """
pub(super) struct FrameSender(std::sync::mpsc::SyncSender<Frame>);
pub(super) struct FrameReceiver(std::sync::mpsc::Receiver<Frame>);
pub(super) fn channel(_budget: usize) -> (FrameSender, FrameReceiver) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    (FrameSender(sender), FrameReceiver(receiver))
}
impl FrameSender {
    fn send(&self, frame: Frame) -> Result<(), ()> {
        self.0.send(frame).map_err(|_| ())
    }
}
impl FrameReceiver {
    pub(super) fn recv_timeout(&self, timeout: Duration) -> Result<Frame, RecvTimeoutError> {
        self.0.recv_timeout(timeout)
    }
}
"""
PEER = """
use std::io::{self, Write};
use std::time::Duration;
fn main() {
    let budget: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let (sender, receiver) = input::channel(budget);
    std::thread::spawn(move || input::read_frames(io::stdin().lock(), sender, budget));
    let mut output = io::stdout().lock();
    loop {
        match receiver.recv_timeout(Duration::from_secs(30)) {
            Ok(input::Frame::Line(line)) => writeln!(output, "{line}").unwrap(),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            other => panic!("unexpected peer outcome: {other:?}"),
        }
    }
}
"""


def compile_rust(source: Path, binary: Path, test: bool = False) -> None:
    subprocess.run([
        "rustup", "run", "1.96.0", "rustc", "--edition=2021",
        *(["--test"] if test else []), str(source), "-o", str(binary),
    ], cwd=ROOT, check=True)


def run(output_dir: Path, tag: str, failed: set[str], only: str = "") -> None:
    harness = output_dir / "reader-tests.rs"
    harness.write_text(f'#[path = "{SOURCE}"]\nmod input;\n')
    binary = output_dir / "reader-tests"
    compile_rust(harness, binary, test=True)
    log = output_dir / f"{tag}.log"
    with log.open("w") as handle:
        result = subprocess.run([
            str(binary), PREFIX + only, "--nocapture", "--test-threads=2",
        ], cwd=ROOT, stdout=handle, stderr=subprocess.STDOUT, timeout=30)
    text = log.read_text()
    outcomes = re.findall(r"^test " + re.escape(PREFIX) + r"(\w+) \.\.\. (ok|FAILED)$", text, re.M)
    passed = {name for name, outcome in outcomes if outcome == "ok"}
    actual_failed = {name for name, outcome in outcomes if outcome == "FAILED"}
    expected = {only} if only else TESTS
    if (
        result.returncode != (101 if failed else 0)
        or len(outcomes) != len(expected)
        or actual_failed != failed
        or passed != expected - failed
        or (failed and "panicked at" not in text)
        or (only and "batch below byte budget stalled before reader drained" not in text)
    ):
        raise RuntimeError(f"{tag}: expected executed assertions were not observed; inspect {log}")
    print(f"{tag}: {len(passed)} named tests passed, {len(failed)} named assertions failed", flush=True)


def wire_batch(binary: Path, count: int, budget: int, backpressure: bool) -> None:
    data = b"".join(f"{index:063}\n".encode() for index in range(count))
    child = subprocess.Popen([str(binary), str(budget)], stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    written = threading.Event()
    errors = []

    def write_all():
        try:
            child.stdin.write(data)
            child.stdin.close()
            written.set()
        except Exception as error:
            errors.append(error)

    writer = threading.Thread(target=write_all)
    watchdog = threading.Timer(30, child.kill)
    watchdog.start()
    writer.start()
    try:
        # stdout stays entirely undrained throughout this observation.
        completed_before_drain = written.wait(0.5 if backpressure else 5)
        if completed_before_drain == backpressure:
            raise AssertionError(f"unexpected write completion: backpressure={backpressure}")
        output = child.stdout.read()
        writer.join(timeout=5)
        assert not writer.is_alive() and written.is_set() and not errors, errors
        assert child.wait(timeout=5) == 0, child.stderr.read()
        assert output == data, "responses lost or reordered"
        print(f"wire: {len(data)} bytes, budget {budget}, backpressure={backpressure}, exact ordered output", flush=True)
    finally:
        if child.poll() is None:
            child.kill()
        child.wait()
        writer.join(timeout=5)
        watchdog.cancel()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "target/daemon-input-mutation")
    args = parser.parse_args()
    output_dir = args.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    subprocess.run(["git", "diff", "--exit-code", "HEAD", "--", str(SOURCE)], cwd=ROOT, check=True)
    original = SOURCE.read_text()
    committed = subprocess.check_output(["git", "show", f"HEAD:{SOURCE.relative_to(ROOT)}"], cwd=ROOT).decode()
    if original != committed:
        raise RuntimeError("commit the reader before mutation")
    run(output_dir, "baseline", set())
    peer = output_dir / "reader-peer.rs"
    peer.write_text(f'#[path = "{SOURCE}"]\nmod input;\n' + PEER)
    binary = output_dir / "reader-peer"
    compile_rust(peer, binary)
    wire_batch(binary, 2000, 128_000, False)
    wire_batch(binary, 20_000, 128_000, True)
    start = original.index("pub(super) fn read_frames(")
    end = original.index("\nstruct FrameReader", start)
    try:
        SOURCE.write_text(original[:start] + UNBOUNDED + original[end:])
        run(output_dir, "unbounded-lines", KILLED_BY)
        queue_start = original.index("// Store LF-delimited")
        queue_end = original.index("pub(super) fn read_frames(")
        SOURCE.write_text(original[:queue_start] + CAPACITY_ONE + original[queue_end:])
        run(output_dir, "capacity-one", {BATCH}, only=BATCH)
    finally:
        SOURCE.write_text(original)
    run(output_dir, "restored", set())


if __name__ == "__main__":
    main()
