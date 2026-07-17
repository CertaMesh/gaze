use gaze_inspection::InspectionEventV1;
use serde::Serialize;

fn require_debug<T: std::fmt::Debug>() {}
fn require_serialize<T: Serialize>() {}

fn main() {
    require_debug::<InspectionEventV1>();
    require_serialize::<InspectionEventV1>();
}
