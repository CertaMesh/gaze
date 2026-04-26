use crate::detector::PiiClass;
pub use gaze_types::Action;

#[derive(Debug, Clone, Default)]
pub struct RuleContext {
    pub field_name: Option<String>,
}

pub trait Rule: Send + Sync {
    fn action(&self, class: &PiiClass, context: &RuleContext) -> Option<Action>;
}

pub struct ClassRule {
    class: PiiClass,
    action: Action,
}

impl ClassRule {
    pub fn new(class: PiiClass, action: Action) -> Self {
        Self { class, action }
    }
}

impl Rule for ClassRule {
    fn action(&self, class: &PiiClass, _context: &RuleContext) -> Option<Action> {
        (self.class == *class).then_some(self.action)
    }
}

pub struct ColumnRule {
    field_name: String,
    action: Action,
}

impl ColumnRule {
    pub fn new(field_name: &str, action: Action) -> Self {
        Self {
            field_name: field_name.to_string(),
            action,
        }
    }
}

impl Rule for ColumnRule {
    fn action(&self, _class: &PiiClass, context: &RuleContext) -> Option<Action> {
        (context.field_name.as_deref() == Some(self.field_name.as_str())).then_some(self.action)
    }
}

pub struct DefaultRule {
    action: Action,
}

impl DefaultRule {
    pub fn new(action: Action) -> Self {
        Self { action }
    }
}

impl Rule for DefaultRule {
    fn action(&self, _class: &PiiClass, _context: &RuleContext) -> Option<Action> {
        Some(self.action)
    }
}
