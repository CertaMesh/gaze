use gaze_inspection::{ActivatedInspectionConsumerV1, InspectionPurgeGuardV1};

fn cannot_retarget(
    guard: InspectionPurgeGuardV1,
    other: &mut ActivatedInspectionConsumerV1,
) {
    guard.complete(other);
}

fn main() {}
