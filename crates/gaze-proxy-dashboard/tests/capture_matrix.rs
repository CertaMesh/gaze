use gaze_proxy_dashboard::{
    DashboardPayloadAcceptance, DashboardStartupConfig, OwnerRawRiskAcknowledgement,
    OwnerRestoredRiskAcknowledgement,
};
use gaze_types::inspection::InspectionDomainV1;

#[test]
fn dashboard_is_default_off_and_provider_visible_is_the_only_on_baseline() {
    assert!(matches!(
        DashboardStartupConfig::default(),
        DashboardStartupConfig::Off
    ));
    let baseline = DashboardPayloadAcceptance::provider_visible();
    assert!(baseline
        .descriptor()
        .captures(InspectionDomainV1::ProviderVisible));
    assert!(!baseline.descriptor().captures(InspectionDomainV1::OwnerRaw));
    assert!(!baseline
        .descriptor()
        .captures(InspectionDomainV1::OwnerRestored));
}

#[test]
fn owner_domains_require_distinct_acknowledgement_types() {
    let raw = DashboardPayloadAcceptance::provider_visible()
        .with_owner_raw(OwnerRawRiskAcknowledgement::acknowledge_pii_risk());
    assert!(raw.descriptor().captures(InspectionDomainV1::OwnerRaw));
    assert!(!raw.descriptor().captures(InspectionDomainV1::OwnerRestored));

    let restored = DashboardPayloadAcceptance::provider_visible()
        .with_owner_restored(OwnerRestoredRiskAcknowledgement::acknowledge_reidentification_risk());
    assert!(!restored.descriptor().captures(InspectionDomainV1::OwnerRaw));
    assert!(restored
        .descriptor()
        .captures(InspectionDomainV1::OwnerRestored));
}
