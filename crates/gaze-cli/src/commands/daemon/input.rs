use std::io::{self, BufRead};
use std::sync::mpsc::{self, Receiver, SyncSender};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Frame {
    Line(String),
    FrameTooLarge { limit: usize },
}

pub(super) fn channel() -> (SyncSender<Frame>, Receiver<Frame>) {
    mpsc::sync_channel(1)
}

pub(super) fn read_frames(reader: impl BufRead, sender: SyncSender<Frame>, limit: usize) {
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
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn collect(input: &[u8], limit: usize, buffer: usize) -> Vec<Frame> {
        std::thread::scope(|scope| {
            let (sender, receiver) = channel();
            scope.spawn(move || {
                read_frames(
                    BufReader::with_capacity(buffer, Cursor::new(input)),
                    sender,
                    limit,
                );
            });
            receiver.into_iter().collect()
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
            let (sender, receiver) = channel();
            let source = Endless {
                read: Arc::clone(&read),
            };
            scope.spawn(move || read_frames(BufReader::with_capacity(3, source), sender, 8));
            assert_eq!(
                receiver.into_iter().collect::<Vec<_>>(),
                vec![Frame::FrameTooLarge { limit: 8 }]
            );
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
    fn daemon_frame_channel_backpressure_preserves_order() {
        let (sender, receiver) = channel();
        let (progress, observed) = mpsc::channel();
        struct Tracked {
            cursor: Cursor<&'static [u8]>,
            progress: mpsc::Sender<u64>,
        }
        impl Read for Tracked {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                let count = self.cursor.read(bytes)?;
                self.progress.send(self.cursor.position()).unwrap();
                Ok(count)
            }
        }
        let worker = std::thread::spawn(move || {
            let source = Tracked {
                cursor: Cursor::new(b"0\n1\n2\n3\n"),
                progress,
            };
            read_frames(BufReader::with_capacity(1, source), sender, 8);
        });
        for position in 1..=4 {
            assert_eq!(
                observed.recv_timeout(Duration::from_secs(5)).unwrap(),
                position
            );
        }
        assert!(matches!(
            observed.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        for index in 0..4 {
            assert_eq!(
                receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
                Frame::Line(index.to_string())
            );
        }
        worker.join().unwrap();
        assert_eq!(
            observed.into_iter().collect::<Vec<_>>(),
            vec![5, 6, 7, 8, 8]
        );
    }
}
