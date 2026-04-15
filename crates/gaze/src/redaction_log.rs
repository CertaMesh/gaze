use crate::detector::PiiClass;
use crate::rule::Action;
use crate::Result;

pub trait RedactionLogger: Send + Sync {
    fn log(&self, entry: &RedactionEntry) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentKind {
    Structured,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionEntry {
    pub source: String,
    pub class: PiiClass,
    pub action: Action,
    pub field_name: Option<String>,
    pub document_kind: DocumentKind,
    pub conflict_loser: bool,
}
