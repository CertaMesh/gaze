use gaze_inspection::OwnerRestoredPayloadV1;

fn main() {
    let payload = OwnerRestoredPayloadV1::capture(b"alice@example.invalid".to_vec());
    let _escaped: &[u8] = payload.with_declassified_bytes(|bytes| bytes);
}
