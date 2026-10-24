//! Private diagnostic protocol; never changes the CLI's stdout contract.
use gaze_types::perf::{Capture, Span};
use std::cell::{Cell, RefCell};

thread_local! {
    static STARTUP: RefCell<Option<(Capture, Span)>> = const { RefCell::new(None) };
    static POLICY: RefCell<Option<Span>> = const { RefCell::new(None) };
    static ORDINAL: Cell<u64> = const { Cell::new(0) };
}

fn enabled() -> bool {
    std::env::var_os("GAZE_PERF").as_deref() == Some(std::ffi::OsStr::new("1"))
}

/// First operation in main. The clock excludes OS loader time before main.
pub(crate) fn start() {
    if enabled() {
        let capture = Capture::with_identity("startup", 0);
        let span = Span::new("process_to_ready_ns");
        POLICY.with(|slot| slot.replace(Some(Span::new("process_to_policy_ns"))));
        STARTUP.with(|slot| slot.replace(Some((capture, span))));
    }
}

pub(crate) fn policy_loaded() {
    POLICY.with(|slot| slot.take());
}

/// Ends startup before reading daemon requests (or before one-shot processing).
pub(crate) fn ready() {
    if let Some((capture, span)) = STARTUP.with(|slot| slot.take()) {
        drop(span);
        emit(capture);
    }
}

fn emit(capture: Capture) {
    // A broken diagnostic sink must not alter protection.
    if let Ok(json) = serde_json::to_string(&capture.finish()) {
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "GAZE_PERF {json}");
    }
}

pub(crate) struct Document(Option<(Capture, Span)>);
impl Document {
    pub(crate) fn from_env() -> Self {
        ready();
        Self(enabled().then(|| {
            let ordinal = ORDINAL.with(|slot| {
                let next = slot.get() + 1;
                slot.set(next);
                next
            });
            let capture = Capture::with_identity("request", ordinal);
            (capture, Span::new("request_ns"))
        }))
    }
}
impl Drop for Document {
    fn drop(&mut self) {
        if let Some((capture, span)) = self.0.take() {
            drop(span);
            emit(capture);
        }
    }
}
