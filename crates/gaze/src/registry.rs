//! Recognizer registry: the single chokepoint every detector passes through.
//!
//! Every type an out-of-crate recognizer needs is re-exported at the crate root, including
//! [`DetectError`] — the error half of [`Recognizer::detect`]'s return type. Adopters who do
//! not depend on `gaze-types` directly can therefore write the full signature with `gaze::`
//! paths alone.
//!
//! ```
//! use gaze::{Candidate, ConflictTier, DetectContext, DetectError, PiiClass, Recognizer};
//!
//! /// Recognizes colon-separated MAC addresses such as `00:1a:2b:3c:4d:5e`: a
//! /// hardware identifier defined by its structure alone, not by any tenant.
//! struct MacAddressRecognizer {
//!     class: PiiClass,
//! }
//!
//! impl Recognizer for MacAddressRecognizer {
//!     fn id(&self) -> &str {
//!         "example.mac_address"
//!     }
//!
//!     fn supported_class(&self) -> &PiiClass {
//!         &self.class
//!     }
//!
//!     fn token_family(&self) -> &str {
//!         "mac_address"
//!     }
//!
//!     fn detect(
//!         &self,
//!         input: &str,
//!         _ctx: &DetectContext<'_>,
//!     ) -> Result<Vec<Candidate>, DetectError> {
//!         // Fail closed: a backend that cannot scan reports an error instead of
//!         // returning "nothing found", which would read as a clean document.
//!         if input.len() > 1_000_000 {
//!             return Err(DetectError::backend(self.id(), "input exceeds scan limit"));
//!         }
//!
//!         const LEN: usize = 17; // six hex pairs joined by five colons
//!         let bytes = input.as_bytes();
//!         let mut candidates = Vec::new();
//!         let mut start = 0;
//!         while start + LEN <= bytes.len() {
//!             let is_mac = bytes[start..start + LEN].iter().enumerate().all(|(i, b)| {
//!                 if i % 3 == 2 {
//!                     *b == b':'
//!                 } else {
//!                     b.is_ascii_hexdigit()
//!                 }
//!             });
//!             if !is_mac {
//!                 start += 1;
//!                 continue;
//!             }
//!             candidates.push(Candidate::new(
//!                 start..start + LEN,
//!                 self.class.clone(),
//!                 self.id(),
//!                 0.9,
//!                 100,
//!                 None,
//!                 self.token_family(),
//!                 self.id(),
//!                 ConflictTier::None,
//!                 Vec::new(),
//!             ));
//!             start += LEN;
//!         }
//!         Ok(candidates)
//!     }
//! }
//!
//! let recognizer = MacAddressRecognizer {
//!     class: PiiClass::Custom("mac_address".to_string()),
//! };
//! let dictionaries = gaze::DictionaryBundle::default();
//! let ctx = DetectContext::new(&[], &dictionaries);
//!
//! let found = recognizer
//!     .detect("device 00:1a:2b:3c:4d:5e joined", &ctx)
//!     .unwrap();
//! assert_eq!(found.len(), 1);
//! assert_eq!(found[0].span, 7..24);
//! ```

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::address_block::{AddressGrammar, AddressVocabulary};
use crate::anchor_resolver::AnchorResolver;
use crate::house_number::{StreetLexicon, StreetNumberOrder};
pub use gaze_types::{Candidate, DetectContext, DetectError, EvidenceKind, Recognizer};
use gaze_types::{
    CollisionMembership, LabelledValueScanReason, LocaleBasis, LocaleChain, LocaleTag, PiiClass,
    LABELLED_FIELD_CONNECTORS,
};

pub trait Validator: Send + Sync {
    fn id(&self) -> &str;
    fn validate(&self, raw: &str) -> ValidationResult;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationResult {
    Valid,
    Invalid,
    Indeterminate,
}

pub trait Canonicalizer: Send + Sync {
    fn canonicalize(&self, raw: &str) -> Option<String>;
}

pub struct RecognizerRegistry {
    entries: Vec<Arc<dyn Recognizer>>,
    recognizers_by_id: HashMap<String, Arc<dyn Recognizer>>,
    validators: HashMap<String, Arc<dyn Validator>>,
    canonicalizers: HashMap<String, Arc<dyn Canonicalizer>>,
    family_policy: FamilyPolicyTable,
    anchor_resolver: AnchorResolver,
    street_lexicon: StreetLexicon,
    address_grammar: AddressGrammar,
}

impl std::fmt::Debug for RecognizerRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecognizerRegistry").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct FamilyPolicyTable {
    inner: FamilyPolicyTableInner,
}

#[derive(Debug, Clone)]
enum FamilyPolicyTableInner {
    Empty,
    Populated {
        by_recognizer: HashMap<String, CollisionMembership>,
        family_index: HashMap<String, FamilyEntry>,
    },
}

#[derive(Debug, Clone, Default)]
struct FamilyEntry {
    variants: HashMap<String, u32>,
}

impl FamilyPolicyTable {
    pub const EMPTY: Self = Self {
        inner: FamilyPolicyTableInner::Empty,
    };

    pub(crate) fn from_memberships(by_recognizer: HashMap<String, CollisionMembership>) -> Self {
        if by_recognizer.is_empty() {
            return Self::EMPTY;
        }
        let mut family_index = HashMap::<String, FamilyEntry>::new();
        for membership in by_recognizer.values() {
            family_index
                .entry(membership.family.clone())
                .or_default()
                .variants
                .entry(membership.variant.clone())
                .and_modify(|precedence| *precedence = (*precedence).min(membership.precedence))
                .or_insert(membership.precedence);
        }
        Self {
            inner: FamilyPolicyTableInner::Populated {
                by_recognizer,
                family_index,
            },
        }
    }

    /// Returns `Some(true)` when `a` wins, `Some(false)` when `b` wins, and
    /// `None` when no family policy applies or precedence is tied.
    pub fn compare(&self, a: &str, b: &str) -> Option<bool> {
        let FamilyPolicyTableInner::Populated {
            by_recognizer,
            family_index,
        } = &self.inner
        else {
            return None;
        };
        let ma = by_recognizer.get(a)?;
        let mb = by_recognizer.get(b)?;
        if ma.family != mb.family || ma.variant == mb.variant {
            return None;
        }
        let family = family_index.get(&ma.family)?;
        let a_precedence = family
            .variants
            .get(&ma.variant)
            .copied()
            .unwrap_or(ma.precedence);
        let b_precedence = family
            .variants
            .get(&mb.variant)
            .copied()
            .unwrap_or(mb.precedence);
        match a_precedence.cmp(&b_precedence) {
            Ordering::Less => Some(true),
            Ordering::Greater => Some(false),
            Ordering::Equal => None,
        }
    }

    pub fn membership(&self, recognizer_id: &str) -> Option<&CollisionMembership> {
        let FamilyPolicyTableInner::Populated { by_recognizer, .. } = &self.inner else {
            return None;
        };
        by_recognizer.get(recognizer_id)
    }

    /// Recognizer ids registered under `family`, in no particular order.
    pub fn member_recognizer_ids<'a>(
        &'a self,
        family: &'a str,
    ) -> impl Iterator<Item = &'a str> + 'a {
        let by_recognizer = match &self.inner {
            FamilyPolicyTableInner::Populated { by_recognizer, .. } => Some(by_recognizer),
            FamilyPolicyTableInner::Empty => None,
        };
        by_recognizer
            .into_iter()
            .flat_map(|memberships| memberships.iter())
            .filter(move |(_, membership)| membership.family == family)
            .map(|(id, _)| id.as_str())
    }

    /// Families that declare a `mandatory_anchor` on at least one member: the
    /// families that emit a `custom:family:<name>` token whenever their anchor
    /// cue is out of range.
    pub fn anchored_families(&self) -> BTreeSet<String> {
        let FamilyPolicyTableInner::Populated { by_recognizer, .. } = &self.inner else {
            return BTreeSet::new();
        };
        by_recognizer
            .values()
            .filter(|membership| membership.mandatory_anchor.is_some())
            .map(|membership| membership.family.clone())
            .collect()
    }

    /// Names of families whose registered recognizers can emit a family token.
    pub fn families(&self) -> impl Iterator<Item = &str> {
        let family_index = match &self.inner {
            FamilyPolicyTableInner::Populated { family_index, .. } => Some(family_index),
            FamilyPolicyTableInner::Empty => None,
        };
        family_index
            .into_iter()
            .flat_map(|families| families.keys().map(String::as_str))
    }

    pub(crate) fn precedence_tie_family(&self, a: &str, b: &str) -> Option<&str> {
        let ma = self.membership(a)?;
        let mb = self.membership(b)?;
        (ma.family == mb.family && ma.variant != mb.variant && ma.precedence == mb.precedence)
            .then_some(ma.family.as_str())
    }
}

impl Default for FamilyPolicyTable {
    fn default() -> Self {
        Self::EMPTY
    }
}

fn labelled_cross_class_boundary_floor(
    recognizer_id: &str,
    capture_end: usize,
    first_group_end: usize,
    validated: bool,
) -> usize {
    // These three rules existed before complete-value scanning. Their original capture must
    // survive even if another class validates a substring inside it.
    if matches!(
        recognizer_id,
        "tax_number.cue_anchored" | "driver_license.cue_anchored" | "national_id.cue_anchored"
    ) || !validated
    {
        capture_end
    } else {
        first_group_end
    }
}

fn uppercase_field_label_before(
    input: &str,
    value_start: usize,
    span_start: usize,
) -> Option<usize> {
    debug_assert!(value_start > span_start);
    if value_start <= span_start || value_start > input.len() {
        return None;
    }
    let bytes = input.as_bytes();
    let mut at = value_start;
    while at > span_start && bytes[at - 1] == b' ' {
        at -= 1;
    }
    if at == span_start || !LABELLED_FIELD_CONNECTORS.contains(&(bytes[at - 1] as char)) {
        return None;
    }
    at -= 1;
    while at > span_start && bytes[at - 1] == b' ' {
        at -= 1;
    }
    let last_end = at;
    while at > span_start && bytes[at - 1].is_ascii_uppercase() {
        at -= 1;
    }
    if last_end - at < 2 {
        return None;
    }
    let last_start = at;
    let last = &input[last_start..last_end];
    while at > span_start && bytes[at - 1] == b' ' {
        at -= 1;
    }
    let previous_end = at;
    while at > span_start && bytes[at - 1].is_ascii_uppercase() {
        at -= 1;
    }
    let previous = &input[at..previous_end];
    let pair = matches!(
        (previous, last),
        ("DRIVER" | "DRIVING", "LICENSE" | "LICENCE")
            | ("NATIONAL", "ID" | "INSURANCE")
            | ("ID" | "IDENTITY", "CARD")
            | ("TAX" | "LICENSE" | "LICENCE" | "PASSPORT", "NUMBER")
            | ("PASSPORT", "ID")
    );
    let start = if pair { at } else { last_start };
    (start == span_start || !bytes[start - 1].is_ascii_alphanumeric()).then_some(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConflictTier, DictionaryBundle, LocaleTag, PiiClass};
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    #[test]
    fn cross_class_boundary_never_cuts_an_original_cue_capture() {
        for rule in [
            "tax_number.cue_anchored",
            "driver_license.cue_anchored",
            "national_id.cue_anchored",
        ] {
            for validated in [false, true] {
                assert_eq!(
                    labelled_cross_class_boundary_floor(rule, 20, 5, validated),
                    20,
                    "{rule} validated={validated}"
                );
            }
        }
        assert_eq!(
            labelled_cross_class_boundary_floor("tax_number.labelled", 20, 5, true),
            5
        );
    }

    #[test]
    fn uppercase_field_label_rejects_an_out_of_range_boundary() {
        assert_eq!(uppercase_field_label_before("ABC: 123", 9, 0), None);
    }

    #[test]
    fn labelled_field_connectors_stay_ascii() {
        assert!(LABELLED_FIELD_CONNECTORS.iter().all(char::is_ascii));
    }

    struct StubRecognizer {
        class: PiiClass,
    }

    impl Recognizer for StubRecognizer {
        fn id(&self) -> &str {
            "stub"
        }

        fn supported_class(&self) -> &PiiClass {
            &self.class
        }

        fn detect(
            &self,
            _input: &str,
            _ctx: &DetectContext<'_>,
        ) -> Result<Vec<Candidate>, DetectError> {
            Ok(vec![Candidate::new(
                0..5,
                self.class.clone(),
                self.id(),
                1.0,
                0,
                Some("canonical".to_string()),
                self.token_family(),
                "test",
                ConflictTier::None,
                Vec::new(),
            )])
        }

        fn token_family(&self) -> &str {
            "counter"
        }
    }

    #[test]
    fn vetoed_locale_candidate_does_not_block_partial_overlap_fallback() {
        struct LocaleProbe {
            id: &'static str,
            locales: Vec<LocaleTag>,
            span: std::ops::Range<usize>,
        }
        impl Recognizer for LocaleProbe {
            fn id(&self) -> &str {
                self.id
            }
            fn supported_class(&self) -> &PiiClass {
                &PiiClass::Email
            }
            fn locales(&self) -> &[LocaleTag] {
                &self.locales
            }
            fn token_family(&self) -> &str {
                "counter"
            }
            fn validator_kind(&self) -> Option<gaze_types::ValidatorKind> {
                Some(gaze_types::ValidatorKind::EmailRfc)
            }
            fn detect(
                &self,
                _: &str,
                _: &DetectContext<'_>,
            ) -> Result<Vec<Candidate>, DetectError> {
                Ok(vec![Candidate::new(
                    self.span.clone(),
                    PiiClass::Email,
                    self.id,
                    1.0,
                    0,
                    None,
                    "counter",
                    self.id,
                    ConflictTier::None,
                    Vec::new(),
                )])
            }
        }
        let input = "!!alice@example.invalid";
        let registry = RecognizerRegistry::builder()
            .register(LocaleProbe {
                id: "global.invalid",
                locales: vec![LocaleTag::Global],
                span: 0..8,
            })
            .register(LocaleProbe {
                id: "regional.valid",
                locales: vec![LocaleTag::DeDe],
                span: 2..input.len(),
            })
            .build();
        let dictionaries = DictionaryBundle::default();
        let locales = [LocaleTag::Global, LocaleTag::DeDe];
        let ctx = DetectContext::new(&locales, &dictionaries);
        let (found, vetoed) = registry.detect_all_resolved(input, &ctx).expect("detect");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].recognizer_id, "regional.valid");
        assert_eq!(found[0].span, 2..input.len());
        assert_eq!(vetoed.len(), 1);
        assert_eq!(vetoed[0].candidate.recognizer_id, "global.invalid");
    }

    #[test]
    fn registry_detect_all_uses_registered_recognizers() {
        let registry = RecognizerRegistry::builder()
            .register(StubRecognizer {
                class: PiiClass::Email,
            })
            .build();
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);

        let candidates = registry.detect_all("input", &ctx).expect("detect all");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].class, PiiClass::Email);
        assert_eq!(candidates[0].token_family, "counter");

        let (candidates, vetoed) = registry
            .detect_all_resolved("input", &ctx)
            .expect("detect all resolved");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].class, PiiClass::Email);
        assert!(vetoed.is_empty());
    }

    #[test]
    fn post_floor_recognizer_sees_raw_prior_candidates() {
        struct NeedsPrior {
            class: PiiClass,
        }

        impl Recognizer for NeedsPrior {
            fn id(&self) -> &str {
                "needs_prior"
            }

            fn supported_class(&self) -> &PiiClass {
                &self.class
            }

            fn token_family(&self) -> &str {
                "counter"
            }

            fn requires_prior_candidates(&self) -> bool {
                true
            }

            fn detect(
                &self,
                _input: &str,
                ctx: &DetectContext<'_>,
            ) -> Result<Vec<Candidate>, DetectError> {
                let prior = ctx.prior_candidates.expect("post-floor context");
                assert_eq!(prior.len(), 1);
                assert_eq!(prior[0].recognizer_id, "stub");
                Ok(vec![Candidate::new(
                    5..10,
                    self.class.clone(),
                    self.id(),
                    1.0,
                    0,
                    None,
                    self.token_family(),
                    self.id(),
                    ConflictTier::None,
                    Vec::new(),
                )])
            }
        }

        let registry = RecognizerRegistry::builder()
            .register(NeedsPrior {
                class: PiiClass::Name,
            })
            .register(StubRecognizer {
                class: PiiClass::Email,
            })
            .build();
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
        assert_eq!(registry.detect_all("1234567890", &ctx).unwrap().len(), 2);
        assert_eq!(
            registry
                .detect_all_resolved("1234567890", &ctx)
                .unwrap()
                .0
                .len(),
            2
        );
    }

    #[test]
    fn default_locale_is_global() {
        let recognizer = StubRecognizer {
            class: PiiClass::Email,
        };

        assert_eq!(recognizer.locales(), &[LocaleTag::Global]);
    }

    #[test]
    fn registry_filters_recognizers_by_locale_before_detection() {
        struct LocaleRecognizer {
            locale: LocaleTag,
        }

        impl Recognizer for LocaleRecognizer {
            fn id(&self) -> &str {
                "locale"
            }

            fn supported_class(&self) -> &PiiClass {
                &PiiClass::Email
            }

            fn detect(
                &self,
                _input: &str,
                _ctx: &DetectContext<'_>,
            ) -> Result<Vec<Candidate>, DetectError> {
                Ok(vec![Candidate::new(
                    0..5,
                    PiiClass::Email,
                    self.id(),
                    1.0,
                    0,
                    None,
                    "counter",
                    self.id(),
                    ConflictTier::None,
                    Vec::new(),
                )])
            }

            fn token_family(&self) -> &str {
                "counter"
            }

            fn locales(&self) -> &[LocaleTag] {
                std::slice::from_ref(&self.locale)
            }
        }

        let registry = RecognizerRegistry::builder()
            .register(LocaleRecognizer {
                locale: LocaleTag::DeDe,
            })
            .build();
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::EnUs, LocaleTag::Global], &dictionaries);

        assert!(registry
            .detect_all("input", &ctx)
            .expect("detect all")
            .is_empty());
    }

    struct BasisRecognizer {
        id: &'static str,
        class: PiiClass,
        locale: LocaleTag,
        locale_basis: LocaleBasis,
        span: std::ops::Range<usize>,
        calls: Arc<AtomicUsize>,
    }

    impl Recognizer for BasisRecognizer {
        fn id(&self) -> &str {
            self.id
        }

        fn supported_class(&self) -> &PiiClass {
            &self.class
        }

        fn detect(
            &self,
            _input: &str,
            _ctx: &DetectContext<'_>,
        ) -> Result<Vec<Candidate>, DetectError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(vec![Candidate::new(
                self.span.clone(),
                self.class.clone(),
                self.id(),
                1.0,
                0,
                None,
                "counter",
                self.id(),
                ConflictTier::None,
                Vec::new(),
            )])
        }

        fn token_family(&self) -> &str {
            "counter"
        }

        fn locales(&self) -> &[LocaleTag] {
            std::slice::from_ref(&self.locale)
        }

        fn locale_basis(&self) -> LocaleBasis {
            self.locale_basis
        }
    }

    #[test]
    fn format_basis_recognizer_ignores_document_locale_in_detect_all() {
        let calls = Arc::new(AtomicUsize::new(0));
        let registry = RecognizerRegistry::builder()
            .register(BasisRecognizer {
                id: "format",
                class: PiiClass::Email,
                locale: LocaleTag::DeDe,
                locale_basis: LocaleBasis::Format,
                span: 0..5,
                calls: Arc::clone(&calls),
            })
            .build();
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::EnUs, LocaleTag::Global], &dictionaries);

        let candidates = registry.detect_all("input", &ctx).expect("detect all");

        assert_eq!(candidates.len(), 1);
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn format_basis_runs_once_and_unions_with_document_fallback() {
        let format_calls = Arc::new(AtomicUsize::new(0));
        let document_calls = Arc::new(AtomicUsize::new(0));
        let registry = RecognizerRegistry::builder()
            .register(BasisRecognizer {
                id: "format",
                class: PiiClass::Email,
                locale: LocaleTag::Other("fr-FR".to_string()),
                locale_basis: LocaleBasis::Format,
                span: 0..5,
                calls: Arc::clone(&format_calls),
            })
            .register(BasisRecognizer {
                id: "document",
                class: PiiClass::Email,
                locale: LocaleTag::DeDe,
                locale_basis: LocaleBasis::Document,
                span: 5..10,
                calls: Arc::clone(&document_calls),
            })
            .build();
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(
            &[LocaleTag::EnUs, LocaleTag::DeDe, LocaleTag::Global],
            &dictionaries,
        );

        let (candidates, vetoed) = registry
            .detect_all_resolved("abcdefghij", &ctx)
            .expect("detect all resolved");

        assert_eq!(candidates.len(), 2);
        assert!(vetoed.is_empty());
        assert_eq!(format_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(document_calls.load(AtomicOrdering::SeqCst), 1);
    }

    fn document_recognizer(
        id: &'static str,
        locale: LocaleTag,
        span: std::ops::Range<usize>,
    ) -> BasisRecognizer {
        BasisRecognizer {
            id,
            class: PiiClass::Email,
            locale,
            locale_basis: LocaleBasis::Document,
            span,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn pool_ids(registry: &RecognizerRegistry, chain: &[LocaleTag]) -> Vec<String> {
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(chain, &dictionaries);
        let (pool, _) = registry
            .detect_candidate_pool("abcdefghij", &ctx)
            .expect("candidate pool");
        pool.originals()
            .iter()
            .map(|candidate| candidate.recognizer_id.clone())
            .collect()
    }

    // A match at an earlier chain locale must not switch off a later locale's rule of the
    // same class elsewhere in the document; that shadowing shipped the second span raw.
    #[test]
    fn later_locale_candidate_outside_earlier_spans_is_kept() {
        let registry = RecognizerRegistry::builder()
            .register(document_recognizer("de-at", LocaleTag::DeAt, 0..5))
            .register(document_recognizer("de-de", LocaleTag::DeDe, 5..10))
            .build();

        assert_eq!(
            pool_ids(&registry, &[LocaleTag::DeAt, LocaleTag::DeDe]),
            vec!["de-at", "de-de"]
        );
    }

    // Mirror of the case above: a later candidate ending exactly where a claimed span
    // starts touches it but does not overlap it.
    #[test]
    fn later_locale_candidate_ending_at_earlier_span_start_is_kept() {
        let registry = RecognizerRegistry::builder()
            .register(document_recognizer("de-at", LocaleTag::DeAt, 5..10))
            .register(document_recognizer("de-de", LocaleTag::DeDe, 0..5))
            .build();

        assert_eq!(
            pool_ids(&registry, &[LocaleTag::DeAt, LocaleTag::DeDe]),
            vec!["de-at", "de-de"]
        );
    }

    #[test]
    fn later_locale_candidate_overlapping_earlier_span_is_dropped() {
        let registry = RecognizerRegistry::builder()
            .register(document_recognizer("de-de", LocaleTag::DeDe, 3..8))
            .register(document_recognizer("de-at", LocaleTag::DeAt, 0..5))
            .build();

        assert_eq!(
            pool_ids(&registry, &[LocaleTag::DeAt, LocaleTag::DeDe]),
            vec!["de-at"]
        );
        assert_eq!(
            pool_ids(&registry, &[LocaleTag::DeDe, LocaleTag::DeAt]),
            vec!["de-de"]
        );
    }

    /// `ClaimedSpans` reads only the start window that can overlap.
    /// Pin it against the full scan it replaced, on random span sets that
    /// include empty, equal, nested and very long spans.
    #[test]
    fn claimed_spans_match_the_full_scan() {
        fn full_scan(
            claimed: &[std::ops::Range<usize>],
            candidate: &std::ops::Range<usize>,
        ) -> bool {
            claimed.iter().any(|span| {
                let overlaps = span.start < candidate.end && candidate.start < span.end;
                let strict_containment = (span.start <= candidate.start
                    && candidate.end <= span.end
                    || candidate.start <= span.start && span.end <= candidate.end)
                    && span != candidate;
                overlaps && !strict_containment
            })
        }
        fn next(state: &mut u64) -> u64 {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            *state
        }
        fn span(state: &mut u64) -> std::ops::Range<usize> {
            let start = (next(state) % 60) as usize;
            let len = match next(state) % 10 {
                0 => (next(state) % 50) as usize,
                _ => (next(state) % 6) as usize,
            };
            start..start + len
        }
        let mut state = 0x3895_u64;
        let (mut blocked, mut passed) = (0, 0);
        for _ in 0..5_000 {
            let spans = (0..next(&mut state) % 12)
                .map(|_| span(&mut state))
                .collect::<Vec<_>>();
            let mut claimed = super::ClaimedSpans::default();
            claimed.extend(spans.iter());
            for _ in 0..8 {
                let candidate = span(&mut state);
                let expected = full_scan(&spans, &candidate);
                assert_eq!(
                    claimed.blocks(&candidate),
                    expected,
                    "{spans:?} {candidate:?}"
                );
                if expected {
                    blocked += 1;
                } else {
                    passed += 1;
                }
            }
        }
        assert!(blocked > 1_000 && passed > 1_000, "{blocked} / {passed}");
    }

    #[test]
    fn strict_same_class_containment_reaches_resolver_in_either_locale_order() {
        let registry = RecognizerRegistry::builder()
            .register(document_recognizer("short", LocaleTag::DeAt, 4..8))
            .register(document_recognizer("long", LocaleTag::EnAu, 0..8))
            .build();
        for chain in [
            [LocaleTag::DeAt, LocaleTag::EnAu],
            [LocaleTag::EnAu, LocaleTag::DeAt],
        ] {
            let ids = pool_ids(&registry, &chain);
            assert_eq!(ids.len(), 2, "both candidates reach arbitration: {ids:?}");
            let dictionaries = DictionaryBundle::default();
            let ctx = DetectContext::new(&chain, &dictionaries);
            let (resolved, vetoed) = registry
                .detect_all_resolved("abcdefghij", &ctx)
                .expect("resolve containment");
            assert!(vetoed.is_empty());
            assert_eq!(resolved.len(), 1);
            assert_eq!(resolved[0].span, 0..8);
            assert_eq!(resolved[0].recognizer_id, "long");
            assert_eq!(resolved[0].decided_by, ConflictTier::SameClassContainment);
            assert!(resolved[0].merged_sources.contains(&"short".to_string()));
        }
    }

    // Global rules run at every chain step and repeat their spans; the repeats must not
    // multiply candidates.
    #[test]
    fn global_recognizer_repeated_across_chain_yields_one_candidate() {
        let registry = RecognizerRegistry::builder()
            .register(document_recognizer("global", LocaleTag::Global, 0..5))
            .build();

        assert_eq!(
            pool_ids(
                &registry,
                &[LocaleTag::DeDe, LocaleTag::EnUs, LocaleTag::Global]
            ),
            vec!["global"]
        );
    }

    struct LocaleInvariant(BasisRecognizer);

    impl Recognizer for LocaleInvariant {
        fn id(&self) -> &str {
            self.0.id()
        }

        fn supported_class(&self) -> &PiiClass {
            self.0.supported_class()
        }

        fn detect(
            &self,
            input: &str,
            ctx: &DetectContext<'_>,
        ) -> Result<Vec<Candidate>, DetectError> {
            self.0.detect(input, ctx)
        }

        fn token_family(&self) -> &str {
            self.0.token_family()
        }

        fn locales(&self) -> &[LocaleTag] {
            self.0.locales()
        }

        fn detect_is_locale_invariant(&self) -> bool {
            true
        }
    }

    fn fifteen_step_chain() -> Vec<LocaleTag> {
        let mut chain = vec![LocaleTag::EnUs, LocaleTag::DeDe, LocaleTag::DeAt];
        chain.extend((0..11).map(|i| LocaleTag::Other(format!("x{i}-XX"))));
        chain.push(LocaleTag::Global);
        chain
    }

    // A locale-invariant recognizer is offered all 15 steps but detects once; a recognizer that
    // keeps the default still runs at every step it is active for.
    #[test]
    fn locale_invariant_recognizer_detects_once_per_document() {
        let invariant = document_recognizer("invariant", LocaleTag::Global, 0..5);
        let invariant_calls = Arc::clone(&invariant.calls);
        let per_step = BasisRecognizer {
            class: PiiClass::Name,
            ..document_recognizer("per-step", LocaleTag::Global, 5..10)
        };
        let per_step_calls = Arc::clone(&per_step.calls);
        let registry = RecognizerRegistry::builder()
            .register(LocaleInvariant(invariant))
            .register(per_step)
            .build();

        assert_eq!(
            pool_ids(&registry, &fifteen_step_chain()),
            vec!["invariant", "per-step"]
        );
        assert_eq!(invariant_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(per_step_calls.load(AtomicOrdering::SeqCst), 15);
    }

    // Reused candidates still pass the claimed-span filter of the step they are reused at: a
    // de-DE-only invariant rule is first eligible at step two and loses the span de-AT claimed.
    #[test]
    fn reused_candidates_keep_per_span_claiming() {
        let invariant = document_recognizer("invariant", LocaleTag::DeDe, 3..8);
        let invariant_calls = Arc::clone(&invariant.calls);
        let registry = RecognizerRegistry::builder()
            .register(LocaleInvariant(invariant))
            .register(document_recognizer("de-at", LocaleTag::DeAt, 0..5))
            .build();

        assert_eq!(
            pool_ids(
                &registry,
                &[LocaleTag::DeAt, LocaleTag::DeDe, LocaleTag::Global]
            ),
            vec!["de-at"]
        );
        assert_eq!(invariant_calls.load(AtomicOrdering::SeqCst), 1);
    }

    // Keeps the default and emits only at the de-DE step, so running it once at en-US drops it.
    struct StepLocale;

    impl Recognizer for StepLocale {
        fn id(&self) -> &str {
            "step-locale"
        }

        fn supported_class(&self) -> &PiiClass {
            &PiiClass::Name
        }

        fn detect(
            &self,
            _input: &str,
            ctx: &DetectContext<'_>,
        ) -> Result<Vec<Candidate>, DetectError> {
            if ctx.locale_chain.first() != Some(&LocaleTag::DeDe) {
                return Ok(Vec::new());
            }
            Ok(vec![Candidate::new(
                0..3,
                PiiClass::Name,
                "step-locale",
                1.0,
                0,
                None,
                "counter",
                "step-locale",
                ConflictTier::None,
                Vec::new(),
            )])
        }

        fn token_family(&self) -> &str {
            "counter"
        }

        fn locales(&self) -> &[LocaleTag] {
            std::slice::from_ref(&LocaleTag::Global)
        }
    }

    #[test]
    fn default_recognizer_still_sees_every_locale_step() {
        let registry = RecognizerRegistry::builder().register(StepLocale).build();
        assert_eq!(
            pool_ids(
                &registry,
                &[LocaleTag::EnUs, LocaleTag::DeDe, LocaleTag::Global]
            ),
            vec!["step-locale"]
        );
    }

    #[test]
    fn empty_family_policy_never_applies() {
        assert_eq!(FamilyPolicyTable::EMPTY.compare("a", "b"), None);
    }

    #[test]
    fn registry_builder_compiles_family_policy_table() {
        let registry = RecognizerRegistry::builder()
            .register_collision(
                "tenant.alpha",
                CollisionMembership::new("tenant-doc", "alpha", 10, None),
            )
            .register_collision(
                "tenant.beta",
                CollisionMembership::new("tenant-doc", "beta", 20, None),
            )
            .register_collision(
                "tenant.gamma",
                CollisionMembership::new("other-doc", "gamma", 5, None),
            )
            .build();

        assert_eq!(
            registry
                .family_policy()
                .membership("tenant.alpha")
                .map(|membership| membership.variant.as_str()),
            Some("alpha")
        );
        assert_eq!(
            registry
                .family_policy()
                .compare("tenant.alpha", "tenant.beta"),
            Some(true)
        );
        assert_eq!(
            registry
                .family_policy()
                .compare("tenant.beta", "tenant.alpha"),
            Some(false)
        );
        assert_eq!(
            registry
                .family_policy()
                .compare("tenant.alpha", "tenant.gamma"),
            None
        );
    }

    // Dangling memberships can change real variants' precedence through min aggregation.
    // Assembly must register collision metadata only after building its recognizer.
    #[test]
    fn dangling_collision_membership_flips_family_precedence() {
        let clean = RecognizerRegistry::builder()
            .register_collision(
                "ssn.de_cue",
                CollisionMembership::new("government-id", "ssn", 10, None),
            )
            .register_collision(
                "tax_number.cue_anchored",
                CollisionMembership::new("government-id", "tax-number", 20, None),
            )
            .build();

        assert_eq!(
            clean
                .family_policy()
                .compare("tax_number.cue_anchored", "ssn.de_cue"),
            Some(false)
        );

        let polluted = RecognizerRegistry::builder()
            .register_collision(
                "ssn.de_cue",
                CollisionMembership::new("government-id", "ssn", 10, None),
            )
            .register_collision(
                "tax_number.cue_anchored",
                CollisionMembership::new("government-id", "tax-number", 20, None),
            )
            .register_collision(
                "custom.name_marker",
                CollisionMembership::new("government-id", "tax-number", 5, None),
            )
            .build();

        assert_eq!(
            polluted
                .family_policy()
                .compare("tax_number.cue_anchored", "ssn.de_cue"),
            Some(true)
        );
    }
}

/// Runs one recognizer and stamps its declared [`Recognizer::evidence`] on every candidate.
/// Every registry detect call goes through here, so the sweep reads a declaration the
/// recognizer made, never a guess from its id.
fn detect_declared(
    recognizer: &dyn Recognizer,
    input: &str,
    ctx: &DetectContext<'_>,
) -> Result<Vec<Candidate>, DetectError> {
    let evidence = recognizer.evidence();
    Ok(recognizer
        .detect_for_registry(input, ctx)?
        .into_iter()
        .map(|candidate| candidate.with_evidence(evidence))
        .collect())
}

/// Spans an earlier locale of one class already matched.
///
/// A later locale's candidate is blocked by a claimed span it partially
/// overlaps or equals; strict same-class containment in either direction
/// passes, so the resolver can pick the enclosing span. Checking every
/// claimed span made each locale step O(N^2). A span overlapping
/// `start..end` starts after `start - longest`, so only that window is read.
#[derive(Default)]
struct ClaimedSpans {
    by_start: BTreeSet<(usize, usize)>,
    longest: usize,
}

impl ClaimedSpans {
    fn blocks(&self, candidate: &std::ops::Range<usize>) -> bool {
        let from = (candidate.start.saturating_sub(self.longest), 0);
        self.by_start
            .range(from..(candidate.end, 0))
            .any(|&(start, end)| {
                let span = start..end;
                let overlaps = span.start < candidate.end && candidate.start < span.end;
                let strict_containment = (span.start <= candidate.start
                    && candidate.end <= span.end
                    || candidate.start <= span.start && span.end <= candidate.end)
                    && &span != candidate;
                overlaps && !strict_containment
            })
    }

    fn extend<'a>(&mut self, spans: impl Iterator<Item = &'a std::ops::Range<usize>>) {
        for span in spans {
            self.longest = self.longest.max(span.len());
            self.by_start.insert((span.start, span.end));
        }
    }
}

impl RecognizerRegistry {
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn builder() -> RecognizerRegistryBuilder {
        RecognizerRegistryBuilder::default()
    }

    pub fn detect_all(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
    ) -> Result<Vec<Candidate>, DetectError> {
        let locale_chain = LocaleChain::from(ctx.locale_chain);
        let mut candidates = Vec::new();
        for recognizer in self.entries.iter().filter(|recognizer| {
            !recognizer.requires_prior_candidates()
                && (recognizer.locale_basis() == LocaleBasis::Format
                    || locale_chain.intersects(recognizer.locales()))
        }) {
            candidates.extend(detect_declared(recognizer.as_ref(), input, ctx)?);
        }
        let post_candidates = self.detect_post_candidates(input, ctx, &candidates)?;
        candidates.extend(post_candidates);
        Ok(candidates)
    }

    fn detect_post_candidates(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
        prior: &[Candidate],
    ) -> Result<Vec<Candidate>, DetectError> {
        let locale_chain = LocaleChain::from(ctx.locale_chain);
        let mut prior_ctx =
            DetectContext::new(ctx.locale_chain, ctx.dictionaries).with_prior_candidates(prior);
        prior_ctx.source_spans = ctx.source_spans;
        prior_ctx.degraded.set(ctx.degraded.get());
        let mut added = Vec::new();
        for recognizer in self.entries.iter().filter(|recognizer| {
            recognizer.requires_prior_candidates()
                && (recognizer.locale_basis() == LocaleBasis::Format
                    || locale_chain.intersects(recognizer.locales()))
        }) {
            added.extend(detect_declared(recognizer.as_ref(), input, &prior_ctx)?);
        }
        ctx.degraded.set(prior_ctx.degraded.get());
        Ok(added)
    }

    pub fn detect_all_resolved(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
    ) -> Result<(Vec<Candidate>, Vec<crate::validator_veto::VetoedCandidate>), DetectError> {
        let (mut pool, vetoed) = self.detect_candidate_pool(input, ctx)?;
        let order = pool.order.clone();
        let resolved = self.resolve_pool(&mut pool, &order, input, ctx.locale_chain);
        Ok((
            resolved.into_iter().map(|node| node.candidate).collect(),
            vetoed,
        ))
    }

    pub(crate) fn resolve_pool(
        &self,
        pool: &mut crate::resolver::CandidatePool,
        ids: &[usize],
        input: &str,
        locale_chain: &[LocaleTag],
    ) -> Vec<crate::resolver::WholeCandidate> {
        let locale_chain = LocaleChain::from(locale_chain);
        pool.resolve(
            ids,
            self.family_policy(),
            Some((&self.anchor_resolver, input, locale_chain.as_slice())),
        )
    }

    pub(crate) fn effective_view(
        &self,
        candidate: &Candidate,
        input: &str,
        locales: &[LocaleTag],
    ) -> (PiiClass, String) {
        let locales = LocaleChain::from(locales);
        crate::resolver::effective_view(
            candidate,
            &self.family_policy,
            &self.anchor_resolver,
            input,
            locales.as_slice(),
        )
    }

    pub(crate) fn detect_candidate_pool(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
    ) -> Result<
        (
            crate::resolver::CandidatePool,
            Vec<crate::validator_veto::VetoedCandidate>,
        ),
        DetectError,
    > {
        let locale_chain = LocaleChain::from(ctx.locale_chain);
        let classes = self
            .entries
            .iter()
            .map(|recognizer| recognizer.supported_class().clone())
            .collect::<BTreeSet<_>>();
        let mut candidates = Vec::new();

        for class in classes {
            for recognizer in self
                .entries
                .iter()
                .filter(|recognizer| recognizer.supported_class() == &class)
                .filter(|recognizer| recognizer.locale_basis() == LocaleBasis::Format)
                .filter(|recognizer| !recognizer.requires_prior_candidates())
            {
                candidates.extend(
                    detect_declared(recognizer.as_ref(), input, ctx)?
                        .into_iter()
                        .filter(|candidate| candidate.score >= min_score(&class)),
                );
            }

            // Earlier chain locales win per span, except strict same-class containment:
            // keep both spans so the resolver can choose the enclosing one and audit the loser.
            // Only validator survivors claim spans. Keep the raw candidates for the later
            // post-floor pass and the single audit veto pass; an invalid earlier-locale
            // match must never suppress a valid partially overlapping fallback.
            let mut claimed = ClaimedSpans::default();
            let mut guard_audit_seen = BTreeSet::new();
            let mut veto_audit_seen = BTreeSet::new();
            // Locale-invariant recognizers detect at their first eligible step; later steps
            // reuse that output, so NER infers once per document instead of once per step.
            let mut reused: HashMap<usize, Vec<Candidate>> = HashMap::new();
            for locale in locale_chain.as_slice() {
                let mut locale_ctx =
                    DetectContext::new(std::slice::from_ref(locale), ctx.dictionaries);
                locale_ctx.source_spans = ctx.source_spans;
                locale_ctx.degraded.set(ctx.degraded.get());
                let mut class_candidates = Vec::new();
                for (index, recognizer) in self
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, recognizer)| recognizer.supported_class() == &class)
                    .filter(|(_, recognizer)| recognizer.locale_basis() == LocaleBasis::Document)
                    .filter(|(_, recognizer)| !recognizer.requires_prior_candidates())
                    .filter(|(_, recognizer)| {
                        LocaleChain::from(locale_ctx.locale_chain).intersects(recognizer.locales())
                    })
                {
                    let fresh;
                    let detected: &[Candidate] = if recognizer.detect_is_locale_invariant() {
                        match reused.entry(index) {
                            std::collections::hash_map::Entry::Occupied(slot) => slot.into_mut(),
                            std::collections::hash_map::Entry::Vacant(slot) => slot
                                .insert(detect_declared(recognizer.as_ref(), input, &locale_ctx)?),
                        }
                    } else {
                        fresh = detect_declared(recognizer.as_ref(), input, &locale_ctx)?;
                        &fresh
                    };
                    class_candidates.extend(
                        detected
                            .iter()
                            .filter(|candidate| candidate.score >= min_score(&class))
                            .filter(|candidate| {
                                if candidate.regex_guard_rejected {
                                    guard_audit_seen.insert((
                                        candidate.recognizer_id.clone(),
                                        candidate.span.start,
                                        candidate.span.end,
                                    ))
                                } else {
                                    !claimed.blocks(&candidate.span)
                                }
                            })
                            .cloned(),
                    );
                }
                let (survivors, vetoed) = crate::validator_veto::apply(
                    class_candidates.clone(),
                    self,
                    input,
                    ctx.source_spans,
                );
                claimed.extend(survivors.iter().map(|candidate| &candidate.span));
                let vetoed_keys = vetoed
                    .into_iter()
                    .map(|veto| {
                        (
                            veto.candidate.recognizer_id,
                            veto.candidate.span.start,
                            veto.candidate.span.end,
                        )
                    })
                    .collect::<BTreeSet<_>>();
                candidates.extend(class_candidates.into_iter().filter(|candidate| {
                    let key = (
                        candidate.recognizer_id.clone(),
                        candidate.span.start,
                        candidate.span.end,
                    );
                    !vetoed_keys.contains(&key) || veto_audit_seen.insert(key)
                }));
            }
        }

        let active_candidates = candidates
            .iter()
            .filter(|candidate| !candidate.regex_guard_rejected)
            .cloned()
            .collect::<Vec<_>>();
        let post_candidates = self.detect_post_candidates(input, ctx, &active_candidates)?;
        candidates.extend(
            post_candidates
                .into_iter()
                .filter(|candidate| candidate.score >= min_score(&candidate.class)),
        );

        let (mut candidates, vetoed) =
            crate::validator_veto::apply(candidates, self, input, ctx.source_spans);

        // A labelled capture may extend through grouped value bytes, but an independently
        // validated value of another class starts a new field. A vetoed lookalike must never
        // shorten the value and leave its suffix raw.
        let boundaries = candidates
            .iter()
            .filter(|candidate| !candidate.regex_guard_rejected)
            .map(|candidate| {
                (
                    candidate.span.start,
                    candidate.class.clone(),
                    candidate.checksum_validated(),
                )
            })
            .collect::<Vec<_>>();
        for candidate in &mut candidates {
            // A labelled phone run can include an adjacent IP or card. Preserve the phone
            // separately before resolution, even when that tail has a higher rule priority.
            // Only an independently validated other class can cut the labelled value.
            if self
                .recognizer(&candidate.recognizer_id)
                .and_then(|recognizer| recognizer.validator_kind())
                .is_some_and(gaze_types::ValidatorKind::is_phone)
            {
                if let Some(boundary) = boundaries
                    .iter()
                    .filter(|(start, class, validated)| {
                        *validated
                            && class != &candidate.class
                            && *start > candidate.span.start
                            && *start < candidate.span.end
                    })
                    .map(|(start, _, _)| *start)
                    .min()
                {
                    candidate.span.end = candidate.span.start
                        + input[candidate.span.start..boundary]
                            .trim_end_matches(|ch: char| {
                                ch.is_whitespace() || matches!(ch, '-' | '/' | '.' | ':')
                            })
                            .len();
                    candidate.labelled_value_scan_reason =
                        Some(LabelledValueScanReason::OtherClassBoundary);
                    let recognizer = self
                        .recognizer(&candidate.recognizer_id)
                        .expect("declared recognizer");
                    let kind = recognizer.validator_kind().expect("phone validator");
                    match kind.validate(&input[candidate.span.clone()]) {
                        gaze_types::ValidatorOutcome::Pass { canonical_form } => {
                            candidate.canonical_form = canonical_form;
                            candidate.validator_fail_reason = None;
                            candidate.evidence = recognizer.evidence();
                        }
                        gaze_types::ValidatorOutcome::Fail { reason } => {
                            candidate.canonical_form = None;
                            candidate.validator_fail_reason = Some(reason);
                            candidate.evidence = gaze_types::EvidenceKind::Learned;
                        }
                        _ => {}
                    }
                }
                // Splitting or cutting a recorded phone must leave separators outside
                // tokens. Keep the trim local so an empty prefix cannot move backwards.
                candidate.span.end = candidate.span.start
                    + input[candidate.span.clone()]
                        .trim_end_matches(char::is_whitespace)
                        .len();
                continue;
            }
            let Some(capture_end) = candidate.labelled_value_capture_end else {
                continue;
            };
            let prefix_start = candidate.span.start
                + input[candidate.span.start..]
                    .bytes()
                    .take_while(|byte| *byte == b'<')
                    .count();
            let first_group_end = prefix_start
                + input[prefix_start..]
                    .bytes()
                    .take_while(u8::is_ascii_alphanumeric)
                    .count();
            let first_group_end = if first_group_end == prefix_start {
                capture_end
            } else {
                first_group_end
            };
            let first_group = &input[prefix_start..first_group_end];
            // A long all-digit ID can contain card-like windows. They are not evidence that a
            // new field starts inside that ID; trimming there would expose its tail.
            if !first_group.bytes().any(|byte| byte.is_ascii_alphabetic())
                || !first_group.bytes().any(|byte| byte.is_ascii_digit())
            {
                continue;
            }
            let next = boundaries
                .iter()
                .filter_map(|(start, class, validated)| {
                    if *start <= candidate.span.start
                        || *start >= candidate.span.end
                        || class == &candidate.class
                    {
                        return None;
                    }
                    let floor = labelled_cross_class_boundary_floor(
                        &candidate.recognizer_id,
                        capture_end,
                        first_group_end,
                        *validated,
                    );
                    let label = uppercase_field_label_before(input, *start, candidate.span.start)
                        .filter(|label_start| {
                            *label_start >= first_group_end
                                && (!matches!(
                                    candidate.recognizer_id.as_str(),
                                    "tax_number.cue_anchored"
                                        | "driver_license.cue_anchored"
                                        | "national_id.cue_anchored"
                                ) || *label_start >= capture_end)
                        });
                    if *start < floor && label.is_none() {
                        return None;
                    }
                    Some(label.unwrap_or(*start))
                })
                .min();
            if let Some(boundary) = next {
                let end = input[..boundary]
                    .trim_end_matches(|ch: char| {
                        ch.is_whitespace() || matches!(ch, '-' | '/' | '.' | ':')
                    })
                    .len()
                    .max(first_group_end);
                candidate.span.end = end;
                candidate.labelled_value_scan_reason =
                    Some(LabelledValueScanReason::OtherClassBoundary);
            }
        }

        candidates.retain(|candidate| {
            !self
                .recognizer(&candidate.recognizer_id)
                .and_then(|recognizer| recognizer.validator_kind())
                .is_some_and(gaze_types::ValidatorKind::is_phone)
                || input[candidate.span.clone()]
                    .bytes()
                    .any(|byte| byte.is_ascii_digit())
        });
        Ok((crate::resolver::CandidatePool::new(candidates), vetoed))
    }

    pub(crate) fn street_lexicon(&self) -> &StreetLexicon {
        &self.street_lexicon
    }

    pub(crate) fn address_grammar(&self) -> &AddressGrammar {
        &self.address_grammar
    }

    pub fn recognizer(&self, id: &str) -> Option<&Arc<dyn Recognizer>> {
        self.recognizers_by_id.get(id)
    }

    /// IDs of recognizers admitted to this registry.
    pub fn recognizer_ids(&self) -> impl Iterator<Item = &str> {
        self.recognizers_by_id.keys().map(String::as_str)
    }

    pub fn validators(&self) -> &HashMap<String, Arc<dyn Validator>> {
        &self.validators
    }

    pub fn canonicalizers(&self) -> &HashMap<String, Arc<dyn Canonicalizer>> {
        &self.canonicalizers
    }

    pub fn family_policy(&self) -> &FamilyPolicyTable {
        &self.family_policy
    }

    /// Classes emitted by the recognizers registered under collision family
    /// `family`, sorted and deduplicated. A family-level token that no policy
    /// rule names derives its action from these classes' rules.
    pub fn family_member_classes(&self, family: &str) -> Vec<PiiClass> {
        let mut classes = self
            .family_policy
            .member_recognizer_ids(family)
            .filter_map(|id| self.recognizers_by_id.get(id))
            .map(|recognizer| recognizer.supported_class().clone())
            .collect::<Vec<_>>();
        classes.sort();
        classes.dedup();
        classes
    }
}

fn min_score(_class: &PiiClass) -> f32 {
    0.0
}

#[derive(Default)]
pub struct RecognizerRegistryBuilder {
    entries: Vec<Arc<dyn Recognizer>>,
    collision_memberships: HashMap<String, CollisionMembership>,
    anchor_resolver: AnchorResolver,
    street_lexicon: StreetLexicon,
    address_grammar: AddressGrammar,
}

impl RecognizerRegistryBuilder {
    pub fn register<R: Recognizer + 'static>(mut self, r: R) -> Self {
        self.entries.push(Arc::new(r));
        self
    }

    pub fn register_arc(mut self, r: Arc<dyn Recognizer>) -> Self {
        self.entries.push(r);
        self
    }

    pub fn register_collision(
        mut self,
        recognizer_id: impl Into<String>,
        membership: CollisionMembership,
    ) -> Self {
        self.collision_memberships
            .insert(recognizer_id.into(), membership);
        self
    }

    pub fn register_anchor_cue_bundle(
        mut self,
        locale: LocaleTag,
        anchor_key: impl Into<String>,
        names: Vec<String>,
        window_chars: Option<u16>,
    ) -> Self {
        self.anchor_resolver
            .register(locale, anchor_key, names, window_chars);
        self
    }

    /// Registers street words whose NER location span licenses an adjacent
    /// house number in `locale`.
    pub fn register_street_lexicon(
        mut self,
        locale: LocaleTag,
        order: StreetNumberOrder,
        names: Vec<String>,
    ) -> Self {
        self.street_lexicon.register(locale, order, names);
        self
    }

    /// Registers the address words of `locale` that let an address winner grow
    /// over the unit, box, state or post office written beside it.
    pub fn register_address_vocabulary(
        mut self,
        locale: LocaleTag,
        vocabulary: AddressVocabulary,
        names: Vec<String>,
    ) -> Self {
        self.address_grammar.register(locale, vocabulary, names);
        self
    }

    pub fn build(self) -> RecognizerRegistry {
        let recognizers_by_id = self
            .entries
            .iter()
            .map(|recognizer| (recognizer.id().to_string(), Arc::clone(recognizer)))
            .collect();
        RecognizerRegistry {
            entries: self.entries,
            recognizers_by_id,
            validators: HashMap::new(),
            canonicalizers: HashMap::new(),
            family_policy: FamilyPolicyTable::from_memberships(self.collision_memberships),
            anchor_resolver: self.anchor_resolver,
            street_lexicon: self.street_lexicon,
            address_grammar: self.address_grammar,
        }
    }
}
