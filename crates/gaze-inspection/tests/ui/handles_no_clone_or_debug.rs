use gaze_inspection::{ActivatedInspectionConsumerV1, InstalledInspectionProducerV1};

fn require_clone<T: Clone>() {}
fn require_debug<T: std::fmt::Debug>() {}

fn main() {
    require_clone::<InstalledInspectionProducerV1>();
    require_debug::<InstalledInspectionProducerV1>();
    require_clone::<ActivatedInspectionConsumerV1>();
    require_debug::<ActivatedInspectionConsumerV1>();
}
