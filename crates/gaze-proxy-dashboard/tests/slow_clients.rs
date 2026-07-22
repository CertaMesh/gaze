use gaze_proxy_dashboard::{DashboardErrorCode, DashboardHttp1Gate, MAX_HTTP_REQUEST_BYTES};

const HOST: &[u8] = b"127.99.88.77:43123";
const ORIGIN: &[u8] = b"http://127.99.88.77:43123";

#[test]
fn partial_oversize_and_trailing_requests_fail_at_the_raw_gate() {
    let requests = [
        b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\n".to_vec(),
        vec![b'x'; MAX_HTTP_REQUEST_BYTES + 1],
        b"GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\n\r\nGET / HTTP/1.1\r\n\r\n".to_vec(),
    ];
    for request in requests {
        let Err(error) = DashboardHttp1Gate::validate(&request, HOST, ORIGIN) else {
            panic!("forbidden request passed the raw gate");
        };
        assert_eq!(error.code(), DashboardErrorCode::HttpRejected);
    }
}

#[test]
fn unknown_and_connection_specific_headers_are_never_ignored() {
    for name in ["X-Unknown", "Connection", "Transfer-Encoding", "TE"] {
        let request =
            format!("GET / HTTP/1.1\r\nHost: 127.99.88.77:43123\r\n{name}: close\r\n\r\n");
        assert!(DashboardHttp1Gate::validate(request.as_bytes(), HOST, ORIGIN).is_err());
    }
}
