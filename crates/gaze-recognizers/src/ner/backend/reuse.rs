//! NER logits reuse across the pipeline calls of one spanning inference request.
//!
//! Outside a spanning scope nothing is cached. A key carries every model input, so a hit
//! returns the logits the same session computed for identical inputs earlier in the request.

use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const MAX_BYTES: usize = 8 * 1024 * 1024;

pub(super) type Logits = Arc<(Vec<i64>, Vec<f32>)>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct WindowKey {
    model: u64,
    ids: Vec<u32>,
    attention: Vec<u32>,
}

impl WindowKey {
    pub(super) fn new(model: u64, ids: &[u32], attention: &[u32]) -> Self {
        Self {
            model,
            ids: ids.to_vec(),
            attention: attention.to_vec(),
        }
    }
}

pub(super) fn model_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .expect("NER backend generation exhausted")
}

#[derive(Default)]
struct Cache {
    logits: HashMap<WindowKey, Logits>,
    bytes: usize,
}

thread_local! {
    static ACTIVE: RefCell<Option<Cache>> = const { RefCell::new(None) };
}

/// Restores the caller's scope on drop, including on unwind. Cannot move to another thread.
pub(crate) struct Spanning {
    previous: Option<Cache>,
    _thread: PhantomData<Rc<()>>,
}

impl Spanning {
    pub(crate) fn enter() -> Self {
        Self {
            previous: ACTIVE.with(|slot| slot.replace(Some(Cache::default()))),
            _thread: PhantomData,
        }
    }
}

impl Drop for Spanning {
    fn drop(&mut self) {
        ACTIVE.with(|slot| slot.replace(self.previous.take()));
    }
}

pub(super) fn get(key: &WindowKey) -> Option<Logits> {
    ACTIVE.with(|slot| slot.borrow().as_ref()?.logits.get(key).cloned())
}

pub(super) fn insert(key: WindowKey, logits: Logits) {
    ACTIVE.with(|slot| {
        let mut active = slot.borrow_mut();
        let Some(cache) = active.as_mut() else { return };
        let bytes = (key.ids.len() + key.attention.len()) * size_of::<u32>()
            + logits.0.len() * size_of::<i64>()
            + logits.1.len() * size_of::<f32>()
            + size_of::<WindowKey>();
        if bytes <= MAX_BYTES.saturating_sub(cache.bytes) {
            cache.bytes += bytes;
            cache.logits.insert(key, logits);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> WindowKey {
        WindowKey::new(1, &[101, 7, 102], &[1, 1, 1])
    }

    fn put(key: WindowKey) {
        insert(key, Arc::new((vec![1, 3, 1], vec![0.5; 3])));
    }

    #[test]
    fn nothing_is_cached_outside_a_spanning_scope() {
        put(key());
        assert!(get(&key()).is_none());
    }

    #[test]
    fn every_model_input_separates_reuse() {
        let _scope = Spanning::enter();
        put(key());
        assert!(get(&key()).is_some());
        for changed in [
            WindowKey::new(2, &[101, 7, 102], &[1, 1, 1]),
            WindowKey::new(1, &[101, 8, 102], &[1, 1, 1]),
            WindowKey::new(1, &[101, 7, 102], &[1, 1, 0]),
        ] {
            assert!(get(&changed).is_none(), "changed model input reused logits");
        }
    }

    #[test]
    fn dropping_the_scope_releases_the_cache() {
        {
            let _scope = Spanning::enter();
            put(key());
        }
        let _next = Spanning::enter();
        assert!(get(&key()).is_none());
    }

    #[test]
    fn cache_budget_exhaustion_only_causes_misses() {
        let _scope = Spanning::enter();
        let oversized = WindowKey::new(1, &vec![0; MAX_BYTES], &[]);
        put(oversized.clone());
        assert!(get(&oversized).is_none());
        put(key());
        assert!(get(&key()).is_some());
    }
}
