use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use gaze_types::nym::{NymLabel, NymOperatingPoint};

use super::decode::NUM_LABELS;

const MAX_BYTES: usize = 8 * 1024 * 1024;
type Rows = Arc<Vec<[f32; NUM_LABELS]>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum View {
    Stable,
    Neutral,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct WindowKey {
    model: u64,
    thresholds: Vec<(NymLabel, u32)>,
    view: View,
    bytes: String,
    ids: Vec<u32>,
}

impl WindowKey {
    pub(super) fn new(
        model: u64,
        operating_point: &NymOperatingPoint,
        view: View,
        bytes: String,
        ids: &[u32],
    ) -> Self {
        Self {
            model,
            thresholds: operating_point
                .iter()
                .map(|(k, v)| (k, v.to_bits()))
                .collect(),
            view,
            bytes,
            ids: ids.to_vec(),
        }
    }
}

pub(super) fn model_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .expect("Nym backend generation exhausted")
}

#[derive(Default)]
struct Cache {
    rows: HashMap<WindowKey, Rows>,
    bytes: usize,
    #[cfg(feature = "test-support")]
    verify: bool,
    #[cfg(feature = "test-support")]
    verified: usize,
}

thread_local! {
    static ACTIVE: RefCell<Option<Cache>> = const { RefCell::new(None) };
}

/// Synchronous inference scope. Nested pipelines get isolated caches; dropping the guard
/// releases this request's input bytes and restores the caller's scope, including on unwind.
/// The guard cannot move to another thread.
#[doc(hidden)]
pub struct InferenceRequest {
    previous: Option<Cache>,
    _thread: PhantomData<Rc<()>>,
}

impl InferenceRequest {
    pub(crate) fn enter() -> Self {
        Self {
            previous: ACTIVE.with(|slot| slot.replace(Some(Cache::default()))),
            _thread: PhantomData,
        }
    }

    pub(super) fn if_absent() -> Option<Self> {
        ACTIVE
            .with(|slot| slot.borrow().is_none())
            .then(Self::enter)
    }
}

#[cfg(feature = "test-support")]
impl InferenceRequest {
    pub(super) fn verifying() -> Self {
        let guard = Self::enter();
        ACTIVE.with(|slot| slot.borrow_mut().as_mut().unwrap().verify = true);
        guard
    }
}

#[cfg(feature = "test-support")]
pub(super) fn verification_enabled() -> bool {
    ACTIVE.with(|slot| slot.borrow().as_ref().is_some_and(|cache| cache.verify))
}

#[cfg(feature = "test-support")]
pub(super) fn verified_window() {
    ACTIVE.with(|slot| slot.borrow_mut().as_mut().unwrap().verified += 1);
}

#[cfg(feature = "test-support")]
pub(super) fn verified_windows() -> usize {
    ACTIVE.with(|slot| slot.borrow().as_ref().unwrap().verified)
}

impl Drop for InferenceRequest {
    fn drop(&mut self) {
        ACTIVE.with(|slot| slot.replace(self.previous.take()));
    }
}

pub(super) fn get(key: &WindowKey) -> Option<Rows> {
    ACTIVE.with(|slot| slot.borrow().as_ref()?.rows.get(key).cloned())
}

pub(super) fn insert(key: WindowKey, rows: Rows) {
    ACTIVE.with(|slot| {
        let mut active = slot.borrow_mut();
        let Some(cache) = active.as_mut() else { return };
        let bytes = key.bytes.len()
            + key.ids.len() * size_of::<u32>()
            + key.thresholds.len() * size_of::<(NymLabel, u32)>()
            + rows.len() * size_of::<[f32; NUM_LABELS]>()
            + size_of::<WindowKey>();
        if bytes <= MAX_BYTES.saturating_sub(cache.bytes) {
            cache.bytes += bytes;
            cache.rows.insert(key, rows);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> WindowKey {
        WindowKey::new(
            1,
            &NymOperatingPoint::op_b(),
            View::Stable,
            "synthetic".into(),
            &[1, 2],
        )
    }

    fn put(key: WindowKey) {
        insert(key, Arc::new(vec![[0.0; NUM_LABELS]]));
    }

    #[test]
    fn every_execution_input_separates_reuse() {
        let _scope = InferenceRequest::enter();
        let base = key();
        put(base.clone());
        assert!(get(&base).is_some());
        let op = NymOperatingPoint::op_b();
        let make = |model, op: &NymOperatingPoint, view, bytes: &str, ids: &[u32]| {
            WindowKey::new(model, op, view, bytes.into(), ids)
        };
        let model = make(2, &op, View::Stable, "synthetic", &[1, 2]);
        let changed_op = NymOperatingPoint::new(op.iter().map(|(label, _)| (label, 0.9))).unwrap();
        let threshold = make(1, &changed_op, View::Stable, "synthetic", &[1, 2]);
        let view = make(1, &op, View::Neutral, "synthetic", &[1, 2]);
        let bytes = make(1, &op, View::Stable, "synthetic ", &[1, 2]);
        let ids = make(1, &op, View::Stable, "synthetic", &[1, 2, 3]);
        for changed in [model, threshold, view, bytes, ids] {
            assert!(
                get(&changed).is_none(),
                "changed execution input reused a window"
            );
        }
    }

    #[test]
    fn requests_and_nested_pipelines_are_isolated() {
        {
            let _outer = InferenceRequest::enter();
            put(key());
            {
                let _inner = InferenceRequest::enter();
                assert!(get(&key()).is_none());
                put(key());
            }
            assert!(get(&key()).is_some());
        }
        assert!(get(&key()).is_none());
        let _next = InferenceRequest::enter();
        assert!(get(&key()).is_none());
    }

    #[test]
    fn unwind_drops_inputs_and_restores_nested_scope() {
        assert!(std::panic::catch_unwind(|| {
            let _scope = InferenceRequest::enter();
            put(key());
            panic!("synthetic unwind");
        })
        .is_err());
        assert!(get(&key()).is_none());
        let _outer = InferenceRequest::enter();
        put(key());
        let _ = std::panic::catch_unwind(|| {
            let _inner = InferenceRequest::enter();
            panic!("synthetic nested unwind");
        });
        assert!(get(&key()).is_some());
    }

    #[test]
    fn another_thread_never_reads_the_callers_cache() {
        let _scope = InferenceRequest::enter();
        put(key());
        std::thread::spawn(|| {
            assert!(get(&key()).is_none());
            let _scope = InferenceRequest::enter();
            assert!(get(&key()).is_none());
            put(key());
        })
        .join()
        .unwrap();
        assert!(get(&key()).is_some());
    }

    #[test]
    fn cache_budget_exhaustion_only_causes_misses() {
        let _scope = InferenceRequest::enter();
        let mut oversized = key();
        oversized.bytes = "x".repeat(MAX_BYTES);
        put(oversized.clone());
        assert!(get(&oversized).is_none());
        put(key());
        assert!(get(&key()).is_some());
    }
}
