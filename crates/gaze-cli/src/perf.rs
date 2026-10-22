//! Private diagnostic protocol; never changes the CLI's stdout contract.
use gaze_types::perf::Capture;

pub(crate) struct Document(Option<Capture>);
impl Document {
    pub(crate) fn from_env() -> Self {
        Self(
            (std::env::var_os("GAZE_PERF").as_deref() == Some(std::ffi::OsStr::new("1")))
                .then(Capture::new),
        )
    }
}
impl Drop for Document {
    fn drop(&mut self) {
        if let Some(capture) = self.0.take() {
            // Fixed-schema metadata only. A broken diagnostic sink must not alter protection.
            if let Ok(json) = serde_json::to_string(&capture.finish()) {
                use std::io::Write;
                let _ = writeln!(std::io::stderr().lock(), "GAZE_PERF {json}");
            }
        }
    }
}
