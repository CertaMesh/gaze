use std::time::Duration;

use gaze_proxy_dashboard::{IpcLimits, RetentionLimits};

#[test]
fn retention_and_frame_limits_reject_zero_and_one_over_the_documented_caps() {
    assert!(RetentionLimits::new(64, 4 * 1024 * 1024, Duration::from_secs(300)).is_ok());
    assert!(RetentionLimits::new(0, 1, Duration::from_secs(1)).is_err());
    assert!(RetentionLimits::new(1_025, 4 * 1024 * 1024, Duration::from_secs(300)).is_err());
    assert!(RetentionLimits::new(64, 64 * 1024 * 1024 + 1, Duration::from_secs(300)).is_err());
    assert!(IpcLimits::new(64, 4 * 1024 * 1024).is_ok());
    assert!(IpcLimits::new(65, 4 * 1024 * 1024 + 1).is_err());
}
