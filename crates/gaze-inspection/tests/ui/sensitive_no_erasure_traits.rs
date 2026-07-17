use gaze_inspection::ProviderVisiblePayloadV1;
use std::hash::Hash;

fn require_as_ref<T: AsRef<[u8]>>(_: &T) {}
fn require_eq<T: Eq>(_: &T) {}
fn require_hash<T: Hash>(_: &T) {}

fn main() {
    let payload = ProviderVisiblePayloadV1::capture(b"<Email_1>".to_vec());
    require_as_ref(&payload);
    require_eq(&payload);
    require_hash(&payload);
    let _bytes: Vec<u8> = payload.into();
}
