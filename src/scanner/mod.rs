//! Layer 3 defense: a compiled-once regex scanner that runs on every log
//! line before it reaches the anonymizer. Replaces matches in-place with
//! `[REDACTED]`. Patterns come from `[policy.logs].strip_patterns`.
//!
//! This layer exists because log lines carry PII in unpredictable free-form
//! places. Column-rule replacement (layer 1) doesn't apply; Worka (layer 2)
//! catches the common cases; this layer is the user-configurable last mile.

use regex::Regex;

#[derive(Debug)]
pub struct Scanner {
    patterns: Vec<Regex>,
}

#[derive(Debug, thiserror::Error)]
#[error("invalid regex: {0}")]
pub struct ScannerError(#[from] regex::Error);

impl Scanner {
    pub fn compile(patterns: &[String]) -> Result<Self, ScannerError> {
        let compiled: Result<Vec<_>, _> = patterns.iter().map(|p| Regex::new(p)).collect();
        Ok(Self {
            patterns: compiled?,
        })
    }

    /// Apply every pattern in order, replacing matches with `[REDACTED]`.
    pub fn redact(&self, line: &str) -> String {
        let mut out = line.to_string();
        for re in &self.patterns {
            out = re.replace_all(&out, "[REDACTED]").into_owned();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_password_equals() {
        let s = Scanner::compile(&["password=[^ ]+".into()]).unwrap();
        assert_eq!(
            s.redact("auth failed password=hunter2 user=alice"),
            "auth failed [REDACTED] user=alice"
        );
    }

    #[test]
    fn strips_bearer_tokens() {
        let s = Scanner::compile(&["Bearer [A-Za-z0-9._-]+".into()]).unwrap();
        assert_eq!(
            s.redact("Authorization: Bearer abc.def-ghi"),
            "Authorization: [REDACTED]"
        );
    }

    #[test]
    fn invalid_regex_errors() {
        let err = Scanner::compile(&["(".into()]).unwrap_err();
        assert!(format!("{err}").contains("invalid regex"));
    }

    #[test]
    fn no_patterns_is_identity() {
        let s = Scanner::compile(&[]).unwrap();
        assert_eq!(s.redact("hello world"), "hello world");
    }
}
