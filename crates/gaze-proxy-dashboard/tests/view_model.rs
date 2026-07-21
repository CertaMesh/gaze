use gaze_proxy_dashboard::{
    AttestationTraceVm, DecisionTraceVm, ProjectionAvailabilityVm, SafeJsonShapeVm,
    SseTimelineEntryVm, SseTimelineVm,
};
use gaze_types::inspection::{
    AttestationTraceV1, DecisionEngineCodeV1, DecisionOutcomeCodeV1, ProjectionOmissionReasonV1,
    SseDeltaKindV1, SseEventKindV1,
};

#[test]
fn sse_contract_exposes_only_ordinal_kind_delta_and_content_block() {
    let vm = SseTimelineVm::present(vec![SseTimelineEntryVm::new(
        7,
        SseEventKindV1::ContentBlockDelta,
        Some(SseDeltaKindV1::InputJson),
        Some(1),
    )]);
    let json = serde_json::to_string(&vm).unwrap();
    for forbidden in ["byte", "timing", "timestamp", "latency", "cadence"] {
        assert!(!json.to_ascii_lowercase().contains(forbidden));
    }
}

#[test]
fn omissions_and_traces_use_exact_closed_caution_language() {
    assert_eq!(
        ProjectionAvailabilityVm::Omitted(ProjectionOmissionReasonV1::ProjectionFailedClosed)
            .label(),
        "PROJECTION FAILED CLOSED"
    );
    let omitted = serde_json::to_string(&SafeJsonShapeVm::omitted(
        ProjectionOmissionReasonV1::NotCapturedByPolicy,
    ))
    .unwrap();
    assert!(!omitted.contains("node_count"));
    assert!(!omitted.contains("value"));
    assert_eq!(
        omitted,
        r#"{"state":"Omitted","reason":"NotCapturedByPolicy"}"#
    );
    assert_eq!(
        DecisionTraceVm::new(
            DecisionEngineCodeV1::ResidualScan,
            DecisionOutcomeCodeV1::RanClean
        )
        .outcome_label(),
        "RAN — NO FINDINGS (not verified clean)"
    );
    assert_eq!(
        AttestationTraceVm::new(AttestationTraceV1::ExactHit).label(),
        "EXACT HIT"
    );
}
