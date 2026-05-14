use http::Method;
use serde_json::Value;
use url::Url;

#[async_trait::async_trait]
pub trait ProviderAdapter: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn matches_path(&self, method: &Method, path: &str) -> bool;
    fn upstream_base(&self) -> &Url;
    fn request_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>>;
    fn response_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>>;
    fn sse_event_pii_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<PiiSurface<'a>>;
}

pub struct PiiSurface<'a> {
    pub field_path: String,
    pub text: &'a mut String,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: Value,
}

impl SseEvent {
    pub fn new(event: Option<String>, data: Value) -> Self {
        Self { event, data }
    }
}

pub(crate) fn push_string<'a>(
    surfaces: &mut Vec<PiiSurface<'a>>,
    field_path: impl Into<String>,
    value: &'a mut Value,
) {
    if let Value::String(text) = value {
        surfaces.push(PiiSurface {
            field_path: field_path.into(),
            text,
        });
    }
}

pub(crate) fn walk_all_strings<'a>(
    surfaces: &mut Vec<PiiSurface<'a>>,
    prefix: String,
    value: &'a mut Value,
) {
    match value {
        Value::String(text) => surfaces.push(PiiSurface {
            field_path: prefix,
            text,
        }),
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                walk_all_strings(surfaces, format!("{prefix}[{index}]"), item);
            }
        }
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                walk_all_strings(surfaces, path, child);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

pub(crate) fn push_text_blocks<'a>(
    surfaces: &mut Vec<PiiSurface<'a>>,
    prefix: &str,
    value: &'a mut Value,
) {
    match value {
        Value::String(text) => surfaces.push(PiiSurface {
            field_path: prefix.to_string(),
            text,
        }),
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                match item {
                    Value::String(text) => surfaces.push(PiiSurface {
                        field_path: format!("{prefix}[{index}]"),
                        text,
                    }),
                    Value::Object(map) => {
                        if matches!(map.get("type"), Some(Value::String(kind)) if kind == "text") {
                            if let Some(Value::String(text)) = map.get_mut("text") {
                                surfaces.push(PiiSurface {
                                    field_path: format!("{prefix}[{index}].text"),
                                    text,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}
