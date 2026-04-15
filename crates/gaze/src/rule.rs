use crate::detector::PiiClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Tokenize,
    Preserve,
}

pub trait Rule: Send + Sync {
    fn action(&self, class: &PiiClass) -> Option<Action>;
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
    fn action(&self, class: &PiiClass) -> Option<Action> {
        (self.class == *class).then_some(self.action)
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
    fn action(&self, _class: &PiiClass) -> Option<Action> {
        Some(self.action)
    }
}
