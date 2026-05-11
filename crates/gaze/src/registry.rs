use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::anchor_resolver::{AnchorOutcome, AnchorResolver};
use crate::resolver::resolve_candidates_with_policy;
pub use gaze_types::{Candidate, DetectContext, Recognizer};
use gaze_types::{CollisionMembership, ConflictTier, LocaleChain, LocaleTag, PiiClass};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConflictTier, DictionaryBundle, LocaleTag, PiiClass};

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

        fn detect(&self, _input: &str, _ctx: &DetectContext<'_>) -> Vec<Candidate> {
            vec![Candidate::new(
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
            )]
        }

        fn token_family(&self) -> &str {
            "counter"
        }
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

        let candidates = registry.detect_all("input", &ctx);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].class, PiiClass::Email);
        assert_eq!(candidates[0].token_family, "counter");

        let (candidates, vetoed) = registry.detect_all_resolved("input", &ctx);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].class, PiiClass::Email);
        assert!(vetoed.is_empty());
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

            fn detect(&self, _input: &str, _ctx: &DetectContext<'_>) -> Vec<Candidate> {
                vec![Candidate::new(
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
                )]
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

        assert!(registry.detect_all("input", &ctx).is_empty());
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
}

impl RecognizerRegistry {
    pub fn builder() -> RecognizerRegistryBuilder {
        RecognizerRegistryBuilder::default()
    }

    pub fn detect_all(&self, input: &str, ctx: &DetectContext<'_>) -> Vec<Candidate> {
        self.entries
            .iter()
            .filter(|recognizer| {
                LocaleChain::from(ctx.locale_chain).intersects(recognizer.locales())
            })
            .flat_map(|recognizer| recognizer.detect(input, ctx))
            .collect()
    }

    pub fn detect_all_resolved(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
    ) -> (Vec<Candidate>, Vec<crate::validator_veto::VetoedCandidate>) {
        let classes = self
            .entries
            .iter()
            .map(|recognizer| recognizer.supported_class().clone())
            .collect::<BTreeSet<_>>();
        let mut candidates = Vec::new();

        for class in classes {
            for locale in ctx.locale_chain {
                let locale_ctx = DetectContext::new(std::slice::from_ref(locale), ctx.dictionaries);
                locale_ctx.degraded.set(ctx.degraded.get());
                let class_candidates = self
                    .entries
                    .iter()
                    .filter(|recognizer| recognizer.supported_class() == &class)
                    .filter(|recognizer| {
                        LocaleChain::from(locale_ctx.locale_chain).intersects(recognizer.locales())
                    })
                    .flat_map(|recognizer| recognizer.detect(input, &locale_ctx))
                    .filter(|candidate| candidate.score >= min_score(&class))
                    .collect::<Vec<_>>();
                if !class_candidates.is_empty() {
                    candidates.extend(class_candidates);
                    break;
                }
            }
        }

        let (candidates, vetoed) = crate::validator_veto::apply(candidates, self, input);
        let candidates = self.apply_anchor_resolution(candidates, input, ctx.locale_chain);
        (
            resolve_candidates_with_policy(candidates, self.family_policy()),
            vetoed,
        )
    }

    pub fn recognizer(&self, id: &str) -> Option<&Arc<dyn Recognizer>> {
        self.recognizers_by_id.get(id)
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

    fn apply_anchor_resolution(
        &self,
        candidates: Vec<Candidate>,
        input: &str,
        locale_chain: &[LocaleTag],
    ) -> Vec<Candidate> {
        candidates
            .into_iter()
            .map(|candidate| {
                match self.anchor_resolver.resolve(
                    &candidate,
                    input,
                    self.family_policy(),
                    locale_chain,
                ) {
                    AnchorOutcome::Found | AnchorOutcome::NotRequired => candidate,
                    AnchorOutcome::Missing { family, .. } => {
                        family_fallback_candidate(candidate, family, ConflictTier::AnchoredContext)
                    }
                }
            })
            .collect()
    }
}

fn family_fallback_candidate(
    candidate: Candidate,
    family: String,
    decided_by: ConflictTier,
) -> Candidate {
    let original_recognizer_id = candidate.recognizer_id.clone();
    Candidate::new(
        candidate.span,
        PiiClass::Custom(format!("family:{family}")),
        format!("collision-family:{family}"),
        candidate.score,
        candidate.priority,
        None,
        format!("collision-family:{family}"),
        candidate.source,
        decided_by,
        vec![original_recognizer_id],
    )
}

fn min_score(_class: &PiiClass) -> f32 {
    0.0
}

#[derive(Default)]
pub struct RecognizerRegistryBuilder {
    entries: Vec<Arc<dyn Recognizer>>,
    validators: HashMap<String, Arc<dyn Validator>>,
    canonicalizers: HashMap<String, Arc<dyn Canonicalizer>>,
    collision_memberships: HashMap<String, CollisionMembership>,
    anchor_resolver: AnchorResolver,
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

    pub fn build(self) -> RecognizerRegistry {
        let recognizers_by_id = self
            .entries
            .iter()
            .map(|recognizer| (recognizer.id().to_string(), Arc::clone(recognizer)))
            .collect();
        RecognizerRegistry {
            entries: self.entries,
            recognizers_by_id,
            validators: self.validators,
            canonicalizers: self.canonicalizers,
            family_policy: FamilyPolicyTable::from_memberships(self.collision_memberships),
            anchor_resolver: self.anchor_resolver,
        }
    }
}
