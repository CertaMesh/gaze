use gaze_inspection::ProviderVisiblePayloadV1;

fn main() {
    let payload = ProviderVisiblePayloadV1::capture(b"<Email_1>".to_vec());
    let _ = payload.clone();
}
