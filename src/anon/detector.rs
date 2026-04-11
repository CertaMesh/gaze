//! The second layer of defense: a content-based PII detector. Column-rule
//! replacement in `Replacer` is layer 1; this is layer 2, run against any
//! value flowing out of an adapter that wasn't already mapped by a column
//! rule. Concrete impls live in submodules (Worka for v0.1).

use crate::policy::classifier::PiiClass;

#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub class: PiiClass,
    /// Byte range within the input string that matched.
    pub start: usize,
    pub end: usize,
}

pub trait PiiDetector: Send + Sync {
    /// Scan `text` for any PII. Returns zero or more non-overlapping
    /// detections, sorted by `start`.
    fn detect(&self, text: &str) -> Vec<Detection>;
}

/// No-op detector. Used in unit tests where the detector shouldn't fire.
pub struct NoopDetector;

impl PiiDetector for NoopDetector {
    fn detect(&self, _text: &str) -> Vec<Detection> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_detector_finds_nothing() {
        let d = NoopDetector;
        assert!(d.detect("krishan@example.com contacted us").is_empty());
    }
}
