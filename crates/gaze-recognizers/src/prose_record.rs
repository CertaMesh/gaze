//! Record identifiers in free text. Structured scalars belong to `field_label`.
use std::{ops::Range, sync::LazyLock};

use gaze_types::{
    Candidate, ConflictTier, DetectContext, DetectError, EvidenceKind, FieldContext, FieldEvidence,
    LocaleBasis, LocaleTag, PiiClass, Recognizer,
};
use regex::Regex;

use crate::RecognizerError;

pub struct ProseRecordRecognizer {
    id: String,
    class: PiiClass,
    cue: Regex,
    complete_joined: bool,
    score: f32,
    priority: i32,
    family: String,
    locales: Vec<LocaleTag>,
    basis: LocaleBasis,
}

impl ProseRecordRecognizer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        class: PiiClass,
        cue: &str,
        complete_joined: bool,
        score: f32,
        priority: i32,
        family: String,
        locales: Vec<LocaleTag>,
        basis: LocaleBasis,
    ) -> crate::Result<Self> {
        // A connector is mandatory (including the period in `Fall-Nr.123`).
        // The cue owns one value on its own line, not the next word containing a digit.
        let cue = Regex::new(&format!(r#"(?i)\b(?:{cue})(?:[\p{{Zs}}\t]*[.:=#]+[\p{{Zs}}\t]*|[\p{{Zs}}\t]+(?:(?:is|as|ist|lautet)[\p{{Zs}}\t]+)?)["'„“]?(?P<value>[\p{{L}}\p{{N}}][\p{{L}}\p{{N}}\p{{M}}]*(?:[\p{{Pd}}_/:.][\p{{L}}\p{{N}}][\p{{L}}\p{{N}}\p{{M}}]*)*)"#)).map_err(RecognizerError::InvalidRegex)?;
        Ok(Self {
            id,
            class,
            cue,
            complete_joined,
            score,
            priority,
            family,
            locales,
            basis,
        })
    }

    fn candidate(&self, span: Range<usize>) -> Candidate {
        Candidate::new(
            span,
            self.class.clone(),
            self.id.clone(),
            self.score,
            self.priority,
            None,
            self.family.clone(),
            "prose_record_id",
            ConflictTier::None,
            Vec::new(),
        )
    }
}

impl Recognizer for ProseRecordRecognizer {
    fn id(&self) -> &str {
        &self.id
    }
    fn supported_class(&self) -> &PiiClass {
        &self.class
    }
    fn evidence(&self) -> EvidenceKind {
        EvidenceKind::Rule
    }
    fn locales(&self) -> &[LocaleTag] {
        &self.locales
    }
    fn locale_basis(&self) -> LocaleBasis {
        self.basis
    }
    fn token_family(&self) -> &str {
        &self.family
    }
    fn requires_prior_candidates(&self) -> bool {
        true
    }

    fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        // A shape does not establish the meaning of a tool selector. Never infer a
        // namespace from a key name; leave structured carriers to their declared rules.
        if ctx.structured_carrier
            || matches!(ctx.fields, Some(FieldContext::WholeInput(f) | FieldContext::Fragment { field: f, .. }) if f.evidence == FieldEvidence::ValidatedParse)
            || matches!(ctx.fields, Some(FieldContext::Spans(fields)) if fields.iter().any(|(_, f)| f.evidence == FieldEvidence::ValidatedParse))
        {
            return Ok(Vec::new());
        }
        let mut matches = Vec::new();
        for capture in self.cue.captures_iter(input) {
            let value = capture.name("value").expect("named capture");
            if value.as_str().bytes().any(|b| b.is_ascii_digit()) {
                matches.push(value.range());
            }
        }
        if self.complete_joined {
            static JOINED: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(r"\b[\p{L}\p{N}]+(?:[\p{Pd}_/][\p{L}\p{N}]+)+\b")
                    .expect("static compound token regex")
            });
            let mut seeds: Vec<_> = ctx
                .prior_candidates
                .unwrap_or_default()
                .iter()
                .filter(|c| {
                    !c.regex_guard_rejected
                        && c.validator_fail_reason.is_none()
                        && input
                            .get(c.span.clone())
                            .is_some_and(|s| s.bytes().any(|b| b.is_ascii_digit()))
                        && c.class.as_custom_name().is_some_and(|name| {
                            matches!(name, "postal_code" | "iban" | "credit_card")
                                || name.ends_with("_id")
                        })
                })
                .map(|c| &c.span)
                .collect();
            seeds.sort_unstable_by_key(|r| r.start);
            for token in JOINED.find_iter(input) {
                let value = token.as_str();
                if !value.chars().any(char::is_alphabetic) {
                    continue;
                }
                // An immediate artifact label owns the unqualified value, not a more
                // distant mention of a person. Independent detectors still see the text.
                let owner = input[..token.start()]
                    .trim_end_matches([' ', '\t', ':', '#'])
                    .rsplit(|c: char| !c.is_alphanumeric())
                    .next()
                    .unwrap_or_default();
                if matches!(
                    owner.to_ascii_lowercase().as_str(),
                    "release" | "build" | "version" | "katalognummer"
                ) {
                    continue;
                }
                let prefix = value.split(['-', '_', '/']).next().unwrap_or_default();
                if matches!(
                    prefix.to_ascii_uppercase().as_str(),
                    "ZIP" | "PLZ" | "POST" | "POSTAL" | "DE" | "US" | "D" | "SKU"
                ) {
                    continue;
                }
                // Underscores hide five-digit groups from the word-boundary ZIP regex.
                // Other shapes need independent evidence; length alone must not admit
                // unlabelled checksum-invalid card and bank account numbers.
                let zip_group = value
                    .split(['-', '_', '/'])
                    .any(|g| g.len() == 5 && g.bytes().all(|b| b.is_ascii_digit()));
                let first = seeds.partition_point(|r| r.start < token.start());
                if zip_group
                    || seeds[first..]
                        .iter()
                        .take_while(|r| r.start < token.end())
                        .any(|r| r.end <= token.end())
                {
                    matches.push(token.range());
                }
            }
        }
        if matches.is_empty() {
            return Ok(Vec::new());
        }
        // Leave validated subvalues to their own policies: e.g. a PAN configured for
        // irreversible redaction cannot become a reversible generic record ID. Cover
        // only the remaining bytes. Merge ranges once rather than scanning the pool per token.
        let mut occupied: Vec<_> = ctx
            .prior_candidates
            .unwrap_or_default()
            .iter()
            .filter(|c| {
                !c.regex_guard_rejected
                    && ((c.checksum_validated() && c.class.as_custom_name() != Some("postal_code"))
                        || c.field
                            .as_ref()
                            .is_some_and(|f| f.evidence == FieldEvidence::CallerDeclared))
            })
            .map(|c| c.span.clone())
            .collect();
        occupied.sort_unstable_by_key(|r| r.start);
        let mut disjoint: Vec<Range<usize>> = Vec::new();
        for r in occupied {
            if let Some(last) = disjoint.last_mut().filter(|l| r.start <= l.end) {
                last.end = last.end.max(r.end);
            } else {
                disjoint.push(r);
            }
        }
        matches.sort_unstable_by_key(|r| (r.start, r.end));
        matches.dedup();
        let mut found = Vec::new();
        for range in matches {
            let mut cursor = range.start;
            let i = disjoint.partition_point(|r| r.end <= cursor);
            for r in disjoint[i..].iter().take_while(|r| r.start < range.end) {
                if cursor < r.start {
                    found.push(self.candidate(cursor..r.start));
                }
                cursor = cursor.max(r.end);
            }
            if cursor < range.end {
                found.push(self.candidate(cursor..range.end));
            }
        }
        Ok(found)
    }
}
