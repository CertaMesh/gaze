//! Request-scoped memo for recognizers that share one expensive computation.

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// Values computed once per detection request and shared by the recognizers of that request.
///
/// Some recognizers are views over one computation: the per-label Nym adapters all read the
/// same model inference. The memo lets the first adapter compute it and the others reuse it.
/// It is owned by the [`DetectContext`](crate::DetectContext) of one request (the per-locale
/// contexts the registry derives with [`DetectContext::narrowed`](crate::DetectContext::narrowed)
/// borrow the same memo) and it is dropped with that context. Nothing is global, so nothing
/// outlives the request and two requests never see each other's values. It is not `Sync`, so
/// concurrent requests cannot share one either.
///
/// Every value is stored under its owner's name and a key the owner derives from everything
/// the value depends on (for Nym: a digest of the input, the model revision and the operating
/// point), so a value computed for one input never answers for another. Each owner keeps at
/// most one value; a new key replaces the old one. Store derived results only (spans,
/// scores), never input text.
#[derive(Default)]
pub struct DetectMemo {
    slots: RefCell<Vec<Slot>>,
}

struct Slot {
    owner: &'static str,
    key: Vec<u8>,
    value: Rc<dyn Any>,
}

impl DetectMemo {
    /// An empty memo.
    pub fn new() -> Self {
        Self::default()
    }

    /// The value `owner` stored under `key`, or the result of `compute` stored in its place.
    ///
    /// A failed `compute` stores nothing, so the next caller computes again. A stored value of
    /// another type counts as absent and is replaced.
    pub fn get_or_try_insert_with<T, E>(
        &self,
        owner: &'static str,
        key: &[u8],
        compute: impl FnOnce() -> Result<T, E>,
    ) -> Result<Rc<T>, E>
    where
        T: Any,
    {
        if let Some(value) = self.get::<T>(owner, key) {
            return Ok(value);
        }
        // Computed outside the borrow so `compute` may itself use the memo.
        let value = Rc::new(compute()?);
        let mut slots = self.slots.borrow_mut();
        slots.retain(|slot| slot.owner != owner);
        slots.push(Slot {
            owner,
            key: key.to_vec(),
            value: Rc::clone(&value) as Rc<dyn Any>,
        });
        Ok(value)
    }

    /// The value `owner` stored under exactly `key`, if it has type `T`.
    pub fn get<T: Any>(&self, owner: &'static str, key: &[u8]) -> Option<Rc<T>> {
        let slots = self.slots.borrow();
        let slot = slots
            .iter()
            .find(|slot| slot.owner == owner && slot.key == key)?;
        Rc::clone(&slot.value).downcast::<T>().ok()
    }

    /// Number of stored values (at most one per owner).
    pub fn len(&self) -> usize {
        self.slots.borrow().len()
    }

    /// Whether nothing is stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Debug for DetectMemo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let slots = self.slots.borrow();
        formatter
            .debug_struct("DetectMemo")
            .field(
                "owners",
                &slots.iter().map(|slot| slot.owner).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::{DetectContext, DictionaryBundle, LocaleTag};

    #[test]
    fn computes_once_per_owner_and_key() {
        let memo = DetectMemo::new();
        let calls = Cell::new(0);
        let compute = || {
            calls.set(calls.get() + 1);
            Ok::<_, ()>(vec![1u8, 2, 3])
        };
        let first = memo.get_or_try_insert_with("nym", b"k1", compute).unwrap();
        let second = memo.get_or_try_insert_with("nym", b"k1", compute).unwrap();
        assert_eq!(calls.get(), 1);
        assert!(Rc::ptr_eq(&first, &second));
    }

    #[test]
    fn a_new_key_replaces_the_owner_slot() {
        let memo = DetectMemo::new();
        memo.get_or_try_insert_with("nym", b"k1", || Ok::<_, ()>(1u32))
            .unwrap();
        let replaced = memo
            .get_or_try_insert_with("nym", b"k2", || Ok::<_, ()>(2u32))
            .unwrap();
        assert_eq!(*replaced, 2);
        assert_eq!(memo.len(), 1, "one slot per owner bounds the memo");
        assert!(memo.get::<u32>("nym", b"k1").is_none());
        memo.get_or_try_insert_with("other", b"k1", || Ok::<_, ()>(3u32))
            .unwrap();
        assert_eq!(memo.len(), 2);
    }

    #[test]
    fn a_failed_compute_stores_nothing() {
        let memo = DetectMemo::new();
        assert_eq!(
            memo.get_or_try_insert_with::<u32, _>("nym", b"k", || Err("down")),
            Err("down")
        );
        assert!(memo.is_empty());
        assert_eq!(
            *memo
                .get_or_try_insert_with("nym", b"k", || Ok::<_, ()>(7u32))
                .unwrap(),
            7
        );
    }

    #[test]
    fn a_value_of_another_type_is_a_miss() {
        let memo = DetectMemo::new();
        memo.get_or_try_insert_with("nym", b"k", || Ok::<_, ()>(1u32))
            .unwrap();
        let value = memo
            .get_or_try_insert_with("nym", b"k", || Ok::<_, ()>("text".to_string()))
            .unwrap();
        assert_eq!(value.as_str(), "text");
    }

    #[test]
    fn narrowed_contexts_share_the_request_memo() {
        let dictionaries = DictionaryBundle::default();
        let chain = [LocaleTag::DeDe, LocaleTag::Global];
        let ctx = DetectContext::new(&chain, &dictionaries);
        ctx.degraded.set(true);
        let narrowed = ctx.narrowed(&chain[..1]);
        assert_eq!(narrowed.locale_chain, &chain[..1]);
        assert!(narrowed.degraded.get());
        narrowed
            .memo()
            .get_or_try_insert_with("nym", b"k", || Ok::<_, ()>(5u32))
            .unwrap();
        assert_eq!(ctx.memo().get::<u32>("nym", b"k").as_deref(), Some(&5));
        let fresh = DetectContext::new(&chain, &dictionaries);
        assert!(
            fresh.memo().is_empty(),
            "a new request starts with an empty memo"
        );
    }
}
