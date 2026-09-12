//! Real-browser navigation header contract for the dashboard raw HTTP/1.1 gate.
//!
//! `DashboardHttp1Gate::validate` is the single connection-level gate every
//! request to the browser dashboard must pass before any handler runs. It
//! whitelists a fixed set of header names and rejects any request carrying a
//! non-whitelisted header with `HttpRejected` (constant 400).
//!
//! Real browsers unconditionally send `Upgrade-Insecure-Requests` and
//! `Sec-Fetch-User` on a top-level user-activated navigation to the shell
//! (`GET /`). These tests pin the gate to *accept* a realistic Chrome® desktop
//! navigation header set through the real `DashboardHttp1Gate::validate` (no
//! mocks) so the dashboard HTML shell is reachable, and to keep accepting the
//! realistic subresource loads the shell triggers. They also guard regressions
//! that the fix must not loosen: unknown and connection-specific headers stay
//! rejected, duplicate headers stay rejected, and the asset-route credential
//! contract is unaffected by the two newly-whitelisted `Passive` headers.

use gaze_proxy_dashboard::{DashboardErrorCode, DashboardHttp1Gate, ValidatedDashboardRequestV1};

const HOST: &[u8] = b"127.99.88.77:43123";
const ORIGIN: &[u8] = b"http://127.99.88.77:43123";

/// A realistic Chrome 126 desktop `GET /` navigation header set, as captured
/// from a real top-level user-activated navigation to a loopback origin.
const REALISTIC_TOP_LEVEL_NAVIGATION: &[u8] = b"GET / HTTP/1.1\r\n\
    Host: 127.99.88.77:43123\r\n\
    Upgrade-Insecure-Requests: 1\r\n\
    User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36\r\n\
    Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r\n\
    Sec-Fetch-Site: none\r\n\
    Sec-Fetch-Mode: navigate\r\n\
    Sec-Fetch-User: ?1\r\n\
    Sec-Fetch-Dest: document\r\n\
    Accept-Encoding: gzip, deflate\r\n\
    Accept-Language: en-US,en;q=0.9\r\n\
    sec-ch-ua: \"Not/A)Brand\";v=\"8\", \"Chromium\";v=\"126\", \"Google Chrome\";v=\"126\"\r\n\
    sec-ch-ua-mobile: ?0\r\n\
    sec-ch-ua-platform: \"Linux\"\r\n\
    \r\n";

/// The realistic navigation with the two navigation-only headers removed —
/// every remaining header was already whitelisted before the fix.
const REALISTIC_TOP_LEVEL_NAVIGATION_WITHOUT_NAV_HEADERS: &[u8] = b"GET / HTTP/1.1\r\n\
    Host: 127.99.88.77:43123\r\n\
    User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36\r\n\
    Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r\n\
    Sec-Fetch-Site: none\r\n\
    Sec-Fetch-Mode: navigate\r\n\
    Sec-Fetch-Dest: document\r\n\
    Accept-Encoding: gzip, deflate\r\n\
    Accept-Language: en-US,en;q=0.9\r\n\
    sec-ch-ua: \"Not/A)Brand\";v=\"8\", \"Chromium\";v=\"126\", \"Google Chrome\";v=\"126\"\r\n\
    sec-ch-ua-mobile: ?0\r\n\
    sec-ch-ua-platform: \"Linux\"\r\n\
    \r\n";

/// A realistic `<script src="/assets/app.js">` request from a page served by
/// the dashboard (which sets `Referrer-Policy: no-referrer`). `Upgrade-Insecure
/// -Requests` is navigation-only and absent from subresource fetches.
const REALISTIC_SUBRESOURCE_LOAD: &[u8] = b"GET /assets/app.js HTTP/1.1\r\n\
    Host: 127.99.88.77:43123\r\n\
    Sec-Fetch-Dest: script\r\n\
    Sec-Fetch-Mode: no-cors\r\n\
    Sec-Fetch-Site: same-origin\r\n\
    Accept: */*\r\n\
    Accept-Encoding: gzip, deflate\r\n\
    Accept-Language: en-US,en;q=0.9\r\n\
    sec-ch-ua: \"Not/A)Brand\";v=\"8\", \"Chromium\";v=\"126\", \"Google Chrome\";v=\"126\"\r\n\
    sec-ch-ua-mobile: ?0\r\n\
    sec-ch-ua-platform: \"Linux\"\r\n\
    \r\n";

#[test]
fn baseline_host_only_navigation_is_accepted() {
    let request = b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\n\r\n";
    assert_eq!(
        DashboardHttp1Gate::validate(request, HOST, ORIGIN)
            .expect("host-only navigation must reach the shell")
            .route(),
        ValidatedDashboardRequestV1::Shell
    );
}

#[test]
fn upgrade_insecure_requests_alone_is_accepted_as_passive() {
    let request =
        b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\nUpgrade-Insecure-Requests: 1\r\n\r\n";
    assert_eq!(
        DashboardHttp1Gate::validate(request, HOST, ORIGIN)
            .expect("Upgrade-Insecure-Requests is a standard navigation header")
            .route(),
        ValidatedDashboardRequestV1::Shell
    );
}

#[test]
fn sec_fetch_user_alone_is_accepted_as_passive() {
    let request = b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\nSec-Fetch-User: ?1\r\n\r\n";
    assert_eq!(
        DashboardHttp1Gate::validate(request, HOST, ORIGIN)
            .expect("Sec-Fetch-User is a Fetch Metadata navigation header")
            .route(),
        ValidatedDashboardRequestV1::Shell
    );
}

#[test]
fn sec_fetch_user_false_value_is_accepted_as_passive() {
    let request = b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\nSec-Fetch-User: ?0\r\n\r\n";
    assert!(DashboardHttp1Gate::validate(request, HOST, ORIGIN).is_ok());
}

#[test]
fn upgrade_insecure_requests_and_sec_fetch_user_together_are_accepted() {
    let request = b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\nUpgrade-Insecure-Requests: 1\r\nSec-Fetch-User: ?1\r\n\r\n";
    assert_eq!(
        DashboardHttp1Gate::validate(request, HOST, ORIGIN)
            .expect("both navigation headers must be accepted together")
            .route(),
        ValidatedDashboardRequestV1::Shell
    );
}

#[test]
fn header_name_matching_is_case_insensitive() {
    for name in [
        "Upgrade-Insecure-Requests",
        "UPGRADE-INSECURE-REQUESTS",
        "upgrade-insecure-requests",
        "Sec-Fetch-User",
        "SEC-FETCH-USER",
        "sec-fetch-user",
    ] {
        let request = format!("GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\n{name}: 1\r\n\r\n");
        assert!(
            DashboardHttp1Gate::validate(request.as_bytes(), HOST, ORIGIN).is_ok(),
            "header {name:?} must match case-insensitively"
        );
    }
}

#[test]
fn realistic_chrome_top_level_navigation_is_accepted_and_routes_to_shell() {
    let validated = DashboardHttp1Gate::validate(REALISTIC_TOP_LEVEL_NAVIGATION, HOST, ORIGIN)
        .expect("a realistic Chrome top-level navigation must reach the shell");
    assert_eq!(validated.route(), ValidatedDashboardRequestV1::Shell);
}

#[test]
fn realistic_chrome_top_level_navigation_without_nav_headers_still_accepted() {
    // Control: the previously-whitelisted remainder is accepted both before and
    // after the fix — the two navigation headers are additions, not a
    // replacement of any prior allowance.
    assert!(DashboardHttp1Gate::validate(
        REALISTIC_TOP_LEVEL_NAVIGATION_WITHOUT_NAV_HEADERS,
        HOST,
        ORIGIN
    )
    .is_ok());
}

#[test]
fn realistic_chrome_subresource_load_is_accepted_and_routes_to_script() {
    let validated = DashboardHttp1Gate::validate(REALISTIC_SUBRESOURCE_LOAD, HOST, ORIGIN)
        .expect("a realistic Chrome subresource load must be accepted");
    assert_eq!(validated.route(), ValidatedDashboardRequestV1::Script);
}

#[test]
fn full_fetch_metadata_set_with_sec_fetch_user_is_accepted() {
    // `Sec-Fetch-User` is the fourth Fetch Metadata header; it must be accepted
    // alongside its already-whitelisted `Dest`/`Mode`/`Site` companions.
    let request = b"GET / HTTP/1.1\r\n\
        Host: 127.99.88.77:43123\r\n\
        Sec-Fetch-Site: none\r\n\
        Sec-Fetch-Mode: navigate\r\n\
        Sec-Fetch-User: ?1\r\n\
        Sec-Fetch-Dest: document\r\n\
        \r\n";
    assert!(DashboardHttp1Gate::validate(request, HOST, ORIGIN).is_ok());
}

#[test]
fn navigation_headers_do_not_satisfy_origin_contract_on_post_routes() {
    // The two new headers are `Passive` and must not relax the origin/credential
    // contract for non-asset routes. A POST snapshot request needs an exact
    // Origin match; adding the navigation headers must not let it through
    // without the rest of the credential contract.
    let request = b"POST /api/v1/events/snapshot HTTP/1.1\r\n\
        Host: 127.99.88.77:43123\r\n\
        Upgrade-Insecure-Requests: 1\r\n\
        Sec-Fetch-User: ?1\r\n\
        Content-Type: application/json\r\n\
        Content-Length: 2\r\n\
        X-Gaze-Page-Session: AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\n\
        X-Gaze-Csrf: AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\n\
        \r\n{}";
    // Missing the required exact Origin header -> still rejected.
    let Err(error) = DashboardHttp1Gate::validate(request, HOST, ORIGIN) else {
        panic!("Passive navigation headers must not relax the Origin contract");
    };
    assert_eq!(error.code(), DashboardErrorCode::HttpRejected);
}

#[test]
fn passive_navigation_headers_are_allowed_on_a_well_formed_post_route() {
    // Adding the two new Passive headers to a contract-correct POST must not
    // break acceptance — they must be transparent to the credential contract.
    let request = b"POST /api/v1/events/snapshot HTTP/1.1\r\n\
        Host: 127.99.88.77:43123\r\n\
        Origin: http://127.99.88.77:43123\r\n\
        Upgrade-Insecure-Requests: 1\r\n\
        Sec-Fetch-User: ?1\r\n\
        Content-Type: application/json\r\n\
        Content-Length: 2\r\n\
        X-Gaze-Page-Session: AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\n\
        X-Gaze-Csrf: AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\n\
        \r\n{}";
    assert_eq!(
        DashboardHttp1Gate::validate(request, HOST, ORIGIN)
            .expect("well-formed POST must accept the new Passive headers")
            .route(),
        ValidatedDashboardRequestV1::Snapshot
    );
}

#[test]
fn duplicate_navigation_header_is_still_rejected() {
    // The duplicate-header invariant must hold for the newly-whitelisted names.
    for request in [
        b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\nUpgrade-Insecure-Requests: 1\r\nUpgrade-Insecure-Requests: 1\r\n\r\n"
            .as_slice(),
        b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\nSec-Fetch-User: ?1\r\nSec-Fetch-User: ?1\r\n\r\n"
            .as_slice(),
    ] {
        let Err(error) = DashboardHttp1Gate::validate(request, HOST, ORIGIN) else {
            panic!("duplicate navigation header must be rejected");
        };
        assert_eq!(error.code(), DashboardErrorCode::HttpRejected);
    }
}

#[test]
fn unknown_and_connection_specific_headers_stay_rejected_with_navigation_headers_present() {
    // The fix broadens the allow-list; it must not turn the gate into a
    // deny-list. Unknown and hop-by-hop smuggling vectors stay rejected even
    // when intermixed with the now-accepted navigation headers.
    for name in [
        "X-Unknown",
        "Connection",
        "Transfer-Encoding",
        "TE",
        "Cookie",
        "Upgrade",
    ] {
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: 127.99.88.77:43123\r\n\
             Upgrade-Insecure-Requests: 1\r\n\
             Sec-Fetch-User: ?1\r\n\
             {name}: close\r\n\
             \r\n"
        );
        let Err(error) = DashboardHttp1Gate::validate(request.as_bytes(), HOST, ORIGIN) else {
            panic!("forbidden header {name:?} must still be rejected");
        };
        assert_eq!(error.code(), DashboardErrorCode::HttpRejected);
    }
}

#[test]
fn realistic_navigation_with_unknown_header_dump_is_rejected() {
    let mut exceeding = REALISTIC_TOP_LEVEL_NAVIGATION.to_vec();
    // Insert an unknown header right before the final blank line.
    let insertion = b"X-Unknown: no\r\n";
    let tail_pos = exceeding
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("blank line present");
    // Insert after the previous header terminator and before the blank line.
    exceeding.splice(tail_pos + 2..tail_pos + 2, insertion.iter().copied());
    let Err(error) = DashboardHttp1Gate::validate(&exceeding, HOST, ORIGIN) else {
        panic!("a navigation carrying an unknown header must be rejected");
    };
    assert_eq!(error.code(), DashboardErrorCode::HttpRejected);
}
