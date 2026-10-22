//! Opt-in, thread-local attribution. Default builds erase the instrumentation macros.
//! Records contain fixed metric keys and hashed recognizer identities, never input values.

/// A zero-sized guard in builds without attribution support.
#[doc(hidden)]
pub struct Disabled;
impl Disabled {
    #[inline(always)]
    pub fn candidates(&mut self, _count: usize) {}
}
impl Drop for Disabled {
    #[inline(always)]
    fn drop(&mut self) {}
}

#[cfg(not(feature = "perf-timers"))]
#[macro_export]
macro_rules! perf_span {
    ($key:expr) => {
        $crate::perf::Disabled
    };
}
#[cfg(feature = "perf-timers")]
#[macro_export]
macro_rules! perf_span {
    ($key:expr) => {
        $crate::perf::Span::new($key)
    };
}
#[cfg(not(feature = "perf-timers"))]
#[macro_export]
macro_rules! perf_recognizer {
    ($id:expr, $evidence:expr) => {
        $crate::perf::Disabled
    };
}
#[cfg(feature = "perf-timers")]
#[macro_export]
macro_rules! perf_recognizer {
    ($id:expr, $evidence:expr) => {
        $crate::perf::RecognizerSpan::new($id, $evidence == $crate::EvidenceKind::Rule)
    };
}
#[cfg(not(feature = "perf-timers"))]
#[macro_export]
macro_rules! perf_count {
    ($key:expr, $count:expr) => {};
}
#[cfg(feature = "perf-timers")]
#[macro_export]
macro_rules! perf_count {
    ($key:expr, $count:expr) => {
        $crate::perf::count($key, $count as u64)
    };
}

#[cfg(feature = "perf-timers")]
pub use enabled::*;

#[cfg(feature = "perf-timers")]
mod enabled {
    use serde::Serialize;
    use sha3::{Digest, Sha3_256};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::marker::PhantomData;
    use std::rc::Rc;
    use std::time::Instant;

    #[derive(Debug, Default, Serialize)]
    pub struct Metric {
        pub ns: u64,
        pub invocations: u64,
        pub candidates: u64,
    }

    #[derive(Debug, Default, Serialize)]
    pub struct Record {
        pub schema: u32,
        #[serde(skip)]
        view: Option<&'static str>,
        pub metrics: BTreeMap<&'static str, Metric>,
        pub counters: BTreeMap<&'static str, u64>,
        /// SHA3-256 of the declared ID. Custom recognizer IDs may contain private data.
        pub recognizers: BTreeMap<String, Metric>,
    }

    thread_local! {
        static ACTIVE: RefCell<Option<Record>> = const { RefCell::new(None) };
    }

    /// One synchronous document. Cannot cross threads; nested captures restore their parent.
    pub struct Capture {
        previous: Option<Option<Record>>,
        _thread: PhantomData<Rc<()>>,
    }
    impl Default for Capture {
        fn default() -> Self {
            Self::new()
        }
    }
    impl Capture {
        pub fn new() -> Self {
            let previous = ACTIVE.with(|slot| {
                slot.replace(Some(Record {
                    schema: 1,
                    ..Record::default()
                }))
            });
            Self {
                previous: Some(previous),
                _thread: PhantomData,
            }
        }
        pub fn finish(mut self) -> Record {
            let previous = self.previous.take().expect("capture owns its parent");
            let record = ACTIVE.with(|slot| slot.replace(previous));
            let mut record = record.expect("capture owns an active record");
            for key in [
                "rules_ns",
                "registry_ns",
                "validators_ns",
                "ner_tokenize_ns",
                "ner_ort_ns",
                "ner_mutex_wait_ns",
                "resolve1_ns",
                "sweep_ns",
                "resolve2_ns",
                "safety_view_build_ns",
                "safety_model_ns",
                "nym_tokenize_ns",
                "nym_ort_ns",
                "nym_mutex_wait_ns",
                "nym_stable_ns",
                "nym_neutral_ns",
                "manifest_ns",
                "audit_ns",
            ] {
                record.metrics.entry(key).or_default();
            }
            for key in [
                "input_bytes",
                "ner_encodes",
                "ner_windows",
                "sweep_candidates",
                "nym_windows",
                "nym_windows_stable",
                "nym_windows_neutral",
            ] {
                record.counters.entry(key).or_default();
            }
            record
        }
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                ACTIVE.with(|slot| {
                    slot.replace(previous);
                });
            }
        }
    }

    pub struct Span {
        key: &'static str,
        start: Option<Instant>,
        previous_view: Option<Option<&'static str>>,
    }
    impl Span {
        pub fn new(key: &'static str) -> Self {
            let start = ACTIVE.with(|slot| slot.borrow().as_ref().map(|_| Instant::now()));
            let previous_view = ACTIVE.with(|slot| {
                let mut slot = slot.borrow_mut();
                let record = slot.as_mut()?;
                let view = match key {
                    "nym_stable_ns" => "nym_windows_stable",
                    "nym_neutral_ns" => "nym_windows_neutral",
                    _ => return None,
                };
                Some(record.view.replace(view))
            });
            Self {
                key,
                start,
                previous_view,
            }
        }
    }
    impl Drop for Span {
        fn drop(&mut self) {
            if let Some(start) = self.start {
                let elapsed = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
                ACTIVE.with(|slot| {
                    if let Some(record) = slot.borrow_mut().as_mut() {
                        if let Some(view) = self.previous_view {
                            record.view = view;
                        }
                        let metric = record.metrics.entry(self.key).or_default();
                        metric.ns += elapsed;
                        metric.invocations += 1;
                    }
                });
            }
        }
    }

    pub struct RecognizerSpan {
        id: Option<String>,
        start: Option<Instant>,
        candidates: u64,
        rule: bool,
    }
    impl RecognizerSpan {
        pub fn new(id: &str, rule: bool) -> Self {
            let active = ACTIVE.with(|slot| slot.borrow().is_some());
            let id = active.then(|| format!("{:x}", Sha3_256::digest(id.as_bytes())));
            Self {
                id,
                start: active.then(Instant::now),
                candidates: 0,
                rule,
            }
        }
        pub fn candidates(&mut self, count: usize) {
            self.candidates = count as u64;
        }
    }
    impl Drop for RecognizerSpan {
        fn drop(&mut self) {
            if let (Some(id), Some(start)) = (self.id.take(), self.start) {
                let ns = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
                ACTIVE.with(|slot| {
                    if let Some(record) = slot.borrow_mut().as_mut() {
                        let metric = record.recognizers.entry(id).or_default();
                        metric.ns += ns;
                        metric.invocations += 1;
                        metric.candidates += self.candidates;
                        if self.rule {
                            let rules = record.metrics.entry("rules_ns").or_default();
                            rules.ns += ns;
                            rules.invocations += 1;
                            rules.candidates += self.candidates;
                        }
                    }
                });
            }
        }
    }

    pub fn count(key: &'static str, count: u64) {
        ACTIVE.with(|slot| {
            if let Some(record) = slot.borrow_mut().as_mut() {
                *record.counters.entry(key).or_default() += count;
                if key == "nym_windows" {
                    if let Some(view) = record.view {
                        *record.counters.entry(view).or_default() += count;
                    }
                }
            }
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn captures_are_isolated_and_identifiers_never_escape() {
            let outer = Capture::new();
            count("input_bytes", 3);
            let inner = Capture::new();
            {
                let _timer = Span::new("registry_ns");
                let mut rule = RecognizerSpan::new("private recognizer id", true);
                rule.candidates(2);
            }
            let record = inner.finish();
            assert_eq!(record.metrics["rules_ns"].invocations, 1);
            let (key, rule) = record.recognizers.first_key_value().unwrap();
            assert_eq!(key.len(), 64);
            assert!(!key.contains("private"));
            assert_eq!(rule.candidates, 2);
            assert_eq!(outer.finish().counters["input_bytes"], 3);
        }
    }
}
