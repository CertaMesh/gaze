use std::sync::Arc;

use gaze_inspection::{
    install_inspection_v1, InspectionQueueLimitsV1, InspectionSink, InspectionSinkErrorV1,
    PendingInspectionConsumerV1, PendingInspectionProducerV1,
};
use gaze_types::inspection::{CaptureDomainsV1, DashboardCaptureDescriptorV1};

struct ClosedSink;

impl InspectionSink for ClosedSink {
    fn try_emit(
        &self,
        _event: gaze_inspection::InspectionEventV1,
    ) -> Result<(), InspectionSinkErrorV1> {
        Err(InspectionSinkErrorV1::Closed)
    }
}

#[test]
fn registration_purge_guard_closes_then_reopens_only_at_its_returned_epoch() {
    let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::MetadataOnly);
    let pending_producer = PendingInspectionProducerV1::new(descriptor);
    let pending_consumer = PendingInspectionConsumerV1::new(
        descriptor,
        Arc::new(ClosedSink),
        InspectionQueueLimitsV1::new(4, 4_096).unwrap(),
    );
    let (producer, mut activated) =
        install_inspection_v1(pending_producer, pending_consumer).unwrap();
    let guard = activated.begin_purge().unwrap();
    assert!(producer.begin_logical().is_err());
    let expected = guard.next_epoch();
    let completed = guard.complete().unwrap();
    assert_eq!(completed, expected);
    assert!(producer.begin_logical().is_ok());
}
