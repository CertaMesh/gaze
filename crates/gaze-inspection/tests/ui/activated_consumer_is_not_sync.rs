use gaze_inspection::ActivatedInspectionConsumerV1;

fn require_sync<T: Sync>() {}

fn main() {
    require_sync::<ActivatedInspectionConsumerV1>();
}
