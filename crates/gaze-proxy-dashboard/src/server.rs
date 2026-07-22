use zeroize::Zeroizing;

use crate::assets::{APP_CSS, APP_JS, DATA_FREE_SHELL};
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
    body: Zeroizing<Vec<u8>>,
    authorization: Option<Zeroizing<Vec<u8>>>,
    page_session: Option<Zeroizing<Vec<u8>>>,
    csrf: Option<Zeroizing<Vec<u8>>>,
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
        self.authorization.as_ref().map(|value| value.as_slice())
    }

    pub(crate) fn page_session(&self) -> Option<&[u8]> {
        self.page_session.as_ref().map(|value| value.as_slice())
    }

    pub(crate) fn csrf(&self) -> Option<&[u8]> {
        self.csrf.as_ref().map(|value| value.as_slice())
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
        if input.is_empty()
            || input.len() > MAX_HTTP_REQUEST_BYTES
            || has_forbidden_control(input)
            || !has_canonical_crlf(input)
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

        let mut seen_headers = 0_u64;
        let mut host = None;
        let mut origin = None;
        let mut authorization = None;
        let mut page_session = None;
        let mut csrf = None;
        let mut content_length = None;
        let mut content_type = None;
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
            let Some((header_id, semantic)) = header_semantic(name) else {
                return rejected();
            };
            let bit = 1_u64 << header_id;
            if seen_headers & bit != 0 {
                return rejected();
            }
            seen_headers |= bit;
            match semantic {
                HeaderSemantic::Host => host = Some(value),
                HeaderSemantic::Origin => origin = Some(value),
                HeaderSemantic::Authorization => authorization = Some(value),
                HeaderSemantic::PageSession => page_session = Some(value),
                HeaderSemantic::Csrf => csrf = Some(value),
                HeaderSemantic::ContentType => content_type = Some(value),
                HeaderSemantic::ContentLength => {
                    let text = std::str::from_utf8(value).map_err(|_| rejected_error())?;
                    if text.starts_with('+')
                        || (text.len() > 1 && text.starts_with('0'))
                        || !text.bytes().all(|byte| byte.is_ascii_digit())
                    {
                        return rejected();
                    }
                    content_length = Some(text.parse::<usize>().map_err(|_| rejected_error())?);
                }
                HeaderSemantic::Passive => {}
            }
        }
        if host != Some(expected_host) {
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
        let is_asset = matches!(
            route,
            ValidatedDashboardRequestV1::Shell
                | ValidatedDashboardRequestV1::Stylesheet
                | ValidatedDashboardRequestV1::Script
        );
        if is_asset {
            if origin.is_some()
                || authorization.is_some()
                || page_session.is_some()
                || csrf.is_some()
                || content_type.is_some()
                || content_length.is_some()
            {
                return rejected();
            }
        } else if origin != Some(expected_origin) {
            return rejected();
        }
        match route {
            ValidatedDashboardRequestV1::PairSession => {
                if authorization.is_none()
                    || page_session.is_some()
                    || csrf.is_some()
                    || content_type != Some(b"application/gaze-dashboard-pair-v1")
                    || body != b"GZDB-PAIR-V1"
                {
                    return rejected();
                }
            }
            ValidatedDashboardRequestV1::Snapshot
            | ValidatedDashboardRequestV1::Follow
            | ValidatedDashboardRequestV1::Purge => {
                if authorization.is_some()
                    || page_session.is_none()
                    || csrf.is_none()
                    || content_type != Some(b"application/json")
                    || body != b"{}"
                {
                    return rejected();
                }
            }
            ValidatedDashboardRequestV1::ProviderVisible
            | ValidatedDashboardRequestV1::RevealOwnerRaw
            | ValidatedDashboardRequestV1::RevealOwnerRestored => {
                if authorization.is_some()
                    || page_session.is_none()
                    || csrf.is_none()
                    || content_type != Some(b"application/json")
                    || body.is_empty()
                {
                    return rejected();
                }
            }
            ValidatedDashboardRequestV1::Shell
            | ValidatedDashboardRequestV1::Stylesheet
            | ValidatedDashboardRequestV1::Script => {}
        }
        Ok(ValidatedRequest {
            route,
            body: Zeroizing::new(body.to_vec()),
            authorization: authorization.map(|value| Zeroizing::new(value.to_vec())),
            page_session: page_session.map(|value| Zeroizing::new(value.to_vec())),
            csrf: csrf.map(|value| Zeroizing::new(value.to_vec())),
        })
    }

    pub(crate) fn shell_response() -> Vec<u8> {
        Self::asset_response("text/html; charset=utf-8", DATA_FREE_SHELL)
    }

    pub(crate) fn stylesheet_response() -> Vec<u8> {
        Self::asset_response("text/css; charset=utf-8", APP_CSS)
    }

    pub(crate) fn script_response() -> Vec<u8> {
        Self::asset_response("text/javascript; charset=utf-8", APP_JS)
    }

    fn asset_response(content_type: &str, body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{}\r\n",
            body.len(),
            SECURITY_HEADERS
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }
}

#[derive(Clone, Copy)]
enum HeaderSemantic {
    Host,
    Origin,
    Authorization,
    PageSession,
    Csrf,
    ContentLength,
    ContentType,
    Passive,
}

fn header_semantic(name: &[u8]) -> Option<(u8, HeaderSemantic)> {
    let known = [
        (b"host".as_slice(), HeaderSemantic::Host),
        (b"origin".as_slice(), HeaderSemantic::Origin),
        (b"authorization".as_slice(), HeaderSemantic::Authorization),
        (
            b"x-gaze-page-session".as_slice(),
            HeaderSemantic::PageSession,
        ),
        (b"x-gaze-csrf".as_slice(), HeaderSemantic::Csrf),
        (b"content-length".as_slice(), HeaderSemantic::ContentLength),
        (b"content-type".as_slice(), HeaderSemantic::ContentType),
        (b"accept".as_slice(), HeaderSemantic::Passive),
        (b"accept-encoding".as_slice(), HeaderSemantic::Passive),
        (b"accept-language".as_slice(), HeaderSemantic::Passive),
        (b"user-agent".as_slice(), HeaderSemantic::Passive),
        (b"sec-fetch-dest".as_slice(), HeaderSemantic::Passive),
        (b"sec-fetch-mode".as_slice(), HeaderSemantic::Passive),
        (b"sec-fetch-site".as_slice(), HeaderSemantic::Passive),
        (b"sec-ch-ua".as_slice(), HeaderSemantic::Passive),
        (b"sec-ch-ua-mobile".as_slice(), HeaderSemantic::Passive),
        (b"sec-ch-ua-platform".as_slice(), HeaderSemantic::Passive),
        (b"dnt".as_slice(), HeaderSemantic::Passive),
        (b"priority".as_slice(), HeaderSemantic::Passive),
        (b"pragma".as_slice(), HeaderSemantic::Passive),
        (b"cache-control".as_slice(), HeaderSemantic::Passive),
    ];
    known
        .iter()
        .enumerate()
        .find(|(_, (known, _))| name.eq_ignore_ascii_case(known))
        .map(|(index, (_, semantic))| (index as u8, *semantic))
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

fn has_canonical_crlf(input: &[u8]) -> bool {
    input.iter().enumerate().all(|(index, byte)| match byte {
        b'\r' => input.get(index + 1) == Some(&b'\n'),
        b'\n' => index > 0 && input[index - 1] == b'\r',
        _ => true,
    })
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

    #[test]
    fn production_assets_are_embedded_byte_for_byte_with_security_headers() {
        for (response, expected) in [
            (DashboardHttp1Gate::shell_response(), DATA_FREE_SHELL),
            (DashboardHttp1Gate::stylesheet_response(), APP_CSS),
            (DashboardHttp1Gate::script_response(), APP_JS),
        ] {
            let boundary = response
                .windows(4)
                .position(|part| part == b"\r\n\r\n")
                .unwrap()
                + 4;
            assert_eq!(&response[boundary..], expected);
            let headers = std::str::from_utf8(&response[..boundary]).unwrap();
            assert!(headers.contains("Cache-Control: no-store"));
            assert!(headers.contains("Content-Security-Policy:"));
            assert!(headers.contains("Clear-Site-Data:"));
        }
    }

    #[test]
    fn pairing_and_post_pair_headers_have_disjoint_credential_contracts() {
        let pair = b"POST /api/v1/session/pair HTTP/1.1\r\nHost: 127.12.34.56:54321\r\nOrigin: http://127.12.34.56:54321\r\nAuthorization: GazeDashboardV1 AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\nContent-Type: application/gaze-dashboard-pair-v1\r\nContent-Length: 12\r\n\r\nGZDB-PAIR-V1";
        assert_eq!(
            DashboardHttp1Gate::validate(pair, HOST, ORIGIN)
                .unwrap()
                .route(),
            ValidatedDashboardRequestV1::PairSession
        );
        let post_pair = b"POST /api/v1/events/snapshot HTTP/1.1\r\nHost: 127.12.34.56:54321\r\nOrigin: http://127.12.34.56:54321\r\nContent-Type: application/json\r\nContent-Length: 2\r\nX-Gaze-Page-Session: AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\nX-Gaze-Csrf: AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\n\r\n{}";
        assert_eq!(
            DashboardHttp1Gate::validate(post_pair, HOST, ORIGIN)
                .unwrap()
                .route(),
            ValidatedDashboardRequestV1::Snapshot
        );
    }
}
