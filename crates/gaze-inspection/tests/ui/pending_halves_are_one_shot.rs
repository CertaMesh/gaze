use gaze_inspection::{
    install_inspection_v1, InspectionQueueLimitsV1, NoopInspectionSinkV1,
    PendingInspectionConsumerV1, PendingInspectionProducerV1,
};
use gaze_types::inspection::{CaptureDomainsV1, DashboardCaptureDescriptorV1};
use std::sync::Arc;

fn main() {
    let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::MetadataOnly);
    let producer = PendingInspectionProducerV1::new(descriptor);
    let consumer = PendingInspectionConsumerV1::new(
        descriptor,
        Arc::new(NoopInspectionSinkV1),
        InspectionQueueLimitsV1::new(1, 1).unwrap(),
    );
    let _installed = install_inspection_v1(producer, consumer);
    let _reuse = producer;
}
