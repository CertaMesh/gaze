use crate::detector::PiiClass;
pub use gaze_types::Action;
use gaze_types::DerivedFamilyAction;
use std::{any::Any, convert::Infallible, sync::Arc};

#[derive(Debug, Clone, Default)]
pub struct RuleContext {
    pub field_name: Option<String>,
}

pub trait Rule: Send + Sync {
    fn action(&self, class: &PiiClass, context: &RuleContext) -> Option<Action>;
}

#[derive(Clone)]
enum StaticRule {
    Class(PiiClass, Action),
    Column(String, Action),
    Default(Action),
}
impl StaticRule {
    fn action(&self, class: &PiiClass, context: &RuleContext) -> Option<Action> {
        match self {
            Self::Class(expected, action) => (expected == class).then_some(*action),
            Self::Column(field, action) => {
                (context.field_name.as_deref() == Some(field.as_str())).then_some(*action)
            }
            Self::Default(action) => Some(*action),
        }
    }
}

pub struct ClassRule {
    description: StaticRule,
}
impl ClassRule {
    pub fn new(class: PiiClass, action: Action) -> Self {
        Self {
            description: StaticRule::Class(class, action),
        }
    }
}
impl Rule for ClassRule {
    fn action(&self, class: &PiiClass, context: &RuleContext) -> Option<Action> {
        self.description.action(class, context)
    }
}
pub struct ColumnRule {
    description: StaticRule,
}
impl ColumnRule {
    pub fn new(field_name: &str, action: Action) -> Self {
        Self {
            description: StaticRule::Column(field_name.to_string(), action),
        }
    }
}
impl Rule for ColumnRule {
    fn action(&self, class: &PiiClass, context: &RuleContext) -> Option<Action> {
        self.description.action(class, context)
    }
}
pub struct DefaultRule {
    description: StaticRule,
}
impl DefaultRule {
    pub fn new(action: Action) -> Self {
        Self {
            description: StaticRule::Default(action),
        }
    }
}
impl Rule for DefaultRule {
    fn action(&self, class: &PiiClass, context: &RuleContext) -> Option<Action> {
        self.description.action(class, context)
    }
}

#[derive(Clone)]
pub(crate) struct RuleEntry {
    runtime: Arc<dyn Rule>,
    description: Option<StaticRule>,
}
impl RuleEntry {
    pub(crate) fn new<R: Rule + 'static>(rule: R) -> Self {
        // Capture only exact immutable built-ins before erasure. Wrappers stay unknown.
        let value = &rule as &dyn Any;
        let description = value
            .downcast_ref::<ClassRule>()
            .map(|r| &r.description)
            .or_else(|| value.downcast_ref::<ColumnRule>().map(|r| &r.description))
            .or_else(|| value.downcast_ref::<DefaultRule>().map(|r| &r.description))
            .cloned();
        Self {
            runtime: Arc::new(rule),
            description,
        }
    }
    #[cfg(test)]
    pub(crate) fn inject_runtime(&mut self, runtime: impl Rule + 'static) {
        self.runtime = Arc::new(runtime);
    }

    pub(crate) fn action(&self, class: &PiiClass, context: &RuleContext) -> Option<Action> {
        self.runtime.action(class, context)
    }

    /// The built-in catch-all rule. Adopter-defined `Rule` impls are never
    /// treated as the default: when one answers for a class it counts as an
    /// explicit rule.
    fn is_default(&self) -> bool {
        matches!(self.description, Some(StaticRule::Default(_)))
    }
}

/// What the rule chain says for one class: first match wins, a `Default`
/// rule matches unconditionally, and no match at all preserves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FirstMatch {
    Explicit(Action),
    Default(Action),
    NoRule,
}

impl FirstMatch {
    fn action(self) -> Action {
        match self {
            Self::Explicit(action) | Self::Default(action) => action,
            Self::NoRule => Action::Preserve,
        }
    }
}

/// Walks the chain with `probe`, which answers `Err` when a rule's verdict
/// cannot be read (the static preview reaching an adopter-defined rule).
fn first_match<E>(
    rules: &[RuleEntry],
    probe: &impl Fn(&RuleEntry, &PiiClass) -> Result<Option<Action>, E>,
    class: &PiiClass,
) -> Result<FirstMatch, E> {
    for rule in rules {
        if let Some(action) = probe(rule, class)? {
            return Ok(if rule.is_default() {
                FirstMatch::Default(action)
            } else {
                FirstMatch::Explicit(action)
            });
        }
    }
    Ok(FirstMatch::NoRule)
}

/// A policy action together with how it was chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedAction {
    pub(crate) action: Action,
    /// Set when `action` belongs to a collision-family token that no rule
    /// named explicitly and was derived from the family's member classes.
    pub(crate) derived: Option<DerivedFamilyAction>,
}

/// The one action lookup for every span the pipeline acts on or logs.
///
/// First match wins for every class. A collision-family token
/// (`custom:family:<name>`) whose first match is the `Default` rule (or no
/// rule) does not take that verdict as-is: it takes the strictest action,
/// under [`Action::strictness_rank`], among its member classes' resolved
/// actions and that default. That keeps a policy which names only
/// `custom:iban` and `custom:credit_card` fail-closed on the family token
/// without ever landing laxer than the default the family would have taken.
/// An explicit rule for the family class, wherever a first-match walk reaches
/// it, wins unchanged. A member that is exactly as strict as the default is
/// credited in the audit record; ties between members go to the lowest class
/// in `PiiClass` order.
fn resolve_with<E>(
    rules: &[RuleEntry],
    probe: &impl Fn(&RuleEntry, &PiiClass) -> Result<Option<Action>, E>,
    class: &PiiClass,
    members: impl FnOnce(&str) -> Vec<PiiClass>,
) -> Result<ResolvedAction, E> {
    let own = first_match(rules, probe, class)?;
    let family = match (own, class.as_family_name()) {
        (FirstMatch::Explicit(_), _) | (_, None) => {
            return Ok(ResolvedAction {
                action: own.action(),
                derived: None,
            });
        }
        (_, Some(family)) => family,
    };
    let mut strictest = own.action();
    let mut member_class = None;
    for member in members(family) {
        let action = first_match(rules, probe, &member)?.action();
        let rank = action.strictness_rank();
        if rank > strictest.strictness_rank()
            || (member_class.is_none() && rank == strictest.strictness_rank())
        {
            strictest = action;
            member_class = Some(member);
        }
    }
    Ok(ResolvedAction {
        action: strictest,
        derived: Some(DerivedFamilyAction::new(strictest, member_class)),
    })
}

/// Runtime lookup: every rule runs, so the walk is total.
pub(crate) fn resolve(
    rules: &[RuleEntry],
    class: &PiiClass,
    context: &RuleContext,
    members: impl FnOnce(&str) -> Vec<PiiClass>,
) -> ResolvedAction {
    let probe = |rule: &RuleEntry, class: &PiiClass| -> Result<Option<Action>, Infallible> {
        Ok(rule.action(class, context))
    };
    match resolve_with(rules, &probe, class, members) {
        Ok(resolved) => resolved,
        Err(never) => match never {},
    }
}

// None is unknown policy, distinct from a known rule's non-match.
pub(crate) fn preview(
    rules: &[RuleEntry],
    class: &PiiClass,
    context: &RuleContext,
    members: impl FnOnce(&str) -> Vec<PiiClass>,
) -> Option<Action> {
    let probe = |rule: &RuleEntry, class: &PiiClass| -> Result<Option<Action>, ()> {
        rule.description
            .as_ref()
            .map(|description| description.action(class, context))
            .ok_or(())
    };
    resolve_with(rules, &probe, class, members)
        .ok()
        .map(|resolved| resolved.action)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(name: &str) -> PiiClass {
        PiiClass::custom(name).expect("custom class")
    }

    fn family() -> PiiClass {
        PiiClass::family("payment-card-or-iban")
    }

    fn members(name: &str) -> Vec<PiiClass> {
        assert_eq!(name, "payment-card-or-iban");
        vec![custom("credit_card"), custom("iban")]
    }

    fn chain(rules: Vec<RuleEntry>, class: &PiiClass) -> ResolvedAction {
        resolve(&rules, class, &RuleContext::default(), members)
    }

    fn class_rule(name: &str, action: Action) -> RuleEntry {
        RuleEntry::new(ClassRule::new(custom(name), action))
    }

    #[test]
    fn strictness_order_is_redact_tokenize_generalize_format_preserve_preserve() {
        let order = [
            Action::Redact,
            Action::Tokenize,
            Action::Generalize,
            Action::FormatPreserve,
            Action::Preserve,
        ];
        for pair in order.windows(2) {
            assert!(
                pair[0].strictness_rank() > pair[1].strictness_rank(),
                "{:?} must rank above {:?}",
                pair[0],
                pair[1]
            );
            assert_eq!(pair[1].strictest(pair[0]), pair[0]);
        }
    }

    #[test]
    fn family_without_an_explicit_rule_takes_the_strictest_member_action() {
        let resolved = chain(
            vec![
                class_rule("iban", Action::Tokenize),
                class_rule("credit_card", Action::Preserve),
                RuleEntry::new(DefaultRule::new(Action::Preserve)),
            ],
            &family(),
        );

        assert_eq!(resolved.action, Action::Tokenize);
        assert_eq!(
            resolved.derived,
            Some(DerivedFamilyAction::new(
                Action::Tokenize,
                Some(custom("iban"))
            ))
        );
    }

    /// Ruling 3746 #1: members disagreeing between two protective actions take
    /// the stricter one, in either declaration order.
    #[test]
    fn members_disagreeing_between_protective_actions_take_the_stricter() {
        for rules in [
            vec![
                class_rule("iban", Action::Tokenize),
                class_rule("credit_card", Action::Redact),
                RuleEntry::new(DefaultRule::new(Action::Preserve)),
            ],
            vec![
                class_rule("credit_card", Action::Redact),
                class_rule("iban", Action::Tokenize),
                RuleEntry::new(DefaultRule::new(Action::Preserve)),
            ],
        ] {
            let resolved = chain(rules, &family());
            assert_eq!(resolved.action, Action::Redact);
            assert_eq!(
                resolved.derived.and_then(|derived| derived.member_class),
                Some(custom("credit_card"))
            );
        }
    }

    #[test]
    fn all_members_preserve_under_a_preserve_default_preserves() {
        let resolved = chain(
            vec![
                class_rule("iban", Action::Preserve),
                class_rule("credit_card", Action::Preserve),
                RuleEntry::new(DefaultRule::new(Action::Preserve)),
            ],
            &family(),
        );

        assert_eq!(resolved.action, Action::Preserve);
        assert!(resolved.derived.is_some());
    }

    /// Ruling 3746 #2: the derivation never lands below the family's own
    /// default, so an all-preserve policy under a tokenize default stays tokenized.
    #[test]
    fn family_never_lands_below_its_own_default() {
        let resolved = chain(
            vec![
                class_rule("iban", Action::Preserve),
                class_rule("credit_card", Action::Preserve),
                RuleEntry::new(DefaultRule::new(Action::Tokenize)),
            ],
            &family(),
        );

        assert_eq!(resolved.action, Action::Tokenize);
        assert_eq!(
            resolved.derived,
            Some(DerivedFamilyAction::new(Action::Tokenize, None))
        );
    }

    #[test]
    fn a_member_as_strict_as_the_default_is_credited() {
        let resolved = chain(
            vec![
                class_rule("iban", Action::Tokenize),
                RuleEntry::new(DefaultRule::new(Action::Tokenize)),
            ],
            &family(),
        );

        assert_eq!(
            resolved.derived.and_then(|derived| derived.member_class),
            Some(custom("credit_card")),
            "the first member in class order at the default's strictness is credited"
        );
    }

    #[test]
    fn an_explicit_family_rule_wins_over_the_members() {
        let resolved = chain(
            vec![
                RuleEntry::new(ClassRule::new(family(), Action::Preserve)),
                class_rule("iban", Action::Redact),
                RuleEntry::new(DefaultRule::new(Action::Tokenize)),
            ],
            &family(),
        );

        assert_eq!(resolved.action, Action::Preserve);
        assert_eq!(resolved.derived, None);
    }

    #[test]
    fn a_family_rule_after_the_default_stays_dead() {
        let resolved = chain(
            vec![
                RuleEntry::new(DefaultRule::new(Action::Preserve)),
                RuleEntry::new(ClassRule::new(family(), Action::Tokenize)),
            ],
            &family(),
        );

        assert_eq!(resolved.action, Action::Preserve);
        assert!(resolved.derived.is_some(), "first match wins is unchanged");
    }

    #[test]
    fn a_column_rule_reaching_the_family_first_is_explicit() {
        let context = RuleContext {
            field_name: Some("notes".to_string()),
        };
        let rules = vec![
            RuleEntry::new(ColumnRule::new("notes", Action::Preserve)),
            class_rule("iban", Action::Tokenize),
            RuleEntry::new(DefaultRule::new(Action::Preserve)),
        ];

        let resolved = resolve(&rules, &family(), &context, members);

        assert_eq!(resolved.action, Action::Preserve);
        assert_eq!(resolved.derived, None);
    }

    #[test]
    fn a_non_family_class_never_consults_members() {
        let resolved = resolve(
            &[
                class_rule("iban", Action::Redact),
                RuleEntry::new(DefaultRule::new(Action::Preserve)),
            ],
            &custom("credit_card"),
            &RuleContext::default(),
            |_| panic!("members must not be consulted for a member class"),
        );

        assert_eq!(resolved.action, Action::Preserve);
        assert_eq!(resolved.derived, None);
    }

    #[test]
    fn an_empty_chain_preserves_the_family_too() {
        let resolved = resolve(&[], &family(), &RuleContext::default(), members);

        assert_eq!(resolved.action, Action::Preserve);
    }

    struct Opaque(Action);
    impl Rule for Opaque {
        fn action(&self, class: &PiiClass, _: &RuleContext) -> Option<Action> {
            (class == &PiiClass::custom("iban").expect("class")).then_some(self.0)
        }
    }

    #[test]
    fn static_preview_derives_like_the_runtime_walk() {
        let rules = vec![
            class_rule("iban", Action::Tokenize),
            RuleEntry::new(DefaultRule::new(Action::Preserve)),
        ];

        assert_eq!(
            preview(&rules, &family(), &RuleContext::default(), members),
            Some(Action::Tokenize)
        );
    }

    #[test]
    fn static_preview_is_unknown_when_an_opaque_rule_decides_a_member() {
        let rules = vec![
            RuleEntry::new(Opaque(Action::Tokenize)),
            RuleEntry::new(DefaultRule::new(Action::Preserve)),
        ];

        assert_eq!(
            preview(&rules, &family(), &RuleContext::default(), members),
            None
        );
        // The runtime walk runs the opaque rule and derives from it.
        assert_eq!(chain(rules, &family()).action, Action::Tokenize);
    }
}
