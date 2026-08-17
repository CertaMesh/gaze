// drift-ack: bundle-tokenization-drift baseline snapshots introduced for S1.
// drift-ack: v0.4.6 release aggregation refreshes bundle snapshots for rulepack version bump only.
// drift-ack: v0.5.1 rulepack_version sync refreshes bundle snapshots for rulepack version bump only.
// drift-ack: Spike 3 mandatory-anchor fallback refreshes core-extended no-policy snapshot.
// drift-ack: mixed locale-basis metadata bumps core to 0.5.2; detections remain unchanged.

#[test]
fn drift_ack_marker_is_source_controlled() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshot_ack.rs"),
    )
    .expect("snapshot_ack.rs is readable");
    assert!(source.contains("drift-ack:"));
}
