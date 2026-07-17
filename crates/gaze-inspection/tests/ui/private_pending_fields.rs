use gaze_inspection::PendingInspectionProducerV1;
use gaze_types::inspection::{CaptureDomainsV1, DashboardCaptureDescriptorV1};

fn main() {
    let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::MetadataOnly);
    let _pending = PendingInspectionProducerV1 { descriptor };
}
