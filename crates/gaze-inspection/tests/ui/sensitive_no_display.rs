use gaze_inspection::OwnerRawPayloadV1;

fn main() {
    let payload = OwnerRawPayloadV1::capture(b"alice@example.invalid".to_vec());
    let _ = format!("{payload}");
}
