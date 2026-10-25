use std::collections::VecDeque;
use std::io::{self, BufRead};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Frame {
    Line(String),
    FrameTooLarge { limit: usize },
}

// Store LF-delimited UTF-8 in one packed buffer: per-frame allocations or
// metadata would let empty/tiny frames escape a byte-only memory budget.
struct Queue {
    state: Mutex<QueueState>,
    changed: Condvar,
    budget: usize,
}

struct QueueState {
    bytes: VecDeque<u8>,
    terminal: Option<usize>,
    sender_alive: bool,
    receiver_alive: bool,
}

pub(super) struct FrameSender(Arc<Queue>);
pub(super) struct FrameReceiver(Arc<Queue>);

pub(super) fn channel(budget: usize) -> (FrameSender, FrameReceiver) {
    budget
        .checked_add(1)
        .expect("queue budget must leave room for LF");
    let queue = Arc::new(Queue {
        state: Mutex::new(QueueState {
            bytes: VecDeque::new(),
            terminal: None,
            sender_alive: true,
            receiver_alive: true,
        }),
        changed: Condvar::new(),
        budget,
    });
    (FrameSender(Arc::clone(&queue)), FrameReceiver(queue))
}

impl FrameSender {
    fn send(&self, frame: Frame) -> Result<(), ()> {
        let mut state = self.0.state.lock().unwrap();
        match frame {
            Frame::Line(line) => {
                assert!(line.len() <= self.0.budget);
                let size = line.len() + 1;
                // An empty queue admits a frame at the cap, plus its delimiter.
                // Otherwise the delimiter also consumes byte credits, so even
                // empty frames cannot accumulate without bound.
                state = self
                    .0
                    .changed
                    .wait_while(state, |state| {
                        state.receiver_alive
                            && !state.bytes.is_empty()
                            && size > self.0.budget.saturating_sub(state.bytes.len())
                    })
                    .unwrap();
                if !state.receiver_alive {
                    return Err(());
                }
                let needed = state.bytes.len() + size;
                if needed > state.bytes.capacity() {
                    let capacity = needed
                        .max(state.bytes.capacity().saturating_mul(2))
                        .min(self.0.budget + 1);
                    let additional = capacity - state.bytes.len();
                    state.bytes.reserve_exact(additional);
                }
                // Credits are held under the mutex before any frame is copied.
                state.bytes.extend(line.as_bytes());
                state.bytes.push_back(b'\n');
            }
            Frame::FrameTooLarge { limit } => {
                if !state.receiver_alive {
                    return Err(());
                }
                state.terminal = Some(limit);
            }
        }
        self.0.changed.notify_all();
        Ok(())
    }
}

impl Drop for FrameSender {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().sender_alive = false;
        self.0.changed.notify_all();
    }
}

impl FrameReceiver {
    pub(super) fn recv_timeout(&self, timeout: Duration) -> Result<Frame, RecvTimeoutError> {
        let state = self.0.state.lock().unwrap();
        let (mut state, _) = self
            .0
            .changed
            .wait_timeout_while(state, timeout, |state| {
                state.bytes.is_empty() && state.terminal.is_none() && state.sender_alive
            })
            .unwrap();
        if let Some(length) = state.bytes.iter().position(|byte| *byte == b'\n') {
            let mut bytes = Vec::with_capacity(length);
            bytes.extend(state.bytes.drain(..length));
            state.bytes.pop_front();
            // Taking a frame returns its byte credits before processing starts.
            drop(state);
            self.0.changed.notify_all();
            return Ok(Frame::Line(
                String::from_utf8(bytes).expect("reader supplied UTF-8"),
            ));
        }
        if let Some(limit) = state.terminal.take() {
            return Ok(Frame::FrameTooLarge { limit });
        }
        if state.sender_alive {
            Err(RecvTimeoutError::Timeout)
        } else {
            Err(RecvTimeoutError::Disconnected)
        }
    }
}

impl Drop for FrameReceiver {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().receiver_alive = false;
        self.0.changed.notify_all();
    }
}

pub(super) fn read_frames(reader: impl BufRead, sender: FrameSender, limit: usize) {
    let mut reader = FrameReader {
        reader,
        limit,
        bytes: Vec::new(),
    };
    while let Ok(Some(frame)) = reader.next() {
        let terminal = matches!(frame, Frame::FrameTooLarge { .. });
        if sender.send(frame).is_err() || terminal {
            return;
        }
    }
}

struct FrameReader<R> {
    reader: R,
    limit: usize,
    bytes: Vec<u8>,
}

impl<R: BufRead> FrameReader<R> {
    fn next(&mut self) -> io::Result<Option<Frame>> {
        loop {
            let available = match self.reader.fill_buf() {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => result?,
            };
            if available.is_empty() {
                return if self.bytes.is_empty() {
                    Ok(None)
                } else {
                    self.finish(false).map(Some)
                };
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let count = newline.unwrap_or(available.len());
            if count > self.limit - self.bytes.len() {
                return Ok(Some(Frame::FrameTooLarge { limit: self.limit }));
            }
            let needed = self.bytes.len() + count;
            if needed > self.bytes.capacity() {
                let capacity = needed
                    .max(self.bytes.capacity().saturating_mul(2))
                    .min(self.limit);
                self.bytes.reserve_exact(capacity - self.bytes.len());
            }
            self.bytes.extend_from_slice(&available[..count]);
            self.reader.consume(count + usize::from(newline.is_some()));
            if newline.is_some() {
                return self.finish(true).map(Some);
            }
        }
    }

    fn finish(&mut self, newline: bool) -> io::Result<Frame> {
        // Match BufRead::lines: remove CR only before LF; accept an EOF tail.
        if newline && self.bytes.last() == Some(&b'\r') {
            self.bytes.pop();
        }
        String::from_utf8(std::mem::take(&mut self.bytes))
            .map(Frame::Line)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor, Read};
    use std::sync::mpsc;
    use std::time::Duration;

    fn drain(receiver: FrameReceiver) -> Vec<Frame> {
        let mut frames = Vec::new();
        loop {
            match receiver.recv_timeout(Duration::from_secs(5)) {
                Ok(frame) => frames.push(frame),
                Err(RecvTimeoutError::Disconnected) => return frames,
                Err(RecvTimeoutError::Timeout) => panic!("reader failed to finish"),
            }
        }
    }

    fn collect(input: &[u8], limit: usize, buffer: usize) -> Vec<Frame> {
        std::thread::scope(|scope| {
            let (sender, receiver) = channel(160 * 1024 * 1024);
            scope.spawn(move || {
                read_frames(
                    BufReader::with_capacity(buffer, Cursor::new(input)),
                    sender,
                    limit,
                );
            });
            drain(receiver)
        })
    }

    #[test]
    fn daemon_frame_at_cap_minus_one_and_cap_are_admitted() {
        assert_eq!(
            collect(b"1234567\n12345678\n", 8, 3),
            vec![
                Frame::Line("1234567".into()),
                Frame::Line("12345678".into()),
            ]
        );
    }

    #[test]
    fn daemon_frame_cap_plus_one_refuses_once_and_closes() {
        assert_eq!(
            collect(b"123456789\nlater\n", 8, 3),
            vec![Frame::FrameTooLarge { limit: 8 }]
        );
    }

    struct Endless {
        read: Arc<Mutex<usize>>,
    }

    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            // Finite guard makes an unbounded-reader mutation fail without hanging CI.
            let mut read = self.read.lock().unwrap();
            if *read >= 1024 {
                return Err(io::Error::other("test reader budget exhausted"));
            }
            buf.fill(b'x');
            *read += buf.len();
            Ok(buf.len())
        }
    }

    #[test]
    fn daemon_frame_without_lf_stops_after_cap_plus_fixed_read_buffer() {
        let read = Arc::new(Mutex::new(0));
        let mut bounded = FrameReader {
            reader: BufReader::with_capacity(
                3,
                Endless {
                    read: Arc::clone(&read),
                },
            ),
            limit: 8,
            bytes: Vec::new(),
        };
        assert_eq!(
            bounded.next().unwrap(),
            Some(Frame::FrameTooLarge { limit: 8 })
        );
        assert!(bounded.bytes.len() <= 8);
        assert!(bounded.bytes.capacity() <= 8);
        assert!(*read.lock().unwrap() <= 8 + 3);
        *read.lock().unwrap() = 0;
        std::thread::scope(|scope| {
            let (sender, receiver) = channel(160 * 1024 * 1024);
            let source = Endless {
                read: Arc::clone(&read),
            };
            scope.spawn(move || read_frames(BufReader::with_capacity(3, source), sender, 8));
            assert_eq!(drain(receiver), vec![Frame::FrameTooLarge { limit: 8 }]);
        });
        assert!(*read.lock().unwrap() <= 8 + 3);
    }

    #[test]
    fn daemon_frames_preserve_lines_utf8_crlf_empty_and_final_eof() {
        let input = "\n\r\nhello\né🙂\r\nlast\r".as_bytes();
        let expected = input
            .lines()
            .map(|line| Frame::Line(line.unwrap()))
            .collect::<Vec<_>>();
        for buffer in 1..=8 {
            assert_eq!(collect(input, 32, buffer), expected);
        }
    }

    #[test]
    fn daemon_frames_invalid_utf8_closes_without_processing_later_frames() {
        assert_eq!(
            collect(b"first\n\xff\nlater\n", 32, 1),
            vec![Frame::Line("first".into())]
        );
    }

    #[test]
    fn daemon_frame_limit_counts_cr_but_excludes_lf() {
        assert_eq!(
            collect(b"1234567\r\n", 8, 3),
            vec![Frame::Line("1234567".into())]
        );
        assert_eq!(
            collect(b"12345678\r\n", 8, 3),
            vec![Frame::FrameTooLarge { limit: 8 }]
        );
        assert_eq!(
            collect(b"12345678", 8, 3),
            vec![Frame::Line("12345678".into())]
        );
        assert_eq!(
            collect(b"123456789", 8, 3),
            vec![Frame::FrameTooLarge { limit: 8 }]
        );
    }

    #[test]
    fn daemon_frames_write_all_before_read_within_byte_budget() {
        let input =
            b"{\"version\":1,\"op\":\"clean\",\"session_id\":\"batch\",\"text\":\"public\"}\n"
                .repeat(2000);
        assert_eq!(input.len(), 128_000);
        let (sender, receiver) = channel(input.len());
        let (done, completed) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            read_frames(Cursor::new(input), sender, 160 * 1024 * 1024);
            done.send(()).unwrap();
        });
        let written = completed.recv_timeout(Duration::from_secs(2)).is_ok();
        let frames = if written {
            drain(receiver)
        } else {
            drop(receiver);
            Vec::new()
        };
        worker.join().unwrap();
        assert!(
            written,
            "batch below byte budget stalled before reader drained"
        );
        assert_eq!(
            frames,
            vec![
                Frame::Line(
                    r#"{"version":1,"op":"clean","session_id":"batch","text":"public"}"#.into()
                );
                2000
            ]
        );
    }

    #[test]
    fn daemon_frame_at_queue_cap_is_admitted_when_empty() {
        let (sender, receiver) = channel(8);
        sender.send(Frame::Line("12345678".into())).unwrap();
        assert_eq!(
            receiver.recv_timeout(Duration::ZERO).unwrap(),
            Frame::Line("12345678".into())
        );
    }

    #[test]
    fn daemon_frame_waits_for_remaining_byte_credits_then_succeeds() {
        let (sender, receiver) = channel(8);
        sender.send(Frame::Line("123".into())).unwrap();
        let (started, starting) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            let result = sender.send(Frame::Line("12345678".into()));
            done.send(result).unwrap();
        });
        starting.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            completed.recv_timeout(Duration::from_millis(100)),
            Err(RecvTimeoutError::Timeout)
        );
        assert_eq!(
            receiver.recv_timeout(Duration::ZERO).unwrap(),
            Frame::Line("123".into())
        );
        assert_eq!(
            completed.recv_timeout(Duration::from_secs(5)).unwrap(),
            Ok(())
        );
        assert_eq!(drain(receiver), vec![Frame::Line("12345678".into())]);
        worker.join().unwrap();
    }

    #[test]
    fn daemon_frame_channel_backpressure_preserves_order() {
        let input = (0..20_000)
            .map(|index| format!("{index:05}\n"))
            .collect::<String>();
        let (sender, receiver) = channel(120_000 - 1);
        let (done, completed) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            read_frames(Cursor::new(input), sender, 120_000 - 1);
            done.send(()).unwrap();
        });
        let blocked = completed.recv_timeout(Duration::from_millis(100));
        let frames = drain(receiver);
        worker.join().unwrap();
        assert_eq!(blocked, Err(RecvTimeoutError::Timeout));
        assert_eq!(completed.recv_timeout(Duration::from_secs(5)), Ok(()));
        let expected = (0..20_000)
            .map(|index| Frame::Line(format!("{index:05}")))
            .collect::<Vec<_>>();
        assert_eq!(frames, expected);
    }

    #[test]
    fn daemon_empty_frames_consume_byte_credits_and_disconnect_wakes_reader() {
        let (sender, receiver) = channel(2);
        sender.send(Frame::Line(String::new())).unwrap();
        sender.send(Frame::Line(String::new())).unwrap();
        let (done, completed) = mpsc::channel();
        let worker =
            std::thread::spawn(move || done.send(sender.send(Frame::Line(String::new()))).unwrap());
        assert_eq!(
            completed.recv_timeout(Duration::from_millis(100)),
            Err(RecvTimeoutError::Timeout)
        );
        drop(receiver);
        assert_eq!(completed.recv_timeout(Duration::from_secs(5)), Ok(Err(())));
        worker.join().unwrap();
    }

    #[test]
    fn daemon_terminal_overflow_follows_queued_frames_and_sender_disconnects() {
        let (sender, receiver) = channel(2);
        sender.send(Frame::Line("x".into())).unwrap();
        sender.send(Frame::FrameTooLarge { limit: 2 }).unwrap();
        drop(sender);
        assert_eq!(
            drain(receiver),
            vec![Frame::Line("x".into()), Frame::FrameTooLarge { limit: 2 }]
        );
        let (sender, receiver) = channel(2);
        assert_eq!(
            receiver.recv_timeout(Duration::ZERO),
            Err(RecvTimeoutError::Timeout)
        );
        drop(sender);
        assert_eq!(
            receiver.recv_timeout(Duration::ZERO),
            Err(RecvTimeoutError::Disconnected)
        );
    }
}
