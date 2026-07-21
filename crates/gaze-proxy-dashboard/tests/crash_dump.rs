use gaze_proxy_dashboard::{NoDumpReadiness, NoDumpReadiness::Verified};

#[test]
fn unix_child_readiness_sets_and_verifies_zero_core_limit() {
    assert_eq!(NoDumpReadiness::install_and_verify(), Verified);
    #[cfg(unix)]
    {
        use rustix::process::{getrlimit, Resource};
        let limit = getrlimit(Resource::Core);
        assert_eq!(limit.current, Some(0));
        assert_eq!(limit.maximum, Some(0));
    }
}
