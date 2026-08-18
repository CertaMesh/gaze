use static_assertions::assert_not_impl_any;

assert_not_impl_any!(gaze_proxy_dashboard::PairedDashboard: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::PendingDashboardActivation: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::DashboardLaunch: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::SpawnedDashboardChild: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::ChildInheritedHandles: Clone, std::fmt::Debug);
