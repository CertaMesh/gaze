use gaze_proxy_dashboard::{SafeJsonShapeVm, SseTimelineVm};
use gaze_types::inspection::ProjectionOmissionReasonV1;

#[test]
fn omitted_reveal_projections_have_reason_but_no_value_container() {
    for reason in [
        ProjectionOmissionReasonV1::NotCapturedByPolicy,
        ProjectionOmissionReasonV1::LimitExceeded,
        ProjectionOmissionReasonV1::ProjectionFailedClosed,
        ProjectionOmissionReasonV1::UnknownFuture,
    ] {
        let shape = serde_json::to_value(SafeJsonShapeVm::omitted(reason)).unwrap();
        let timeline = serde_json::to_value(SseTimelineVm::omitted(reason)).unwrap();
        for value in [shape, timeline] {
            assert_eq!(value["state"], "Omitted");
            assert!(value.get("reason").is_some());
            assert!(value.get("value").is_none());
            assert!(value.get("entries").is_none());
        }
    }
}
