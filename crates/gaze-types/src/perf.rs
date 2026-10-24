//! Opt-in, thread-local attribution. Default builds erase the instrumentation macros.
//! Records contain fixed metric keys and process-local recognizer ordinals, never input values.

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
        $crate::perf::RecognizerSpan::new($id, $evidence)
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

    #[derive(Debug, Serialize)]
    pub struct RecognizerMetric {
        #[serde(flatten)]
        pub metric: Metric,
        pub evidence: &'static str,
        pub model: &'static str,
    }

    #[derive(Debug, Default, Serialize)]
    pub struct Record {
        pub schema: u32,
        pub phase: &'static str,
        pub ordinal: u64,
        #[serde(skip)]
        view: Option<&'static str>,
        pub metrics: BTreeMap<&'static str, Metric>,
        pub counters: BTreeMap<&'static str, u64>,
        /// Opaque encounter-order ordinals; declared IDs never leave process memory.
        pub recognizers: BTreeMap<String, RecognizerMetric>,
    }

    thread_local! {
        static IDENTIFIERS: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
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
            Self::with_identity("request", 0)
        }
        pub fn with_identity(phase: &'static str, ordinal: u64) -> Self {
            let previous = ACTIVE.with(|slot| {
                slot.replace(Some(Record {
                    schema: 3,
                    phase,
                    ordinal,
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
                "process_to_ready_ns",
                "process_to_policy_ns",
                "policy_load_ns",
                "pipeline_build_ns",
                "assembly_ns",
                "rule_construction_ns",
                "recognizer_construction_ns",
                "nym_attach_ns",
                "model_constructor_ns",
                "nym_model_constructor_ns",
                "nym_outer_verify_ns",
                "tokenizer_load_ns",
                "model_load_ns",
                "bundle_verify_ns",
                "daemon_startup_ns",
                "request_ns",
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
        evidence: crate::EvidenceKind,
        model: &'static str,
    }
    impl RecognizerSpan {
        pub fn new(id: &str, evidence: crate::EvidenceKind) -> Self {
            let model = match (evidence, id) {
                (crate::EvidenceKind::Rule, _) => "none",
                (_, crate::NER_RECOGNIZER_ID) => "ner",
                (_, "dob.gliner") => "dob",
                _ => "other",
            };
            let active = ACTIVE.with(|slot| slot.borrow().is_some());
            let id = active.then(|| {
                IDENTIFIERS.with(|ids| {
                    let mut ids = ids.borrow_mut();
                    let ordinal = format!("r{}", ids.len() + 1);
                    ids.entry(id.to_owned()).or_insert(ordinal).clone()
                })
            });
            Self {
                id,
                start: active.then(Instant::now),
                candidates: 0,
                evidence,
                model,
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
                        let entry =
                            record
                                .recognizers
                                .entry(id)
                                .or_insert_with(|| RecognizerMetric {
                                    metric: Metric::default(),
                                    evidence: match self.evidence {
                                        crate::EvidenceKind::Rule => "rule",
                                        crate::EvidenceKind::Learned => "learned",
                                    },
                                    model: self.model,
                                });
                        if entry.evidence
                            != match self.evidence {
                                crate::EvidenceKind::Rule => "rule",
                                crate::EvidenceKind::Learned => "learned",
                            }
                            || entry.model != self.model
                        {
                            entry.model = "other";
                        }
                        let metric = &mut entry.metric;
                        metric.ns += ns;
                        metric.invocations += 1;
                        metric.candidates += self.candidates;
                        if self.evidence == crate::EvidenceKind::Rule {
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
        fn model_roles_are_value_free_and_unknown_learned_roles_stay_visible() {
            let capture = Capture::new();
            for id in [
                crate::NER_RECOGNIZER_ID,
                "dob.gliner",
                "private learned recognizer",
            ] {
                let _span = RecognizerSpan::new(id, crate::EvidenceKind::Learned);
            }
            let record = capture.finish();
            assert_eq!(record.schema, 3);
            let models: std::collections::BTreeSet<_> = record
                .recognizers
                .values()
                .map(|entry| {
                    assert_eq!(entry.evidence, "learned");
                    entry.model
                })
                .collect();
            assert_eq!(models, ["dob", "ner", "other"].into_iter().collect());
            assert_eq!(record.metrics["rules_ns"].invocations, 0);
        }

        #[test]
        fn captures_are_isolated_and_identifiers_never_escape() {
            let outer = Capture::new();
            count("input_bytes", 3);
            let inner = Capture::new();
            {
                let _timer = Span::new("registry_ns");
                let mut rule =
                    RecognizerSpan::new("private recognizer id", crate::EvidenceKind::Rule);
                rule.candidates(2);
            }
            let record = inner.finish();
            assert_eq!(record.metrics["rules_ns"].invocations, 1);
            let (key, rule) = record.recognizers.first_key_value().unwrap();
            assert!(key.starts_with('r'));
            assert!(key[1..].parse::<u64>().is_ok());
            assert!(!key.contains("private"));
            assert_eq!(rule.metric.candidates, 2);
            assert_eq!(rule.evidence, "rule");
            assert_eq!(rule.model, "none");
            assert_eq!(rule.metric.ns, record.metrics["rules_ns"].ns);
            assert_eq!(outer.finish().counters["input_bytes"], 3);
        }
    }
}

#[cfg(all(test, not(feature = "perf-timers")))]
mod disabled_tests {
    #[test]
    fn disabled_macros_erase_arguments() {
        let _span = crate::perf_span!(panic!("disabled span evaluated"));
        let _recognizer = crate::perf_recognizer!(
            panic!("disabled id evaluated"),
            panic!("disabled evidence evaluated")
        );
        crate::perf_count!(
            panic!("disabled key evaluated"),
            panic!("disabled count evaluated")
        );
    }
}
