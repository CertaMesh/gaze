use crate::detector::PiiClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Tokenize,
    Redact,
    FormatPreserve,
    Generalize,
    Preserve,
}

#[derive(Debug, Clone, Default)]
pub struct Context {
    pub field_name: Option<String>,
}

pub trait Rule: Send + Sync {
    fn action(&self, class: &PiiClass, context: &Context) -> Option<Action>;
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
    fn action(&self, class: &PiiClass, _context: &Context) -> Option<Action> {
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
    fn action(&self, _class: &PiiClass, context: &Context) -> Option<Action> {
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
    fn action(&self, _class: &PiiClass, _context: &Context) -> Option<Action> {
        Some(self.action)
    }
}
