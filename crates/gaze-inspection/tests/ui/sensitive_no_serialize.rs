use gaze_inspection::OwnerRestoredPayloadV1;

fn main() {
    let payload = OwnerRestoredPayloadV1::capture(b"alice@example.invalid".to_vec());
    let _ = serde_json::to_string(&payload);
}
