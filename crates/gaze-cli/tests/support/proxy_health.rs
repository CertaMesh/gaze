use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// Whether a gaze proxy at `addr` answers `GET /_gaze_proxy/healthz` with 200.
///
/// A bare `connect` does not prove a server is listening. With several tests
/// polling not-yet-bound loopback ports at once, a 1 s slow-startup probe
/// showed `connect` succeeding on ports nothing listened on (most likely two
/// polling sockets pairing up in a TCP simultaneous open). Only an HTTP answer
/// counts.
///
/// A probe that gets no answer returns `false`, and the caller retries. The
/// read timeout bounds one probe against a peer that never answers; it never
/// bounds startup and never fails a test.
pub fn proxy_answers_health(addr: SocketAddr) -> bool {
    let Ok(mut stream) = TcpStream::connect(addr) else {
        return false;
    };
    if stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .is_err()
        || write!(
            stream,
            "GET /_gaze_proxy/healthz HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\n\r\n"
        )
        .is_err()
    {
        return false;
    }
    // Decide on the status line alone: a paired polling socket sends its own
    // request or closes, never a response, so neither side waits on the other.
    let mut status = [0_u8; 12];
    stream.read_exact(&mut status).is_ok() && status == *b"HTTP/1.1 200"
}
