//! Shared detection view and raw-byte provenance for recognition and guess exclusion.

pub use gaze_types::normalize::{raw_range, NormalizedText};

/// Fold the shared detection view, retaining raw-byte provenance.
pub fn normalize(input: &str) -> NormalizedText {
    #[cfg(test)]
    crate::record_guess::WORK.with(|count| {
        let (normalize, fold) = count.get();
        count.set((normalize + 1, fold));
    });
    gaze_types::normalize::normalize(input)
}
