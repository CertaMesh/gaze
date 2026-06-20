use std::marker::PhantomData;

struct RestoreState {
    _marker: PhantomData<gaze_audit::AuditLogRow>,
}

fn main() {
    let _ = RestoreState {
        _marker: PhantomData,
    };
}
