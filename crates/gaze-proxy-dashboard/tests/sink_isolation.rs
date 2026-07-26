use static_assertions::assert_not_impl_any;

assert_not_impl_any!(gaze_proxy_dashboard::DashboardInspectionSink: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::PendingDashboardActivation: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::SpawnedDashboardChild: Clone, std::fmt::Debug);

#[test]
fn activation_capability_and_child_custody_are_linear_types() {
    assert!(std::mem::size_of::<gaze_proxy_dashboard::PendingDashboardActivation>() > 0);
    assert!(std::mem::size_of::<gaze_proxy_dashboard::SpawnedDashboardChild>() > 0);
}
