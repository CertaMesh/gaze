use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Template {
    pub id: String,
    pub context: String,
    pub locale_chain: Vec<String>,
    pub body: String,
    pub expected_classes: Vec<String>,
    pub notes: String,
}

pub fn phase_1_templates() -> serde_json::Result<Vec<Template>> {
    let raw = include_str!("prose-en/prose-en-001.json");
    serde_json::from_str(raw).map(|template| vec![template])
}
