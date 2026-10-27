use serde_json::{json, Value};

pub const KEYS: [&str; 6] = [
    "customer_id",
    "customerId",
    "customer-id",
    "Kundennummer",
    "employee_id",
    "person_id",
];

// Main has no bundled record-ID rule. This adopter rule deliberately requires a label,
// matches both prose and whole JSON, and never recognizes an unlabelled number.
pub const RULEPACK: &str = r#"
schema_version = "0.1.0"
rulepack_id = "structured-record-id-probe"
rulepack_version = "0.1.0"
default_locales = ["global"]
[[recognizers]]
id = "adopter.labelled_record_id"
class = "custom:probe_record_id"
enabled = true
safety_tier = "safe_default"
locales = ["global"]
[recognizers.match]
kind = "regex"
pattern = '''(?i)\b(?:customer[_-]?id|Kundennummer|employee_id|person_id)["']?\s*:\s*["']?((?:[A-Z]-)?[0-9]{6})\b'''
capture_groups = [1]
"#;

#[derive(Debug)]
pub struct Case {
    pub key: &'static str,
    pub numeric: bool,
    pub depth: usize,
    pub array: bool,
}

impl Case {
    pub fn raw(&self) -> &'static str {
        if self.numeric {
            "731284"
        } else {
            "C-731284"
        }
    }

    pub fn document(&self) -> Value {
        let value = if self.numeric {
            json!(731284)
        } else {
            json!(self.raw())
        };
        let mut document = json!({self.key: value});
        for _ in 0..self.depth {
            document = json!({"record": document});
        }
        if self.array {
            json!({"items":[document]})
        } else {
            document
        }
    }

    pub fn prose(&self) -> String {
        format!("Please look up {}: {}.", self.key, self.raw())
    }
}

pub fn cases() -> impl Iterator<Item = Case> {
    KEYS.into_iter().flat_map(|key| {
        [false, true].into_iter().flat_map(move |numeric| {
            (0..=3).flat_map(move |depth| {
                [false, true].into_iter().map(move |array| Case {
                    key,
                    numeric,
                    depth,
                    array,
                })
            })
        })
    })
}

// Known failures execute the same protection oracle and require its failure. If a fix
// protects a case, this assertion fails so the case must be promoted to Protected.
pub fn check(case: &Case, surface: &str, captured: &str, known_failure: bool) {
    let protected = !without_tokens(captured).contains(case.raw());
    println!(
        "RECORD_ID {surface} {case:?} {} {captured}",
        if protected { "PROTECTED" } else { "LEAKS" }
    );
    assert_eq!(
        protected, !known_failure,
        "unexpected protection outcome on {surface} {case:?}: {captured}"
    );
}

pub fn without_tokens(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            out.push_str(&rest[start..]);
            return out;
        };
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}
