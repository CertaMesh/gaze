use crate::detector::PiiClass;
pub use gaze_types::Action;
use std::{any::Any, sync::Arc};

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
}

// None is unknown policy, distinct from a known rule's non-match.
pub(crate) fn preview(
    rules: &[RuleEntry],
    class: &PiiClass,
    context: &RuleContext,
) -> Option<Action> {
    for rule in rules {
        let description = rule.description.as_ref()?;
        if let Some(action) = description.action(class, context) {
            return Some(action);
        }
    }
    Some(Action::Preserve)
}
