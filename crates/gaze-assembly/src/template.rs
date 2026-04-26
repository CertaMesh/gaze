use std::collections::HashMap;

use gaze::{PolicyError, RulepackError};

use crate::BuildError;

pub(crate) fn lower_regex_pattern(
    id: &str,
    pattern: Option<String>,
    pattern_template: Option<String>,
    locale_vocab: &HashMap<String, Vec<String>>,
) -> Result<String, BuildError> {
    match (pattern, pattern_template) {
        (Some(pattern), None) => Ok(pattern),
        (None, Some(template)) => lower_pattern_template(id, &template, locale_vocab),
        _ => Err(RulepackError::RegexPatternChoice { id: id.to_string() }.into()),
    }
}

pub(crate) fn lower_pattern_template(
    id: &str,
    template: &str,
    locale_vocab: &HashMap<String, Vec<String>>,
) -> Result<String, BuildError> {
    let mut lowered = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        lowered.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return Err(RulepackError::UnknownPatternTemplatePlaceholder {
                id: id.to_string(),
                placeholder: after.to_string(),
            }
            .into());
        };
        let placeholder = &after[..end];
        if !is_template_placeholder(placeholder) {
            lowered.push('{');
            lowered.push_str(placeholder);
            lowered.push('}');
            rest = &after[end + 1..];
            continue;
        }
        if let Some(bucket_name) = locale_bucket_name(placeholder) {
            let Some(names) = locale_vocab.get(bucket_name) else {
                return Err(PolicyError::UnknownLocaleBucket {
                    name: bucket_name.to_string(),
                }
                .into());
            };
            lowered.push_str(&format!(
                "(?:{})",
                names
                    .iter()
                    .map(|name| regex::escape(name))
                    .collect::<Vec<_>>()
                    .join("|")
            ));
        } else {
            return Err(RulepackError::UnknownPatternTemplatePlaceholder {
                id: id.to_string(),
                placeholder: placeholder.to_string(),
            }
            .into());
        }
        rest = &after[end + 1..];
    }
    lowered.push_str(rest);
    Ok(lowered)
}

fn is_template_placeholder(value: &str) -> bool {
    is_legacy_template_placeholder(value) || locale_bucket_name(value).is_some()
}

fn is_legacy_template_placeholder(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
}

fn locale_bucket_name(placeholder: &str) -> Option<&str> {
    const LEGACY_EMAIL_HEADERS_ALIAS: &str = "locale_email_headers";
    const LEGACY_EMAIL_HEADERS_BUCKET: &str = "email_headers";

    if placeholder == LEGACY_EMAIL_HEADERS_ALIAS {
        return Some(LEGACY_EMAIL_HEADERS_BUCKET);
    }

    let bucket_name = placeholder.strip_prefix("locale.")?;
    if !bucket_name.is_empty()
        && bucket_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        Some(bucket_name)
    } else {
        None
    }
}
