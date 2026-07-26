use std::error::Error as _;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use gaze_proxy::{
    classify_upstream_status, parse_request_stream_format, validate_declared_body,
    validate_direct_response_head, ProxyErrorCode, ProxyErrorPhase, WireFormat,
    DIRECT_PROXY_ERROR_FRAME, DIRECT_PROXY_PING_FRAME,
};

fn run_internal_behavioral_proof(name: &str) {
    static NESTED_CARGO: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = NESTED_CARGO.get_or_init(|| Mutex::new(())).lock().unwrap();
    let output = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "test",
            "--locked",
            "-p",
            "gaze-proxy",
            "--all-features",
            "--lib",
            name,
            "--",
            "--exact",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "internal behavioral proof {name} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn lifecycle_commit_and_exact_single_send_are_real_loopback_proofs() {
    run_internal_behavioral_proof(
        "server::tests::direct_send_requires_commit_is_single_use_and_captures_exact_proof",
    );
}

#[test]
fn ambient_proxy_environment_is_ignored_by_the_internal_direct_client() {
    run_internal_behavioral_proof("server::tests::proxy_environment_is_ignored_in_isolated_helper");
}

#[test]
fn overlimit_proved_request_rejects_before_wire_io() {
    run_internal_behavioral_proof(
        "server::tests::overlimit_proved_request_and_duplicate_credentials_do_zero_io",
    );
}

#[test]
fn timeout_and_body_failure_retain_the_nonforkable_io_state() {
    run_internal_behavioral_proof(
        "server::tests::timeout_and_body_stream_error_retain_io_attempted",
    );
}

#[test]
fn direct_transport_internals_are_not_public() {
    let probe = tempfile::tempdir().unwrap();
    std::fs::create_dir(probe.path().join("src")).unwrap();
    let manifest_dir = env!("CARGO_MANIFEST_DIR").replace('\\', "\\\\");
    std::fs::write(
        probe.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"gaze-proxy-api-probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\ngaze-proxy = {{ path = \"{manifest_dir}\" }}\n"
        ),
    )
    .unwrap();
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("Cargo.lock"),
        probe.path().join("Cargo.lock"),
    )
    .unwrap();
    std::fs::write(
        probe.path().join("src/main.rs"),
        r#"use gaze_proxy::{
    DirectClient, DirectClientConfig, DirectRequest, DirectResponse, DirectTransactionState,
};

fn main() {
    let lifecycle = DirectTransactionState::new();
    let fork = lifecycle.clone();
    let _ = (lifecycle, fork);
    let _raw_request = DirectRequest::try_new;
    let _send = DirectClient::execute;
    let _raw_response = DirectResponse::into_body;
    let _config = DirectClientConfig::default();
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO"))
        .args([
            "check",
            "--offline",
            "--quiet",
            "--manifest-path",
            probe.path().join("Cargo.toml").to_str().unwrap(),
        ])
        .env("CARGO_TARGET_DIR", probe.path().join("target"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "direct transport internals unexpectedly remained public"
    );
    for forbidden in [
        "DirectClient",
        "DirectClientConfig",
        "DirectRequest",
        "DirectResponse",
        "DirectTransactionState",
    ] {
        assert!(
            stderr.contains(forbidden),
            "API probe failed without proving {forbidden} inaccessible: {stderr}"
        );
    }
}

#[test]
fn closed_errors_have_reviewed_statuses_and_sanitized_rendering() {
    let cases = [
        (ProxyErrorCode::RouteRejected, StatusCode::NOT_FOUND),
        (ProxyErrorCode::HeaderRejected, StatusCode::BAD_REQUEST),
        (
            ProxyErrorCode::SessionIdentityRequired,
            StatusCode::BAD_REQUEST,
        ),
        (
            ProxyErrorCode::UnexpectedSessionHeader,
            StatusCode::BAD_REQUEST,
        ),
        (ProxyErrorCode::PrincipalRequired, StatusCode::UNAUTHORIZED),
        (ProxyErrorCode::PrincipalRejected, StatusCode::FORBIDDEN),
        (
            ProxyErrorCode::RegistryCapacity,
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (ProxyErrorCode::SessionExpired, StatusCode::GONE),
        (
            ProxyErrorCode::SessionGenerationConflict,
            StatusCode::CONFLICT,
        ),
        (
            ProxyErrorCode::InvalidUpstreamUrl,
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            ProxyErrorCode::ProxyConfiguration,
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            ProxyErrorCode::InvalidUpstreamHeader,
            StatusCode::BAD_GATEWAY,
        ),
        (
            ProxyErrorCode::UnsupportedContentEncoding,
            StatusCode::BAD_GATEWAY,
        ),
        (ProxyErrorCode::InvalidFraming, StatusCode::BAD_GATEWAY),
        (
            ProxyErrorCode::RequestBodyLimitExceeded,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            ProxyErrorCode::ResponseBodyLimitExceeded,
            StatusCode::BAD_GATEWAY,
        ),
        (ProxyErrorCode::HeaderLimitExceeded, StatusCode::BAD_GATEWAY),
        (
            ProxyErrorCode::InvalidRequestFormat,
            StatusCode::BAD_REQUEST,
        ),
        (
            ProxyErrorCode::InvalidUpstreamResponseFormat,
            StatusCode::BAD_GATEWAY,
        ),
        (ProxyErrorCode::DuplicateObjectKey, StatusCode::BAD_REQUEST),
        (
            ProxyErrorCode::InternalCoverageFailure,
            StatusCode::BAD_GATEWAY,
        ),
        (
            ProxyErrorCode::ControlWouldMutate,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            ProxyErrorCode::OpaqueMediaUninspected,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (ProxyErrorCode::InvalidProvenance, StatusCode::BAD_GATEWAY),
        (ProxyErrorCode::InvalidToken, StatusCode::BAD_GATEWAY),
        (
            ProxyErrorCode::SignedMutationRequired,
            StatusCode::BAD_GATEWAY,
        ),
        (
            ProxyErrorCode::SignedSurfaceMalformed,
            StatusCode::BAD_GATEWAY,
        ),
        (ProxyErrorCode::InvalidSseLifecycle, StatusCode::BAD_GATEWAY),
        (
            ProxyErrorCode::InvalidContentBlockIndex,
            StatusCode::BAD_GATEWAY,
        ),
        (ProxyErrorCode::ConnectTimeout, StatusCode::GATEWAY_TIMEOUT),
        (ProxyErrorCode::RequestTimeout, StatusCode::GATEWAY_TIMEOUT),
        (ProxyErrorCode::TotalTimeout, StatusCode::GATEWAY_TIMEOUT),
        (ProxyErrorCode::UpstreamRedirect, StatusCode::BAD_GATEWAY),
        (ProxyErrorCode::UpstreamBadRequest, StatusCode::BAD_REQUEST),
        (
            ProxyErrorCode::UpstreamUnauthorized,
            StatusCode::UNAUTHORIZED,
        ),
        (ProxyErrorCode::UpstreamForbidden, StatusCode::FORBIDDEN),
        (ProxyErrorCode::UpstreamNotFound, StatusCode::NOT_FOUND),
        (ProxyErrorCode::UpstreamConflict, StatusCode::CONFLICT),
        (
            ProxyErrorCode::UpstreamPayloadTooLarge,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            ProxyErrorCode::UpstreamRateLimited,
            StatusCode::TOO_MANY_REQUESTS,
        ),
        (
            ProxyErrorCode::UpstreamClientFailure,
            StatusCode::BAD_GATEWAY,
        ),
        (
            ProxyErrorCode::UpstreamUnavailable,
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            ProxyErrorCode::UpstreamServerFailure,
            StatusCode::BAD_GATEWAY,
        ),
        (ProxyErrorCode::UpstreamProtocol, StatusCode::BAD_GATEWAY),
        (ProxyErrorCode::UpstreamUnreachable, StatusCode::BAD_GATEWAY),
        (
            ProxyErrorCode::InspectionInternal,
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (
            ProxyErrorCode::InvalidStateTransition,
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ];

    for (code, status) in cases {
        let error = code.error(ProxyErrorPhase::ResponseValidation);
        assert_eq!(error.http_status(), status);
        assert_eq!(error.sse_error_frame(), DIRECT_PROXY_ERROR_FRAME);
        assert!(error.source().is_none());
        for rendered in [
            format!("{error}"),
            format!("{error:?}"),
            format!("{error:#?}"),
        ] {
            assert!(rendered.contains(&format!("{code:?}")));
            assert!(rendered.contains("ResponseValidation"));
            assert!(!rendered.contains("alice@example.invalid"));
            assert!(!rendered.contains("sk-ant-synthetic-secret"));
            assert!(!rendered.contains("https://upstream.invalid/private?secret=1"));
        }
    }
}

#[test]
fn stream_control_is_closed_and_duplicate_safe() {
    assert_eq!(
        parse_request_stream_format(br#"{}"#).unwrap(),
        WireFormat::Json
    );
    assert_eq!(
        parse_request_stream_format(br#"{"stream":false}"#).unwrap(),
        WireFormat::Json
    );
    assert_eq!(
        parse_request_stream_format(br#"{"stream":true}"#).unwrap(),
        WireFormat::Sse
    );

    for body in [
        br#"{"stream":null}"#.as_slice(),
        br#"{"stream":"true"}"#.as_slice(),
        br#"[]"#.as_slice(),
        br#"not-json"#.as_slice(),
    ] {
        assert_eq!(
            parse_request_stream_format(body).unwrap_err().code(),
            ProxyErrorCode::InvalidRequestFormat
        );
    }
    assert_eq!(
        parse_request_stream_format(br#"{"stream":false,"stream":true}"#)
            .unwrap_err()
            .code(),
        ProxyErrorCode::DuplicateObjectKey
    );
}

#[test]
fn direct_status_format_and_retry_after_matrix_is_exact() {
    let mut json = HeaderMap::new();
    json.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    let mut sse = HeaderMap::new();
    sse.insert(
        "content-type",
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );

    let json_head = validate_direct_response_head(StatusCode::OK, &json, WireFormat::Json).unwrap();
    assert_eq!(json_head.format(), WireFormat::Json);
    assert_eq!(json_head.headers().len(), 1);
    assert!(validate_direct_response_head(StatusCode::OK, &json, WireFormat::Sse).is_err());
    assert!(validate_direct_response_head(StatusCode::OK, &sse, WireFormat::Json).is_err());
    assert!(validate_direct_response_head(StatusCode::OK, &sse, WireFormat::Sse).is_ok());
    assert!(
        validate_direct_response_head(StatusCode::OK, &HeaderMap::new(), WireFormat::Json).is_err()
    );

    let rows = [
        (400, ProxyErrorCode::UpstreamBadRequest, 400),
        (401, ProxyErrorCode::UpstreamUnauthorized, 401),
        (403, ProxyErrorCode::UpstreamForbidden, 403),
        (404, ProxyErrorCode::UpstreamNotFound, 404),
        (409, ProxyErrorCode::UpstreamConflict, 409),
        (413, ProxyErrorCode::UpstreamPayloadTooLarge, 413),
        (429, ProxyErrorCode::UpstreamRateLimited, 429),
        (418, ProxyErrorCode::UpstreamClientFailure, 502),
        (502, ProxyErrorCode::UpstreamUnavailable, 503),
        (503, ProxyErrorCode::UpstreamUnavailable, 503),
        (504, ProxyErrorCode::UpstreamUnavailable, 503),
        (529, ProxyErrorCode::UpstreamUnavailable, 503),
        (500, ProxyErrorCode::UpstreamServerFailure, 502),
    ];
    for (upstream, code, downstream) in rows {
        let error = classify_upstream_status(StatusCode::from_u16(upstream).unwrap()).unwrap_err();
        assert_eq!(error.code(), code);
        assert_eq!(
            error.http_status(),
            StatusCode::from_u16(downstream).unwrap()
        );
    }
    for redirect in 300..=399 {
        assert_eq!(
            classify_upstream_status(StatusCode::from_u16(redirect).unwrap())
                .unwrap_err()
                .code(),
            ProxyErrorCode::UpstreamRedirect
        );
    }

    let mut retry = HeaderMap::new();
    retry.insert("retry-after", HeaderValue::from_static("17"));
    let error =
        validate_direct_response_head(StatusCode::TOO_MANY_REQUESTS, &retry, WireFormat::Json)
            .unwrap_err();
    assert_eq!(error.code(), ProxyErrorCode::UpstreamRateLimited);
    assert_eq!(error.retry_after_seconds(), Some(17));

    retry.insert(
        "retry-after",
        HeaderValue::from_static("alice@example.invalid"),
    );
    assert_eq!(
        validate_direct_response_head(StatusCode::TOO_MANY_REQUESTS, &retry, WireFormat::Json)
            .unwrap_err()
            .code(),
        ProxyErrorCode::InvalidUpstreamHeader
    );

    let mut malformed_framing = HeaderMap::new();
    malformed_framing.insert("content-length", HeaderValue::from_static("not-a-length"));
    assert_eq!(
        validate_direct_response_head(
            StatusCode::TOO_MANY_REQUESTS,
            &malformed_framing,
            WireFormat::Json,
        )
        .unwrap_err()
        .code(),
        ProxyErrorCode::InvalidFraming
    );

    let mut duplicate_retry = HeaderMap::new();
    duplicate_retry.append("retry-after", HeaderValue::from_static("1"));
    duplicate_retry.append("retry-after", HeaderValue::from_static("2"));
    assert_eq!(
        validate_direct_response_head(
            StatusCode::TOO_MANY_REQUESTS,
            &duplicate_retry,
            WireFormat::Json,
        )
        .unwrap_err()
        .code(),
        ProxyErrorCode::InvalidUpstreamHeader
    );
}

#[test]
fn every_declared_format_is_deterministic_and_raw_headers_fail_closed() {
    assert!(validate_direct_response_head(
        StatusCode::NO_CONTENT,
        &HeaderMap::new(),
        WireFormat::Empty
    )
    .is_ok());
    assert!(validate_declared_body(WireFormat::Empty, b"", 1).is_ok());
    assert!(validate_declared_body(WireFormat::Empty, b"x", 1).is_err());

    assert!(validate_declared_body(WireFormat::Json, br#"{"ok":true}"#, 64).is_ok());
    assert_eq!(
        validate_declared_body(WireFormat::Json, br#"{"ok":true,"ok":false}"#, 64)
            .unwrap_err()
            .code(),
        ProxyErrorCode::DuplicateObjectKey
    );
    assert!(validate_declared_body(WireFormat::Ndjson, b"{\"n\":1}\n\n{\"n\":2}\n", 64).is_ok());
    assert!(validate_declared_body(WireFormat::Ndjson, b"plain text", 64).is_err());
    assert!(validate_declared_body(WireFormat::Utf8Text, b"plain text", 64).is_ok());
    assert!(validate_declared_body(WireFormat::Utf8Text, &[0xff], 64).is_err());
    assert!(validate_declared_body(
        WireFormat::Sse,
        b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        128
    )
    .is_ok());
    assert!(validate_declared_body(WireFormat::Sse, b"truncated", 64).is_err());
    assert!(validate_declared_body(WireFormat::Sse, b"\xef\xbb\xbfevent: ping\n\n", 64).is_err());

    let formats = [
        ("application/x-ndjson", WireFormat::Ndjson),
        ("text/plain; charset=utf-8", WireFormat::Utf8Text),
        ("text/event-stream", WireFormat::Sse),
    ];
    for (content_type, format) in formats {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_str(content_type).unwrap());
        assert!(validate_direct_response_head(StatusCode::OK, &headers, format).is_ok());
    }

    let mut framing = HeaderMap::new();
    framing.insert("content-type", HeaderValue::from_static("application/json"));
    framing.insert("content-length", HeaderValue::from_static("2"));
    framing.insert("transfer-encoding", HeaderValue::from_static("chunked"));
    assert_eq!(
        validate_direct_response_head(StatusCode::OK, &framing, WireFormat::Json)
            .unwrap_err()
            .code(),
        ProxyErrorCode::InvalidFraming
    );

    let mut duplicate_length = HeaderMap::new();
    duplicate_length.insert("content-type", HeaderValue::from_static("application/json"));
    duplicate_length.append("content-length", HeaderValue::from_static("2"));
    duplicate_length.append("content-length", HeaderValue::from_static("3"));
    assert_eq!(
        validate_direct_response_head(StatusCode::OK, &duplicate_length, WireFormat::Json)
            .unwrap_err()
            .code(),
        ProxyErrorCode::InvalidUpstreamHeader
    );

    let mut encoded = HeaderMap::new();
    encoded.insert("content-type", HeaderValue::from_static("application/json"));
    encoded.insert("content-encoding", HeaderValue::from_static("deflate"));
    assert_eq!(
        validate_direct_response_head(StatusCode::OK, &encoded, WireFormat::Json)
            .unwrap_err()
            .code(),
        ProxyErrorCode::UnsupportedContentEncoding
    );

    let mut invalid_mime = HeaderMap::new();
    invalid_mime.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset"),
    );
    assert_eq!(
        validate_direct_response_head(StatusCode::OK, &invalid_mime, WireFormat::Json)
            .unwrap_err()
            .code(),
        ProxyErrorCode::InvalidUpstreamResponseFormat
    );

    let mut too_many = HeaderMap::new();
    too_many.insert("content-type", HeaderValue::from_static("application/json"));
    for index in 0..64 {
        let name: HeaderName = format!("x-synthetic-{index:02}").parse().unwrap();
        too_many.insert(name, HeaderValue::from_static("bounded"));
    }
    assert_eq!(
        validate_direct_response_head(StatusCode::OK, &too_many, WireFormat::Json)
            .unwrap_err()
            .code(),
        ProxyErrorCode::HeaderLimitExceeded
    );
}

#[test]
fn constants_are_frozen_and_match_the_anthropic_codec() {
    assert_eq!(
        DIRECT_PROXY_PING_FRAME,
        b"event: ping\ndata: {\"type\":\"ping\"}\n\n"
    );
    assert_eq!(
        DIRECT_PROXY_ERROR_FRAME,
        b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"proxy_validation_failed\"}}\n\n"
    );
    assert_eq!(
        DIRECT_PROXY_PING_FRAME,
        gaze_proxy::ANTHROPIC_PROXY_PING_FRAME
    );
    assert_eq!(
        DIRECT_PROXY_ERROR_FRAME,
        gaze_proxy::ANTHROPIC_PROXY_ERROR_FRAME
    );
}
