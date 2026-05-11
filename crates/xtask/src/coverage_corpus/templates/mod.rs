use serde::Deserialize;
#[derive(Debug, Clone, Deserialize)]
pub struct Template {
    pub id: String,
    pub context: String,
    pub locale_chain: Vec<String>,
    pub body: String,
    pub expected_classes: Vec<String>,
    #[serde(default)]
    pub length_tier: TemplateLengthTier,
    pub notes: String,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateLengthTier {
    Snippet,
    Page,
    MultiPage,
}

impl TemplateLengthTier {
    pub fn new() -> Self {
        Self::Snippet
    }
}

impl Default for TemplateLengthTier {
    fn default() -> Self {
        Self::new()
    }
}

const TEMPLATE_JSON: &[&str] = &[
    include_str!("prose-en/prose-en-001.json"),
    include_str!("prose-en/prose-en-002.json"),
    include_str!("prose-en/prose-en-003.json"),
    include_str!("prose-en/prose-en-004.json"),
    include_str!("prose-en/prose-en-005.json"),
    include_str!("prose-en/prose-en-006.json"),
    include_str!("prose-en/prose-en-007.json"),
    include_str!("prose-en/prose-en-008.json"),
    include_str!("prose-en/prose-en-009.json"),
    include_str!("prose-en/prose-en-010.json"),
    include_str!("prose-de/prose-de-001.json"),
    include_str!("prose-de/prose-de-002.json"),
    include_str!("prose-de/prose-de-003.json"),
    include_str!("prose-de/prose-de-004.json"),
    include_str!("prose-de/prose-de-005.json"),
    include_str!("prose-de/prose-de-006.json"),
    include_str!("prose-de/prose-de-007.json"),
    include_str!("prose-de/prose-de-008.json"),
    include_str!("prose-de/prose-de-009.json"),
    include_str!("prose-de/prose-de-010.json"),
    include_str!("tool-call-json/tool-call-json-001.json"),
    include_str!("tool-call-json/tool-call-json-002.json"),
    include_str!("tool-call-json/tool-call-json-003.json"),
    include_str!("tool-call-json/tool-call-json-004.json"),
    include_str!("tool-call-json/tool-call-json-005.json"),
    include_str!("tool-call-json/tool-call-json-006.json"),
    include_str!("tool-call-json/tool-call-json-007.json"),
    include_str!("tool-call-json/tool-call-json-008.json"),
    include_str!("tool-call-json/tool-call-json-009.json"),
    include_str!("tool-call-json/tool-call-json-010.json"),
    include_str!("log-line/log-line-001.json"),
    include_str!("log-line/log-line-002.json"),
    include_str!("log-line/log-line-003.json"),
    include_str!("log-line/log-line-004.json"),
    include_str!("log-line/log-line-005.json"),
    include_str!("log-line/log-line-006.json"),
    include_str!("log-line/log-line-007.json"),
    include_str!("log-line/log-line-008.json"),
    include_str!("log-line/log-line-009.json"),
    include_str!("log-line/log-line-010.json"),
    include_str!("support-email-de/support-email-de-001.json"),
    include_str!("support-email-de/support-email-de-002.json"),
    include_str!("support-email-de/support-email-de-003.json"),
    include_str!("support-email-de/support-email-de-004.json"),
    include_str!("support-email-de/support-email-de-005.json"),
    include_str!("support-email-de/support-email-de-006.json"),
    include_str!("support-email-de/support-email-de-007.json"),
    include_str!("support-email-de/support-email-de-008.json"),
    include_str!("support-email-de/support-email-de-009.json"),
    include_str!("support-email-de/support-email-de-010.json"),
    include_str!("ocr-output/ocr-output-001.json"),
    include_str!("ocr-output/ocr-output-002.json"),
    include_str!("ocr-output/ocr-output-003.json"),
    include_str!("ocr-output/ocr-output-004.json"),
    include_str!("ocr-output/ocr-output-005.json"),
    include_str!("ocr-output/ocr-output-006.json"),
    include_str!("ocr-output/ocr-output-007.json"),
    include_str!("ocr-output/ocr-output-008.json"),
    include_str!("ocr-output/ocr-output-009.json"),
    include_str!("ocr-output/ocr-output-010.json"),
    include_str!("conversational/conversational-001.json"),
    include_str!("conversational/conversational-002.json"),
    include_str!("conversational/conversational-003.json"),
    include_str!("conversational/conversational-004.json"),
    include_str!("conversational/conversational-005.json"),
    include_str!("conversational/conversational-006.json"),
    include_str!("code-mixed/code-mixed-001.json"),
    include_str!("code-mixed/code-mixed-002.json"),
    include_str!("code-mixed/code-mixed-003.json"),
    include_str!("code-mixed/code-mixed-004.json"),
    include_str!("code-mixed/code-mixed-005.json"),
    include_str!("code-mixed/code-mixed-006.json"),
    include_str!("code-mixed/code-mixed-007.json"),
    include_str!("code-mixed/code-mixed-008.json"),
];

pub fn phase_1_templates() -> serde_json::Result<Vec<Template>> {
    TEMPLATE_JSON
        .iter()
        .map(|raw| serde_json::from_str(raw))
        .collect()
}
