use static_assertions::assert_not_impl_any;

assert_not_impl_any!(gaze_proxy_dashboard::PairedDashboard: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::PendingDashboardActivation: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::DashboardLaunch: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::SpawnedDashboardChild: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::ChildInheritedHandles: Clone, std::fmt::Debug);

#[test]
fn activation_and_child_ownership_are_one_shot_by_type() {
    assert!(std::mem::size_of::<gaze_proxy_dashboard::PendingDashboardActivation>() > 0);
}
