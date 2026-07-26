use gaze_proxy_dashboard::{CanonicalAuthorizationV1, DeliveredAckV1};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(gaze_proxy_dashboard::PairingSecret: Clone, std::fmt::Debug, std::fmt::Display, serde::Serialize, AsRef<[u8]>);
assert_not_impl_any!(gaze_proxy_dashboard::PageSessionSecretV1: Clone, std::fmt::Debug, std::fmt::Display, serde::Serialize, AsRef<[u8]>);
assert_not_impl_any!(gaze_proxy_dashboard::CsrfSecretV1: Clone, std::fmt::Debug, std::fmt::Display, serde::Serialize, AsRef<[u8]>);

#[test]
fn canonical_authorization_has_one_exact_spelling() {
    let canonical = b"GazeDashboardV1 AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    assert!(CanonicalAuthorizationV1::parse(canonical).is_ok());
    for rejected in [
        b"gazeDashboardV1 AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".as_slice(),
        b"GazeDashboardV1  AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".as_slice(),
        b"GazeDashboardV1 AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".as_slice(),
        b"GazeDashboardV1 AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA+".as_slice(),
    ] {
        assert!(CanonicalAuthorizationV1::parse(rejected).is_err());
    }
}

#[test]
fn acknowledgement_is_exactly_nonce_bound() {
    let nonce = [3_u8; 16];
    let _ack = DeliveredAckV1::delivered(nonce);
    let mut raw = [0_u8; 22];
    raw[0..4].copy_from_slice(b"GZDB");
    raw[4] = 1;
    raw[5..21].copy_from_slice(&nonce);
    raw[21] = 1;
    assert!(DeliveredAckV1::decode(raw, nonce).is_ok());
    raw[21] = 0;
    assert!(DeliveredAckV1::decode(raw, nonce).is_err());
}
