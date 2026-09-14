use std::io::Read;

const MAX_VERBOSE_STDERR_BYTES: usize = 256;
const TRUNCATION_MARKER: &str = " [truncated]";

/// Keep a bounded diagnostic prefix, but consume every byte so the child can exit.
pub(super) fn read_stderr(mut reader: impl Read) -> std::io::Result<String> {
    let mut prefix = Vec::with_capacity(MAX_VERBOSE_STDERR_BYTES);
    let mut chunk = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let keep = read.min(MAX_VERBOSE_STDERR_BYTES - prefix.len());
        prefix.extend_from_slice(&chunk[..keep]);
        truncated |= keep < read;
    }

    if truncated {
        // A token cut before its @ or seventh digit could evade the redactor.
        // Only display tokens whose delimiter was captured.
        let complete = prefix
            .iter()
            .rposition(|byte| !byte.is_ascii_graphic())
            .map_or(0, |index| index + 1);
        prefix.truncate(complete);
    }
    Ok(format_stderr(&prefix, truncated))
}

#[cfg(test)]
pub(super) fn sanitize_stderr(bytes: &[u8]) -> String {
    format_stderr(bytes, false)
}

fn format_stderr(bytes: &[u8], truncated: bool) -> String {
    let ascii = bytes
        .iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                ' '
            }
        })
        .collect::<String>();
    let mut sanitized = sanitize_error(&ascii);
    if truncated || sanitized.len() > MAX_VERBOSE_STDERR_BYTES {
        // Sanitization produces ASCII, so this byte boundary is also UTF-8 safe.
        sanitized.truncate(MAX_VERBOSE_STDERR_BYTES - TRUNCATION_MARKER.len());
        sanitized.push_str(TRUNCATION_MARKER);
    }
    sanitized
}

pub(super) fn sanitize_error(message: &str) -> String {
    message
        .split_ascii_whitespace()
        .map(sanitize_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_token(token: &str) -> String {
    if token.contains('@') {
        return "<redacted>".to_string();
    }

    let digit_count = token.bytes().filter(u8::is_ascii_digit).count();
    if digit_count >= 7 {
        return "<redacted>".to_string();
    }

    token.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_boundaries_and_redaction_expansion_stay_within_byte_cap() {
        for size in [0, 255, 256, 257, 2 * 1024 * 1024] {
            let input = vec![b'w'; size];
            let mut reader = std::io::Cursor::new(&input);
            let diagnostic = read_stderr(&mut reader).unwrap();
            assert_eq!(reader.position(), size as u64, "must drain to EOF");
            assert!(diagnostic.len() <= 256);
            assert_eq!(diagnostic.ends_with("[truncated]"), size > 256);
            if size <= 256 {
                assert_eq!(diagnostic.as_bytes(), input);
            }
        }

        // Short tokens expand to <redacted>, so enforce the cap after sanitizing.
        let diagnostic = read_stderr("@ ".repeat(128).as_bytes()).unwrap();
        assert_eq!(diagnostic.len(), 256);
        assert!(diagnostic.ends_with("[truncated]"));
        assert!(!diagnostic.contains('@'));
    }

    #[test]
    fn incomplete_sensitive_tokens_are_not_displayed() {
        for suffix in ["alice@example.invalid", "+1-555-0101", "é@example.invalid"] {
            let input = format!("{}{}", "safe ".repeat(50), suffix);
            let diagnostic = read_stderr(input.as_bytes()).unwrap();
            assert!(diagnostic.ends_with("[truncated]"));
            assert!(!diagnostic.contains("alice"));
            assert!(!diagnostic.contains("555"));
            assert!(diagnostic.is_ascii());
        }
        let diagnostic = read_stderr(b"failed \x1b\xff alice@example.invalid".as_slice()).unwrap();
        assert_eq!(diagnostic, "failed <redacted>");
    }

    #[test]
    fn real_io_errors_before_and_after_cap_are_preserved() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "synthetic read failure",
                ))
            }
        }

        for size in [0, 256, 257, 2 * 1024 * 1024] {
            let prefix = std::io::Cursor::new(vec![b'w'; size]);
            let error = read_stderr(prefix.chain(Broken)).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
    }
}
