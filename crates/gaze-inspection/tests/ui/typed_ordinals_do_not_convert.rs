use gaze_types::inspection::{JsonNodeOrdinalV1, SseEntryOrdinalV1};

fn main() {
    let json = JsonNodeOrdinalV1::new(4);
    let _sse: SseEntryOrdinalV1 = json.into();
}
