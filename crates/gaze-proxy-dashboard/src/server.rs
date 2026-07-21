use std::collections::HashSet;

use crate::assets::DATA_FREE_SHELL;
use crate::security_headers::SECURITY_HEADERS;
use crate::{DashboardError, DashboardErrorCode};

/// Fixed raw request cap before any framework parser.
pub const MAX_HTTP_REQUEST_BYTES: usize = 16 * 1024;
const MAX_REQUEST_LINE: usize = 512;
const MAX_HEADER_COUNT: usize = 32;
const MAX_HEADER_LINE: usize = 2_048;
const MAX_BODY_BYTES: usize = 4_096;

/// Byte-invariant raw-gate rejection with the full fixed security policy.
pub const CONSTANT_REJECTION_RESPONSE: &[u8] = concat!(
    "HTTP/1.1 400 Bad Request\r\n",
    "Content-Type: text/plain; charset=utf-8\r\n",
    "Content-Length: 17\r\n",
    "Cache-Control: no-store\r\n",
    "Pragma: no-cache\r\n",
    "Expires: 0\r\n",
    "X-Content-Type-Options: nosniff\r\n",
    "Referrer-Policy: no-referrer\r\n",
    "X-Frame-Options: DENY\r\n",
    "Cross-Origin-Opener-Policy: same-origin\r\n",
    "Cross-Origin-Embedder-Policy: require-corp\r\n",
    "Cross-Origin-Resource-Policy: same-origin\r\n",
    "Permissions-Policy: accelerometer=(), camera=(), geolocation=(), microphone=(), payment=(), usb=()\r\n",
    "Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'none'; font-src 'none'; object-src 'none'; frame-src 'none'; worker-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; require-trusted-types-for 'script'; trusted-types 'none'\r\n",
    "Clear-Site-Data: \"cache\", \"cookies\", \"storage\"\r\n",
    "Connection: close\r\n",
    "\r\n",
    "request rejected\n"
)
.as_bytes();

/// Closed route class emitted by the raw connection gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidatedDashboardRequestV1 {
    /// Data-free pre-authentication shell.
    Shell,
    /// Embedded stylesheet.
    Stylesheet,
    /// Embedded script.
    Script,
    /// One-time launch credential pairing.
    PairSession,
    /// Safe metadata snapshot.
    Snapshot,
    /// Safe metadata follow/poll.
    Follow,
    /// Exact provider-visible payload request.
    ProviderVisible,
    /// Exact OwnerRaw reveal request.
    RevealOwnerRaw,
    /// Exact OwnerRestored reveal request.
    RevealOwnerRestored,
    /// Reusable purge request; no caller-selected epoch.
    Purge,
}

/// Validated request information containing only a closed route and fixed-body bytes.
pub struct ValidatedRequest {
    route: ValidatedDashboardRequestV1,
    body: Vec<u8>,
    authorization: Option<Vec<u8>>,
    page_session: Option<Vec<u8>>,
    csrf: Option<Vec<u8>>,
}

impl ValidatedRequest {
    /// Returns the closed route class.
    #[must_use]
    pub const fn route(&self) -> ValidatedDashboardRequestV1 {
        self.route
    }

    /// Returns the bounded route body after raw framing validation.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub(crate) fn authorization(&self) -> Option<&[u8]> {
        self.authorization.as_deref()
    }

    pub(crate) fn page_session(&self) -> Option<&[u8]> {
        self.page_session.as_deref()
    }

    pub(crate) fn csrf(&self) -> Option<&[u8]> {
        self.csrf.as_deref()
    }
}

/// Connection-level one-request HTTP/1.1 gate.
pub struct DashboardHttp1Gate;

impl DashboardHttp1Gate {
    /// Validates a complete connection buffer before any framework/parser/logging layer.
    pub fn validate(
        input: &[u8],
        expected_host: &[u8],
        expected_origin: &[u8],
    ) -> Result<ValidatedRequest, DashboardError> {
        if input.is_empty() || input.len() > MAX_HTTP_REQUEST_BYTES || has_forbidden_control(input)
        {
            return rejected();
        }
        let header_end = find_bytes(input, b"\r\n\r\n").ok_or_else(rejected_error)?;
        let head = &input[..header_end];
        let body = &input[header_end + 4..];
        let mut lines = head.split(|byte| *byte == b'\n');
        let request_line = trim_cr(lines.next().ok_or_else(rejected_error)?);
        if request_line.len() > MAX_REQUEST_LINE {
            return rejected();
        }
        let mut request_parts = request_line.split(|byte| *byte == b' ');
        let method = request_parts.next().ok_or_else(rejected_error)?;
        let target = request_parts.next().ok_or_else(rejected_error)?;
        let version = request_parts.next().ok_or_else(rejected_error)?;
        if request_parts.next().is_some()
            || version != b"HTTP/1.1"
            || !target.starts_with(b"/")
            || target.starts_with(b"//")
            || target.contains(&b'?')
            || target.contains(&b'#')
            || method == b"CONNECT"
        {
            return rejected();
        }
        let route = route(method, target).ok_or_else(rejected_error)?;
        let body_allowed = !matches!(
            route,
            ValidatedDashboardRequestV1::Shell
                | ValidatedDashboardRequestV1::Stylesheet
                | ValidatedDashboardRequestV1::Script
        );

        let mut names = HashSet::new();
        let mut host = None;
        let mut origin = None;
        let mut authorization = None;
        let mut page_session = None;
        let mut csrf = None;
        let mut content_length = None;
        let mut count = 0_usize;
        for raw_line in lines {
            let line = trim_cr(raw_line);
            count += 1;
            if count > MAX_HEADER_COUNT
                || line.is_empty()
                || line.len() > MAX_HEADER_LINE
                || line[0] == b' '
                || line[0] == b'\t'
            {
                return rejected();
            }
            let separator = line
                .iter()
                .position(|byte| *byte == b':')
                .ok_or_else(rejected_error)?;
            let name = &line[..separator];
            let value = trim_ows(&line[separator + 1..]);
            if name.is_empty()
                || !name.iter().all(|byte| is_token(*byte))
                || !value
                    .iter()
                    .all(|byte| *byte == b'\t' || (0x20..=0x7e).contains(byte))
            {
                return rejected();
            }
            let lower = name.iter().map(u8::to_ascii_lowercase).collect::<Vec<_>>();
            if !names.insert(lower.clone()) {
                return rejected();
            }
            match lower.as_slice() {
                b"host" => host = Some(value),
                b"origin" => origin = Some(value),
                b"authorization" => authorization = Some(value.to_vec()),
                b"x-gaze-page-session" => page_session = Some(value.to_vec()),
                b"x-gaze-csrf" => csrf = Some(value.to_vec()),
                b"content-length" => {
                    let text = std::str::from_utf8(value).map_err(|_| rejected_error())?;
                    if text.starts_with('+')
                        || (text.len() > 1 && text.starts_with('0'))
                        || !text.bytes().all(|byte| byte.is_ascii_digit())
                    {
                        return rejected();
                    }
                    content_length = Some(text.parse::<usize>().map_err(|_| rejected_error())?);
                }
                b"transfer-encoding"
                | b"upgrade"
                | b"expect"
                | b"forwarded"
                | b"x-forwarded-for"
                | b"x-forwarded-host"
                | b"x-forwarded-proto"
                | b"cookie"
                | b"cookie2"
                | b"proxy-authorization" => return rejected(),
                _ => {}
            }
        }
        if host != Some(expected_host) {
            return rejected();
        }
        if method == b"POST" && origin != Some(expected_origin) {
            return rejected();
        }
        if method == b"GET" && origin.is_some() {
            return rejected();
        }
        let declared = content_length.unwrap_or(0);
        if declared != body.len()
            || body.len() > MAX_BODY_BYTES
            || (!body_allowed && !body.is_empty())
            || (body_allowed && content_length.is_none())
        {
            return rejected();
        }
        Ok(ValidatedRequest {
            route,
            body: body.to_vec(),
            authorization,
            page_session,
            csrf,
        })
    }

    pub(crate) fn shell_response() -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n{}\r\n",
            DATA_FREE_SHELL.len(),
            SECURITY_HEADERS
        )
        .into_bytes();
        response.extend_from_slice(DATA_FREE_SHELL);
        response
    }
}

fn route(method: &[u8], target: &[u8]) -> Option<ValidatedDashboardRequestV1> {
    match (method, target) {
        (b"GET", b"/") => Some(ValidatedDashboardRequestV1::Shell),
        (b"GET", b"/assets/app.css") => Some(ValidatedDashboardRequestV1::Stylesheet),
        (b"GET", b"/assets/app.js") => Some(ValidatedDashboardRequestV1::Script),
        (b"POST", b"/api/v1/session/pair") => Some(ValidatedDashboardRequestV1::PairSession),
        (b"POST", b"/api/v1/events/snapshot") => Some(ValidatedDashboardRequestV1::Snapshot),
        (b"POST", b"/api/v1/events/follow") => Some(ValidatedDashboardRequestV1::Follow),
        (b"POST", b"/api/v1/events/provider-visible") => {
            Some(ValidatedDashboardRequestV1::ProviderVisible)
        }
        (b"POST", b"/api/v1/reveal/owner-raw") => Some(ValidatedDashboardRequestV1::RevealOwnerRaw),
        (b"POST", b"/api/v1/reveal/owner-restored") => {
            Some(ValidatedDashboardRequestV1::RevealOwnerRestored)
        }
        (b"POST", b"/api/v1/purge") => Some(ValidatedDashboardRequestV1::Purge),
        _ => None,
    }
}

fn has_forbidden_control(input: &[u8]) -> bool {
    input
        .iter()
        .any(|byte| *byte == 0 || (*byte < 0x09) || (*byte > 0x0d && *byte < 0x20) || *byte == 0x7f)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn trim_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn trim_ows(mut value: &[u8]) -> &[u8] {
    while value
        .first()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        value = &value[1..];
    }
    while value
        .last()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        value = &value[..value.len() - 1];
    }
    value
}

fn is_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn rejected<T>() -> Result<T, DashboardError> {
    Err(rejected_error())
}

const fn rejected_error() -> DashboardError {
    DashboardError::new(DashboardErrorCode::HttpRejected)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &[u8] = b"127.12.34.56:54321";
    const ORIGIN: &[u8] = b"http://127.12.34.56:54321";

    #[test]
    fn accepts_one_canonical_request() {
        let request = b"GET / HTTP/1.1\r\nHost: 127.12.34.56:54321\r\n\r\n";
        assert_eq!(
            DashboardHttp1Gate::validate(request, HOST, ORIGIN)
                .unwrap()
                .route(),
            ValidatedDashboardRequestV1::Shell
        );
    }

    #[test]
    fn rejects_smuggling_and_ambient_credentials_with_same_error() {
        let cases: &[&[u8]] = &[
            b"GET / HTTP/1.1\r\nHost: 127.12.34.56:54321\r\nHost: 127.12.34.56:54321\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: 127.12.34.56:54321\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: 127.12.34.56:54321\r\nCookie: stale=yes\r\n\r\n",
            b"GET /?x=1 HTTP/1.1\r\nHost: 127.12.34.56:54321\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: 127.12.34.56:54321\r\n\r\nGET / HTTP/1.1\r\n\r\n",
        ];
        for request in cases {
            let Err(error) = DashboardHttp1Gate::validate(request, HOST, ORIGIN) else {
                panic!("raw gate accepted a forbidden request");
            };
            assert_eq!(error.code(), DashboardErrorCode::HttpRejected);
        }
    }
}
