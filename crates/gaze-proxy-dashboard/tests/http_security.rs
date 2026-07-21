use gaze_proxy_dashboard::{
    DashboardErrorCode, DashboardHttp1Gate, ValidatedDashboardRequestV1,
    CONSTANT_REJECTION_RESPONSE,
};

const HOST: &[u8] = b"127.99.88.77:43123";
const ORIGIN: &[u8] = b"http://127.99.88.77:43123";

#[test]
fn raw_gate_accepts_only_one_canonical_origin_form_request() {
    let request = b"POST /api/v1/events/snapshot HTTP/1.1\r\nHost: 127.99.88.77:43123\r\nOrigin: http://127.99.88.77:43123\r\nContent-Length: 0\r\nX-Gaze-Page-Session: AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\nX-Gaze-Csrf: AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\n\r\n";
    let validated = DashboardHttp1Gate::validate(request, HOST, ORIGIN).expect("canonical request");
    assert_eq!(validated.route(), ValidatedDashboardRequestV1::Snapshot);
}

#[test]
fn smuggling_rebinding_cors_and_ambient_auth_all_fail_closed() {
    let cases: &[&[u8]] = &[
        b"GET http://127.99.88.77:43123/ HTTP/1.1\r\nHost: 127.99.88.77:43123\r\n\r\n",
        b"CONNECT / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\nHost: 127.99.88.77:43123\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\nCookie: ambient=yes\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\nUpgrade: h2c\r\n\r\n",
        b"OPTIONS /api/v1/events/snapshot HTTP/1.1\r\nHost: 127.99.88.77:43123\r\nOrigin: http://127.99.88.77:43123\r\n\r\n",
        b"GET /?secret=x HTTP/1.1\r\nHost: 127.99.88.77:43123\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: 127.0.0.1:43123\r\n\r\n",
        b"GET / HTTP/1.1\nHost: 127.99.88.77:43123\n\n",
        b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\rX-Test: no\r\n\r\n",
    ];
    for request in cases {
        let Err(error) = DashboardHttp1Gate::validate(request, HOST, ORIGIN) else {
            panic!("forbidden request passed the raw gate");
        };
        assert_eq!(error.code(), DashboardErrorCode::HttpRejected);
    }
    let response = std::str::from_utf8(CONSTANT_REJECTION_RESPONSE).unwrap();
    assert!(response.contains("Content-Security-Policy: default-src 'none'"));
    assert!(response.contains("Cache-Control: no-store"));
    assert!(!response.contains("Access-Control-Allow-Origin"));
}
