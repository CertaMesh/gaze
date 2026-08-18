use static_assertions::assert_not_impl_any;

assert_not_impl_any!(gaze_proxy_dashboard::DashboardInspectionSink: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::PendingDashboardActivation: Clone, std::fmt::Debug);
assert_not_impl_any!(gaze_proxy_dashboard::SpawnedDashboardChild: Clone, std::fmt::Debug);
