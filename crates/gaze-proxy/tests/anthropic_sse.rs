#![allow(dead_code)]

#[path = "../src/codecs/anthropic.rs"]
mod anthropic;
#[path = "../src/codec.rs"]
mod codec;

use anthropic::{AnthropicMessagesCodec, ANTHROPIC_PROXY_ERROR_FRAME, ANTHROPIC_PROXY_PING_FRAME};
use codec::{
    CodecErrorCode, CodecLimits, ResponseResidualValidator, ResponseTransformContext, WireFormat,
};
use gaze::{PiiClass, Scope, Session};

const EMAIL: &str = "alice@example.invalid";

struct SyntheticResidualValidator;

impl ResponseResidualValidator for SyntheticResidualValidator {
    fn validate(
        &mut self,
        value: &str,
        authorized_output_ranges: &[std::ops::Range<usize>],
    ) -> Result<(), CodecErrorCode> {
        for (start, _) in value.match_indices(EMAIL) {
            let matched = start..start + EMAIL.len();
            if !authorized_output_ranges
                .iter()
                .any(|range| range.start <= matched.start && range.end >= matched.end)
            {
                return Err(CodecErrorCode::ProviderOriginPii);
            }
        }
        Ok(())
    }
}

fn snapshot_with_email() -> (gaze::CommittedSessionSnapshot, String) {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let token = transaction.tokenize(&PiiClass::Email, EMAIL).unwrap();
    (transaction.commit().unwrap(), token)
}

fn frame(event: &str, data: &str) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

fn successful_stream(text_delta: &str) -> Vec<u8> {
    [
        frame(
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#,
        ),
        frame(
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        ),
        frame(
            "content_block_delta",
            &format!(r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{text_delta}"}}}}"#),
        ),
        frame(
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        ),
        frame(
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}"#,
        ),
        frame("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat()
    .into_bytes()
}

#[test]
fn sse_handles_every_chunk_boundary_split_utf8_and_split_manifest_tokens() {
    let codec = AnthropicMessagesCodec;
    let (snapshot, token) = snapshot_with_email();
    let stream = successful_stream(&format!("Grüße {token}"));

    for split in 0..=stream.len() {
        let chunks = [&stream[..split], &stream[split..]];
        let mut residual = SyntheticResidualValidator;
        let mut context = ResponseTransformContext::new(
            &snapshot,
            &mut residual,
            WireFormat::Sse,
            CodecLimits::default(),
        );
        let proved = codec.restore_sse_chunks(chunks, &mut context).unwrap();
        assert!(proved.provenance().final_buffer_verified());
        let output = std::str::from_utf8(proved.bytes()).unwrap();
        assert!(output.contains("Grüße alice@example.invalid"));
        assert!(!output.contains(&token));
    }
}

#[test]
fn sse_interleaved_indices_keep_text_and_partial_json_isolated() {
    let codec = AnthropicMessagesCodec;
    let (snapshot, token) = snapshot_with_email();
    let closing_partial = serde_json::to_string(&format!("{token}\"}}")).unwrap();
    let stream = [
        frame(
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#,
        ),
        frame("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
        frame("content_block_start", r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}"#),
        frame("content_block_delta", &format!(r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{token}"}}}}"#)),
        frame("content_block_delta", r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"email\":\""}}"#),
        frame("content_block_delta", &format!(r#"{{"type":"content_block_delta","index":1,"delta":{{"type":"input_json_delta","partial_json":{closing_partial}}}}}"#)),
        frame("content_block_stop", r#"{"type":"content_block_stop","index":1}"#),
        frame("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        frame("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":1}}"#),
        frame("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat()
    .into_bytes();
    let mut residual = SyntheticResidualValidator;
    let proved = codec
        .restore_sse_chunks(
            [&stream[..]],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Sse,
                CodecLimits::default(),
            ),
        )
        .unwrap();
    let output = std::str::from_utf8(proved.bytes()).unwrap();
    assert!(output.matches(EMAIL).count() >= 2);
}

#[test]
fn sse_lifecycle_index_and_future_event_violations_fail_closed() {
    let codec = AnthropicMessagesCodec;
    let (snapshot, _) = snapshot_with_email();
    let cases = [
        frame("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"synthetic"}}"#),
        [
            frame("message_start", r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#),
            frame("content_block_stop", r#"{"type":"content_block_stop","index":9}"#),
        ].concat(),
        [
            frame("message_start", r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#),
            frame("future_event", r#"{"type":"future_event","text":"alice@example.invalid"}"#),
            frame("message_stop", r#"{"type":"message_stop"}"#),
        ].concat(),
    ];
    for stream in cases {
        let mut residual = SyntheticResidualValidator;
        assert!(codec
            .restore_sse_chunks(
                [stream.as_bytes()],
                &mut ResponseTransformContext::new(
                    &snapshot,
                    &mut residual,
                    WireFormat::Sse,
                    CodecLimits::default(),
                ),
            )
            .is_err());
    }
}

#[test]
fn safe_future_events_and_provider_pings_are_suppressed_and_constants_are_frozen() {
    assert_eq!(
        ANTHROPIC_PROXY_PING_FRAME,
        b"event: ping\ndata: {\"type\":\"ping\"}\n\n"
    );
    assert_eq!(
        ANTHROPIC_PROXY_ERROR_FRAME,
        b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"proxy_validation_failed\"}}\n\n"
    );

    let codec = AnthropicMessagesCodec;
    let (snapshot, _) = snapshot_with_email();
    let stream = [
        frame("ping", r#"{"type":"ping"}"#),
        frame("future_event", r#"{"type":"future_event","supported":true}"#),
        frame("message_start", r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#),
        frame("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat();
    let mut residual = SyntheticResidualValidator;
    let proved = codec
        .restore_sse_chunks(
            [stream.as_bytes()],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Sse,
                CodecLimits::default(),
            ),
        )
        .unwrap();
    let output = std::str::from_utf8(proved.bytes()).unwrap();
    assert!(!output.contains("future_event"));
    assert!(!output.contains("event: ping"));
}

#[test]
fn signed_sse_frames_are_exact_and_signed_failures_are_closed() {
    let codec = AnthropicMessagesCodec;
    let (snapshot, token) = snapshot_with_email();
    let signed_start = "event: content_block_start\r\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\r\n\r\n";
    let thinking_delta = format!(
        "event: content_block_delta\r\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"thinking_delta\",\"thinking\":\"{token}\"}}}}\r\n\r\n"
    );
    let signature_delta = "event: content_block_delta\r\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"opaque-signature\"}}\r\n\r\n";
    let stream = [
        frame("message_start", r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#),
        signed_start.to_owned(),
        thinking_delta.clone(),
        signature_delta.to_owned(),
        frame("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        frame("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}"#),
        frame("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat();
    let mut residual = SyntheticResidualValidator;
    let proved = codec
        .restore_sse_chunks(
            [stream.as_bytes()],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Sse,
                CodecLimits::default(),
            ),
        )
        .unwrap();
    for raw in [
        signed_start.as_bytes(),
        thinking_delta.as_bytes(),
        signature_delta.as_bytes(),
    ] {
        assert!(proved
            .bytes()
            .windows(raw.len())
            .any(|window| window == raw));
    }
    assert!(!proved.provenance().signed_opaque_ranges().is_empty());

    let raw_pii_stream = [
        frame("message_start", r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#),
        frame("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#),
        frame("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"alice@example.invalid"}}"#),
        frame("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        frame("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}"#),
        frame("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat();
    let error = codec
        .restore_sse_chunks(
            [raw_pii_stream.as_bytes()],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Sse,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::ProviderOriginPii);

    let unknown_signature_stream = stream.replace("opaque-signature", "<Email_999>");
    let error = codec
        .restore_sse_chunks(
            [unknown_signature_stream.as_bytes()],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Sse,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::RestoreFailedClosed);
}

#[test]
fn citation_deltas_restore_once_and_keep_order() {
    let codec = AnthropicMessagesCodec;
    let (snapshot, token) = snapshot_with_email();
    let citation = frame(
        "content_block_delta",
        &format!(
            r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"citations_delta","citation":{{"type":"char_location","cited_text":"{token}","document_title":"Synthetic document","document_index":0,"start_char_index":1,"end_char_index":2}}}}}}"#
        ),
    );
    let stream = [
        frame("message_start", r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#),
        frame("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
        citation,
        frame("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        frame("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}"#),
        frame("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat();
    let mut residual = SyntheticResidualValidator;
    let proved = codec
        .restore_sse_chunks(
            [stream.as_bytes()],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Sse,
                CodecLimits::default(),
            ),
        )
        .unwrap();
    let output = std::str::from_utf8(proved.bytes()).unwrap();
    assert!(output.contains(EMAIL));
    assert_eq!(output.matches(r#""type":"citations_delta""#).count(), 1);
    assert_eq!(output.matches(r#""type":"char_location""#).count(), 1);
}

#[test]
fn adjacent_sse_text_blocks_are_scanned_as_one_logical_domain() {
    let codec = AnthropicMessagesCodec;
    let (snapshot, _) = snapshot_with_email();
    let stream = [
        frame("message_start", r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#),
        frame("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
        frame("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"alice@"}}"#),
        frame("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        frame("content_block_start", r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#),
        frame("content_block_delta", r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"example.invalid"}}"#),
        frame("content_block_stop", r#"{"type":"content_block_stop","index":1}"#),
        frame("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}"#),
        frame("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat();
    let mut residual = SyntheticResidualValidator;
    let error = codec
        .restore_sse_chunks(
            [stream.as_bytes()],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Sse,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::ProviderOriginPii);
}

#[test]
fn zero_delta_tool_block_remains_valid_and_empty() {
    let codec = AnthropicMessagesCodec;
    let (snapshot, _) = snapshot_with_email();
    let stream = [
        frame("message_start", r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#),
        frame("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}"#),
        frame("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        frame("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":1}}"#),
        frame("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat();
    let mut residual = SyntheticResidualValidator;
    let proved = codec
        .restore_sse_chunks(
            [stream.as_bytes()],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Sse,
                CodecLimits::default(),
            ),
        )
        .unwrap();
    let output = std::str::from_utf8(proved.bytes()).unwrap();
    assert!(output.contains(r#""input":{}"#));
    assert!(!output.contains("input_json_delta"));
}

#[test]
fn strict_sse_framing_rejects_ambiguous_or_truncated_input() {
    let codec = AnthropicMessagesCodec;
    let (snapshot, _) = snapshot_with_email();
    let terminal = successful_stream("synthetic");
    let cases = [
        b"\xef\xbb\xbfevent: ping\ndata: {\"type\":\"ping\"}\n\n".to_vec(),
        b"event: ping\nevent: ping\ndata: {\"type\":\"ping\"}\n\n".to_vec(),
        b"id: 1\ndata: {\"type\":\"ping\"}\n\n".to_vec(),
        b"retry: 10\ndata: {\"type\":\"ping\"}\n\n".to_vec(),
        b"colonless\n\n".to_vec(),
        b"event: ping\ndata: {\"type\":\"ping\"}\n".to_vec(),
        [terminal.as_slice(), b": late-comment\n\n"].concat(),
    ];
    for input in cases {
        let mut residual = SyntheticResidualValidator;
        assert!(codec
            .restore_sse_chunks(
                [input.as_slice()],
                &mut ResponseTransformContext::new(
                    &snapshot,
                    &mut residual,
                    WireFormat::Sse,
                    CodecLimits::default(),
                ),
            )
            .is_err());
    }

    let mut residual = SyntheticResidualValidator;
    let error = codec
        .restore_sse_chunks(
            [b"event: ping\ndata: {\"type\":\"ping\"}\n\n".as_slice()],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Sse,
                CodecLimits::default().with_max_sse_line_bytes(8),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::SseLineLimitExceeded);

    let cr_only = String::from_utf8(successful_stream("synthetic"))
        .unwrap()
        .replace('\n', "\r");
    let mut residual = SyntheticResidualValidator;
    codec
        .restore_sse_chunks(
            [cr_only.as_bytes()],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Sse,
                CodecLimits::default(),
            ),
        )
        .unwrap();
}

#[test]
fn stopped_sse_index_cannot_be_reused() {
    let codec = AnthropicMessagesCodec;
    let (snapshot, _) = snapshot_with_email();
    let stream = [
        frame("message_start", r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#),
        frame("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
        frame("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        frame("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
        frame("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        frame("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}"#),
        frame("message_stop", r#"{"type":"message_stop"}"#),
    ]
    .concat();
    let mut residual = SyntheticResidualValidator;
    let error = codec
        .restore_sse_chunks(
            [stream.as_bytes()],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Sse,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::InvalidContentBlockIndex);
}

#[test]
fn unsafe_future_event_names_and_keys_fail_closed() {
    let codec = AnthropicMessagesCodec;
    let (snapshot, token) = snapshot_with_email();
    let encoded_key = "A".repeat(80);
    let unsafe_events = [
        frame(
            "alice@example.invalid",
            r#"{"type":"alice@example.invalid","supported":true}"#,
        ),
        frame("data:future", r#"{"type":"data:future","supported":true}"#),
        frame(&token, &format!(r#"{{"type":"{token}","supported":true}}"#)),
        frame(
            "future_event",
            r#"{"type":"future_event","alice@example.invalid":true}"#,
        ),
        frame(
            "future_event",
            r#"{"type":"future_event","data:payload":true}"#,
        ),
        frame(
            "future_event",
            r#"{"type":"future_event","%61%6c%69%63%65":true}"#,
        ),
        frame(
            "future_event",
            &format!(r#"{{"type":"future_event","{encoded_key}":true}}"#),
        ),
        frame("future_event", r#"{"type":"future_event","café":true}"#),
    ];
    for future in unsafe_events {
        let stream = [
            future,
            frame("message_start", r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}"#),
            frame("message_stop", r#"{"type":"message_stop"}"#),
        ]
        .concat();
        let mut residual = SyntheticResidualValidator;
        assert!(codec
            .restore_sse_chunks(
                [stream.as_bytes()],
                &mut ResponseTransformContext::new(
                    &snapshot,
                    &mut residual,
                    WireFormat::Sse,
                    CodecLimits::default(),
                ),
            )
            .is_err());
    }
}
