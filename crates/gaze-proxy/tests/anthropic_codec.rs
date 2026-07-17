#![allow(dead_code)]

#[path = "../src/codecs/anthropic.rs"]
mod anthropic;
#[path = "../src/codec.rs"]
mod codec;

use anthropic::AnthropicMessagesCodec;
use codec::{
    BodyCodec, CodecErrorCode, CodecLimits, RequestPseudonymizer, RequestTransformContext,
    ResponseResidualValidator, ResponseTransformContext, WireFormat,
};
use gaze::{PiiClass, Scope, Session, SessionTransaction};

const EMAIL: &str = "alice@example.invalid";

struct SyntheticPseudonymizer<'a, 'session> {
    transaction: &'a mut SessionTransaction<'session>,
}

impl RequestPseudonymizer for SyntheticPseudonymizer<'_, '_> {
    fn protect(&mut self, input: &str) -> Result<String, CodecErrorCode> {
        if !input.contains(EMAIL) {
            return Ok(input.to_owned());
        }
        let token = self
            .transaction
            .tokenize(&PiiClass::Email, EMAIL)
            .map_err(|_| CodecErrorCode::ProtectionFailedClosed)?;
        Ok(input.replace(EMAIL, &token))
    }

    fn validate_token_shapes(&mut self, input: &str) -> Result<(), CodecErrorCode> {
        if !input.contains('<') && !input.contains('>') {
            return Ok(());
        }
        self.transaction
            .restore_strict_text(input)
            .map(|_| ())
            .map_err(|_| CodecErrorCode::RestoreFailedClosed)
    }
}

struct SyntheticResidualValidator;

impl ResponseResidualValidator for SyntheticResidualValidator {
    fn validate(
        &mut self,
        value: &str,
        authorized_output_ranges: &[std::ops::Range<usize>],
    ) -> Result<(), CodecErrorCode> {
        for (start, _) in value.match_indices(EMAIL) {
            let matched = start..start + EMAIL.len();
            if !authorized_output_ranges.iter().any(|authorized| {
                authorized.start <= matched.start && authorized.end >= matched.end
            }) {
                return Err(CodecErrorCode::ProviderOriginPii);
            }
        }
        Ok(())
    }
}

struct AllowAllResidualValidator;

impl ResponseResidualValidator for AllowAllResidualValidator {
    fn validate(
        &mut self,
        _value: &str,
        _authorized_output_ranges: &[std::ops::Range<usize>],
    ) -> Result<(), CodecErrorCode> {
        Ok(())
    }
}

struct ExpandingPseudonymizer;

impl RequestPseudonymizer for ExpandingPseudonymizer {
    fn protect(&mut self, input: &str) -> Result<String, CodecErrorCode> {
        if input == "EXPAND" {
            Ok("synthetic".repeat(512))
        } else {
            Ok(input.to_owned())
        }
    }

    fn validate_token_shapes(&mut self, _input: &str) -> Result<(), CodecErrorCode> {
        Ok(())
    }
}

fn request_context<'a>(
    pseudonymizer: &'a mut dyn RequestPseudonymizer,
) -> RequestTransformContext<'a> {
    RequestTransformContext::new(pseudonymizer, WireFormat::Json, CodecLimits::default())
}

#[test]
fn request_rejects_duplicates_and_preserves_raw_numbers_while_protecting_tool_json() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let mut pseudonymizer = SyntheticPseudonymizer {
        transaction: &mut transaction,
    };
    let duplicate = br#"{"model":"claude-test","max_tokens":32,"messages":[],"metadata":{"email":"a","\u0065mail":"b"}}"#;
    let error = codec
        .protect_request(duplicate, &mut request_context(&mut pseudonymizer))
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::DuplicateObjectKey);

    let normalization_ambiguous =
        r#"{"model":"claude-test","max_tokens":32,"messages":[],"metadata":{"café":"synthetic"}}"#;
    let error = codec
        .protect_request(
            normalization_ambiguous.as_bytes(),
            &mut request_context(&mut pseudonymizer),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::InvalidJson);

    let input = br#"{"model":"claude-test","max_tokens":32,"temperature":1.2300e-2,"messages":[{"role":"user","content":[{"type":"tool_use","id":"toolu_1","name":"lookup","input":{"email":"alice@example.invalid","active":true}}]}]}"#;
    let proved = codec
        .protect_request(input, &mut request_context(&mut pseudonymizer))
        .unwrap();
    let output = std::str::from_utf8(proved.bytes()).unwrap();
    assert!(output.contains("1.2300e-2"));
    assert!(!output.contains(EMAIL));
    assert!(output.contains("<"));
    assert!(proved.provenance().occurrence_count() > 0);
    assert!(proved.provenance().final_buffer_verified());
    assert_eq!(proved.format(), WireFormat::Json);

    let schema = br#"{"model":"claude-test","max_tokens":32,"messages":[],"tools":[{"name":"lookup","description":"Find alice@example.invalid","input_schema":{"type":"object","properties":{"alice@example.invalid":{"type":"string","description":"alice@example.invalid"}},"required":["alice@example.invalid"]}}]}"#;
    let proved = codec
        .protect_request(schema, &mut request_context(&mut pseudonymizer))
        .unwrap();
    let output = std::str::from_utf8(proved.bytes()).unwrap();
    assert!(!output.contains(EMAIL));
    let restored = pseudonymizer
        .transaction
        .restore_strict_text(output)
        .expect("all emitted schema tokens belong to the staged transaction");
    assert!(restored.matches(EMAIL).count() >= 4);
}

#[test]
fn request_signed_and_opaque_surfaces_are_explicit_and_fail_closed() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let mut pseudonymizer = SyntheticPseudonymizer {
        transaction: &mut transaction,
    };
    let signed_pii = br#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"alice@example.invalid","signature":"opaque-signature"}]}]}"#;
    let error = codec
        .protect_request(signed_pii, &mut request_context(&mut pseudonymizer))
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::SignedMutationRequired);

    let opaque_media = br#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"YWxpY2VAZXhhbXBsZS5pbnZhbGlk"}}]}]}"#;
    let error = codec
        .protect_request(opaque_media, &mut request_context(&mut pseudonymizer))
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::OpaqueMediaUninspected);

    let safe_signed = br#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"synthetic reasoning","signature":"opaque-signature"}]}]}"#;
    let proved = codec
        .protect_request(safe_signed, &mut request_context(&mut pseudonymizer))
        .unwrap();
    assert!(std::str::from_utf8(proved.bytes()).unwrap().contains(
        r#"{"type":"thinking","thinking":"synthetic reasoning","signature":"opaque-signature"}"#
    ));
    assert_eq!(proved.provenance().signed_opaque_ranges().len(), 1);
}

#[test]
fn non_stream_response_restores_exact_bytes_and_maps_output_provenance() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let token = transaction.tokenize(&PiiClass::Email, EMAIL).unwrap();
    let snapshot = transaction.commit().unwrap();
    let input = format!(
        r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{{"type":"text","text":"owner={token}"}}],"stop_reason":"end_turn","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":2}}}}"#
    );
    let mut residual = SyntheticResidualValidator;
    let mut context = ResponseTransformContext::new(
        &snapshot,
        &mut residual,
        WireFormat::Json,
        CodecLimits::default(),
    );
    let proved = codec
        .restore_response(input.as_bytes(), &mut context)
        .unwrap();
    let output = std::str::from_utf8(proved.bytes()).unwrap();
    assert!(output.contains(r#"owner=alice@example.invalid"#));
    assert!(!proved.provenance().authorized_output_ranges().is_empty());
    assert!(proved.provenance().final_buffer_verified());
    for range in proved.provenance().authorized_output_ranges() {
        assert!(range.end <= proved.bytes().len());
        assert_eq!(&proved.bytes()[range.clone()], EMAIL.as_bytes());
    }
}

#[test]
fn response_rejects_provider_numbers_collisions_and_raw_signed_pii() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let key_token = transaction.tokenize(&PiiClass::Email, EMAIL).unwrap();
    let snapshot = transaction.commit().unwrap();
    let mut residual = SyntheticResidualValidator;

    let numeric = br#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"tool_use","id":"toolu_1","name":"lookup","input":{"account":12345}}],"stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":2}}"#;
    let error = codec
        .restore_response(
            numeric,
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::ProviderOriginNumeric);

    let collision = format!(
        r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{{"type":"tool_use","id":"toolu_1","name":"lookup","input":{{"{key_token}":"a","alice@example.invalid":"b"}}}}],"stop_reason":"tool_use","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":2}}}}"#
    );
    let mut allow_all = AllowAllResidualValidator;
    let error = codec
        .restore_response(
            collision.as_bytes(),
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut allow_all,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::TransformedKeyCollision);

    let signed_raw = br#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"thinking","thinking":"alice@example.invalid","signature":"opaque-signature"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":2}}"#;
    let error = codec
        .restore_response(
            signed_raw,
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::ProviderOriginPii);

    let adjacent = br#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"text","text":"alice@"},{"type":"text","text":"example.invalid"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":2}}"#;
    let error = codec
        .restore_response(
            adjacent,
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::ProviderOriginPii);
}

#[test]
fn signed_response_bytes_and_tokens_are_preserved_and_citations_restore() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let token = transaction.tokenize(&PiiClass::Email, EMAIL).unwrap();
    let snapshot = transaction.commit().unwrap();
    let mut residual = SyntheticResidualValidator;

    let signed = format!(
        r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{{"type":"thinking","thinking":"{token}","signature":"opaque-signature"}}],"stop_reason":"end_turn","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":2}}}}"#
    );
    let proved = codec
        .restore_response(
            signed.as_bytes(),
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap();
    assert_eq!(proved.bytes(), signed.as_bytes());
    assert!(!proved.provenance().signed_opaque_ranges().is_empty());
    assert!(proved.provenance().authorized_output_ranges().is_empty());

    let unknown_signature = r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"thinking","thinking":"","signature":"<Email_999>"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":2}}"#;
    let error = codec
        .restore_response(
            unknown_signature.as_bytes(),
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::RestoreFailedClosed);

    let malformed_signed = r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"thinking","thinking":"","signature":"opaque-signature","unknown":"synthetic"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":2}}"#;
    let error = codec
        .restore_response(
            malformed_signed.as_bytes(),
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::SignedSurfaceMalformed);

    let citation = format!(
        r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{{"type":"text","text":"","citations":[{{"type":"char_location","cited_text":"{token}","document_title":"Synthetic document","document_index":0,"start_char_index":1,"end_char_index":2}}]}}],"stop_reason":"end_turn","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":2}}}}"#
    );
    let proved = codec
        .restore_response(
            citation.as_bytes(),
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap();
    let output = std::str::from_utf8(proved.bytes()).unwrap();
    assert!(output.contains(EMAIL));
    assert_eq!(output.matches(r#""type":"char_location""#).count(), 1);
}

#[test]
fn angle_wrapped_response_carriers_fail_closed_in_text_and_tool_output() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let snapshot = session.begin_transaction().commit().unwrap();
    let encoded = format!("<{}>", "A".repeat(80));
    let carriers = [
        "<data:text/plain;base64,YWxpY2VAZXhhbXBsZS5pbnZhbGlk>".to_owned(),
        "<https://example.invalid/synthetic>".to_owned(),
        "<file:///synthetic/path>".to_owned(),
        "<%61%6c%69%63%65%40example.invalid>".to_owned(),
        encoded,
        format!(
            "{}data:text/plain,synthetic{}",
            "<".repeat(128),
            ">".repeat(128)
        ),
    ];
    for carrier in carriers {
        let bodies = [
            format!(
                r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{{"type":"text","text":"{carrier}"}}],"stop_reason":"end_turn","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":1}}}}"#
            ),
            format!(
                r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{{"type":"tool_use","id":"toolu_1","name":"lookup","input":{{"result":"{carrier}"}}}}],"stop_reason":"tool_use","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":1}}}}"#
            ),
        ];
        for body in bodies {
            let mut residual = SyntheticResidualValidator;
            let error = codec
                .restore_response(
                    body.as_bytes(),
                    &mut ResponseTransformContext::new(
                        &snapshot,
                        &mut residual,
                        WireFormat::Json,
                        CodecLimits::default(),
                    ),
                )
                .unwrap_err();
            assert_eq!(error.code(), CodecErrorCode::ProviderOriginPii);
        }
    }
}

#[test]
fn deeply_angle_wrapped_request_text_fails_with_a_closed_code() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let mut pseudonymizer = SyntheticPseudonymizer {
        transaction: &mut transaction,
    };
    let wrapped = format!("{}synthetic{}", "<".repeat(256), ">".repeat(256));
    let request =
        format!(r#"{{"model":"claude-test","max_tokens":32,"system":"{wrapped}","messages":[]}}"#);

    let error = codec
        .protect_request(request.as_bytes(), &mut request_context(&mut pseudonymizer))
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::OpaqueMediaUninspected);

    let boundary = format!("{}synthetic{}", "<".repeat(128), ">".repeat(128));
    let boundary_request =
        format!(r#"{{"model":"claude-test","max_tokens":32,"system":"{boundary}","messages":[]}}"#);
    assert!(codec
        .protect_request(
            boundary_request.as_bytes(),
            &mut request_context(&mut pseudonymizer),
        )
        .is_ok());
}

#[test]
fn deeply_angle_wrapped_response_text_fails_with_a_closed_code() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let snapshot = session.begin_transaction().commit().unwrap();
    let wrapped = format!("{}synthetic{}", "<".repeat(256), ">".repeat(256));
    let response = format!(
        r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{{"type":"text","text":"{wrapped}"}}],"stop_reason":"end_turn","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":1}}}}"#
    );
    let mut residual = SyntheticResidualValidator;

    let error = codec
        .restore_response(
            response.as_bytes(),
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::ProviderOriginPii);

    let boundary = format!("{}synthetic{}", "<".repeat(128), ">".repeat(128));
    let boundary_response = format!(
        r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{{"type":"text","text":"{boundary}"}}],"stop_reason":"end_turn","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":1}}}}"#
    );
    assert!(codec
        .restore_response(
            boundary_response.as_bytes(),
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .is_ok());
}

#[test]
fn escaped_authorized_ranges_point_to_exact_wire_bytes() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let token = transaction
        .tokenize(&PiiClass::Name, r#"Dr. "Schmidt""#)
        .unwrap();
    let snapshot = transaction.commit().unwrap();
    let body = format!(
        r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{{"type":"text","text":"{token}"}}],"stop_reason":"end_turn","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":1}}}}"#
    );
    let mut residual = SyntheticResidualValidator;
    let proved = codec
        .restore_response(
            body.as_bytes(),
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap();
    assert!(proved.provenance().final_buffer_verified());
    assert_eq!(proved.provenance().authorized_output_ranges().len(), 1);
    let range = proved.provenance().authorized_output_ranges()[0].clone();
    assert_eq!(&proved.bytes()[range], br#"Dr. \"Schmidt\""#);
}

#[test]
fn every_nonstream_format_retains_exact_final_buffer_proof() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let token = transaction.tokenize(&PiiClass::Email, EMAIL).unwrap();
    let snapshot = transaction.commit().unwrap();

    let mut text_residual = SyntheticResidualValidator;
    let text = codec
        .restore_response(
            token.as_bytes(),
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut text_residual,
                WireFormat::Utf8Text,
                CodecLimits::default(),
            ),
        )
        .unwrap();
    assert_eq!(text.bytes(), EMAIL.as_bytes());
    assert!(text.provenance().final_buffer_verified());
    assert_eq!(
        &text.bytes()[text.provenance().authorized_output_ranges()[0].clone()],
        EMAIL.as_bytes()
    );

    let ndjson_body = format!(
        r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{{"type":"text","text":"{token}"}}],"stop_reason":"end_turn","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":1}}}}"#
    );
    let mut ndjson_residual = SyntheticResidualValidator;
    let ndjson = codec
        .restore_response(
            ndjson_body.as_bytes(),
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut ndjson_residual,
                WireFormat::Ndjson,
                CodecLimits::default(),
            ),
        )
        .unwrap();
    assert!(ndjson.provenance().final_buffer_verified());
    let ndjson_range = ndjson.provenance().authorized_output_ranges()[0].clone();
    assert_eq!(&ndjson.bytes()[ndjson_range], EMAIL.as_bytes());

    let mut empty_residual = SyntheticResidualValidator;
    let empty = codec
        .restore_response(
            &[],
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut empty_residual,
                WireFormat::Empty,
                CodecLimits::default(),
            ),
        )
        .unwrap();
    assert!(empty.bytes().is_empty());
    assert!(empty.provenance().final_buffer_verified());
}

#[test]
fn request_and_response_discriminants_and_required_fields_are_closed() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    {
        let mut pseudonymizer = SyntheticPseudonymizer {
            transaction: &mut transaction,
        };
        let invalid_requests = [
            r#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"user","content":[{"type":"future"}]}]}"#,
            r#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"future","content":"synthetic"}]}"#,
            r#"{"model":"claude-test","messages":[{"role":"user","content":"synthetic"}]}"#,
            r#"{"max_tokens":32,"messages":[{"role":"user","content":"synthetic"}]}"#,
            r#"{"model":"claude-test","max_tokens":32}"#,
            r#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"user","content":"synthetic"}],"thinking":{"type":"enabled","budget_tokens":1024,"alice@example.invalid":true}}"#,
            r#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"user","content":"synthetic"}],"tool_choice":{"type":"auto","future":123}}"#,
            r#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"user","content":"synthetic"}],"cache_control":{"type":"ephemeral","future":true}}"#,
            r#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"user","content":"synthetic"}],"tool_choice":{"type":"auto","data:text/plain":true}}"#,
            r#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"user","content":"synthetic"}],"cache_control":{"type":"ephemeral","%61%6c%69%63%65":true}}"#,
            r#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"user","content":"synthetic"}],"service_tier":"future"}"#,
        ];
        for request in invalid_requests {
            assert!(codec
                .protect_request(request.as_bytes(), &mut request_context(&mut pseudonymizer),)
                .is_err());
        }
    }
    let snapshot = transaction.commit().unwrap();
    let invalid_responses = [
        r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":"end_turn","stop_sequence":null}"#,
        r#"{"id":"msg_1","type":"message","role":"user","model":"claude-test","content":[],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}"#,
        r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"future"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}"#,
        r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":"future","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}"#,
        r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"text"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}"#,
        r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"tool_use","id":"toolu_1","name":"lookup"}],"stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}"#,
    ];
    for response in invalid_responses {
        let mut residual = SyntheticResidualValidator;
        assert!(codec
            .restore_response(
                response.as_bytes(),
                &mut ResponseTransformContext::new(
                    &snapshot,
                    &mut residual,
                    WireFormat::Json,
                    CodecLimits::default(),
                ),
            )
            .is_err());
    }
}

#[test]
fn response_tool_result_array_is_one_adjacency_domain() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let snapshot = session.begin_transaction().commit().unwrap();
    let response = br#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":["alice@","example.invalid"],"is_error":false}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}"#;
    let mut residual = SyntheticResidualValidator;
    let error = codec
        .restore_response(
            response,
            &mut ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::ProviderOriginPii);
}

#[test]
fn tiny_json_limits_fail_with_closed_codes() {
    let codec = AnthropicMessagesCodec;
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let mut pseudonymizer = SyntheticPseudonymizer {
        transaction: &mut transaction,
    };
    let cases = [
        (
            CodecLimits::default().with_max_body_bytes(8),
            CodecErrorCode::BodyLimitExceeded,
        ),
        (
            CodecLimits::default().with_max_json_depth(1),
            CodecErrorCode::DepthLimitExceeded,
        ),
        (
            CodecLimits::default().with_max_json_nodes(2),
            CodecErrorCode::NodeLimitExceeded,
        ),
        (
            CodecLimits::default().with_max_string_bytes(4),
            CodecErrorCode::StringLimitExceeded,
        ),
        (
            CodecLimits::default().with_max_number_bytes(1),
            CodecErrorCode::NumberLimitExceeded,
        ),
    ];
    let request =
        br#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"user","content":"synthetic"}]}"#;
    for (limits, expected) in cases {
        let error = codec
            .protect_request(
                request,
                &mut RequestTransformContext::new(&mut pseudonymizer, WireFormat::Json, limits),
            )
            .unwrap_err();
        assert_eq!(error.code(), expected);
    }

    let mut expanding = ExpandingPseudonymizer;
    let growth = br#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"user","content":"EXPAND"}]}"#;
    let error = codec
        .protect_request(
            growth,
            &mut RequestTransformContext::new(
                &mut expanding,
                WireFormat::Json,
                CodecLimits::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(error.code(), CodecErrorCode::GrowthLimitExceeded);
}
