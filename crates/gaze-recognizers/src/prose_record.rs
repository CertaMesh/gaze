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
        let cue = Regex::new(&format!(r#"(?i)\b(?:{cue})(?:[\p{{Zs}}\t]*[.:=#]+[\p{{Zs}}\t]*|[\p{{Zs}}\t]+)(?:(?:is|as|ist|lautet)[\p{{Zs}}\t]+)?[`"'„“]?(?P<value>[\p{{L}}\p{{N}}][\p{{L}}\p{{N}}\p{{M}}]*(?:[\p{{Pd}}_/:.]+[\p{{L}}\p{{N}}][\p{{L}}\p{{N}}\p{{M}}]*)*)"#)).map_err(RecognizerError::InvalidRegex)?;
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
        let mut candidate = Candidate::new(
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
        );
        candidate.complete_record_id = true;
        candidate
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
    fn detect_is_locale_invariant(&self) -> bool {
        true
    }
    fn requires_prior_candidates(&self) -> bool {
        self.complete_joined
    }
    fn requires_validated_prior(&self) -> bool {
        self.complete_joined
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
            // Counters, numbered orders and file shard ordinals do not identify a
            // person by shape alone. An independent detector or personal cue still counts.
            static COUNTER: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(r"\A(?:\p{Lu}?\p{Ll}+_[0-9]{5}|(?i:ord(?:er)?)[-_][0-9]+)\z")
                    .expect("static counter regex")
            });
            static FILE_SHARD: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(r"\A[\p{L}/_-]+-[0-9]+-of-[0-9]+\z").expect("static shard name regex")
            });
            static LOGIN_OWNER: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(concat!(
                    r"(?ix)\b(?: (?:my|your|his|her|our|their|mein|dein|sein|ihr|unser)[\p{Zs}\t]+",
                    r"(?:login|username|benutzername)(?:[\p{Zs}\t]+(?:is|was|ist|lautet))?",
                    r"| (?:sign(?:ed)?|log(?:ged)?)[\p{Zs}\t]+in[\p{Zs}\t]+(?:as|with)",
                    r"| meld(?:e|et|en|ete|eten)[\p{Zs}\t]+(?:mich|dich|sich|uns|euch)[\p{Zs}\t]+(?:als|mit)",
                    r#")\b[\p{Zs}\t:=]+[`"'„“‘]?"#,
                ))
                    .expect("static personal login cue regex")
            });
            static LOGIN_USE: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(r#"(?i)\A[`"'”’“]?[\p{Zs}\t]+(?:to[\p{Zs}\t]+(?:sign|log)[\p{Zs}\t]+in|angemeldet|eingeloggt)\b"#)
                    .expect("static login purpose regex")
            });
            static ARTIFACT: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(r#"(?i)\b(?:release|build|version|katalognummer)\b(?:[\p{Zs}\t]+(?:tag|id|number|nr)\b)?(?:[\p{Zs}\t]+(?:is|ist|lautet)\b)?[\p{Zs}\t:=#`"'„“(\[]+"#)
                    .expect("static artifact label regex")
            });
            let mut owners = ARTIFACT.find_iter(input).peekable();
            let mut logins = LOGIN_OWNER.find_iter(input).peekable();
            let mut seeds: Vec<_> = ctx
                .prior_candidates
                .unwrap_or_default()
                .iter()
                .filter(|c| {
                    c.evidence == EvidenceKind::Rule
                        && !c.regex_guard_rejected
                        && c.validator_fail_reason.is_none()
                        && input
                            .get(c.span.clone())
                            .is_some_and(|s| s.bytes().any(|b| b.is_ascii_digit()))
                        && c.class.as_custom_name().is_some_and(|name| {
                            matches!(name, "postal_code" | "iban" | "credit_card")
                                || name.ends_with("_id")
                        })
                })
                .collect();
            seeds.sort_unstable_by_key(|c| c.span.start);
            let mut cues = seeds.iter().filter(|c| c.complete_record_id).peekable();
            let mut cue_end = 0;
            for token in JOINED.find_iter(input) {
                let value = token.as_str();
                if !value.chars().any(char::is_alphabetic) {
                    continue;
                }
                // An immediate artifact label owns the unqualified value, not a more
                // distant mention of a person. Independent detectors still see the text.
                while owners
                    .next_if(|label| label.end() < token.start())
                    .is_some()
                {}
                if owners
                    .next_if(|label| label.end() == token.start())
                    .is_some()
                {
                    continue;
                }
                let mut groups = value.split(|c: char| !c.is_alphanumeric());
                let prefix = groups.next().unwrap_or_default();
                if matches!(
                    prefix.to_ascii_uppercase().as_str(),
                    "ZIP" | "PLZ" | "POST" | "POSTAL" | "DE" | "US" | "D" | "SKU"
                ) {
                    continue;
                }
                // Underscores hide five-digit groups from the word-boundary ZIP regex.
                // Other shapes need independent evidence; length alone must not admit
                // unlabelled checksum-invalid card and bank account numbers.
                let zip_group = std::iter::once(prefix)
                    .chain(groups)
                    .any(|g| g.len() == 5 && g.bytes().all(|b| b.is_ascii_digit()));
                let first = seeds.partition_point(|c| c.span.start < token.start());
                let mut inside = seeds[first..]
                    .iter()
                    .take_while(|c| c.span.start < token.end());
                // Completion must not displace a recognizer that owns the entire ID,
                // even when its value contains an otherwise eligible five-digit group.
                if inside
                    .clone()
                    .any(|c| c.span == token.range() && !c.complete_record_id)
                {
                    continue;
                }
                // A five-digit postal seed is the same shape evidence as zip_group.
                // Keep the locale's own detection, but do not let it promote an
                // order counter or file shard into a person-linked identifier.
                while let Some(cue) = cues.next_if(|c| c.span.start <= token.start()) {
                    cue_end = cue_end.max(cue.span.end);
                }
                if !inside.any(|c| {
                    c.span.end <= token.end()
                        && !c.complete_record_id
                        && !(zip_group
                            && matches!(c.recognizer_id.as_str(), "postal.de" | "postal.us")
                            && c.span.len() == 5)
                }) {
                    if !zip_group {
                        continue;
                    }
                    let file_shard = input[token.end()..]
                        .strip_prefix('.')
                        .is_some_and(|s| s.starts_with(|c: char| c.is_ascii_alphabetic()))
                        && FILE_SHARD.is_match(value);
                    if cue_end < token.end() && (COUNTER.is_match(value) || file_shard) {
                        while logins.next_if(|m| m.end() < token.start()).is_some() {}
                        if logins.peek().is_none_or(|m| m.end() != token.start())
                            && !LOGIN_USE.is_match(&input[token.end()..])
                        {
                            continue;
                        }
                    }
                }
                matches.push(token.range());
            }
        }
        if matches.is_empty() {
            return Ok(Vec::new());
        }
        // Keep the whole claim even if a validated or caller-declared subvalue wins.
        // Effective-action inheritance needs both spans and their original classes.
        matches.sort_unstable_by_key(|r| (r.start, r.end));
        matches.dedup();
        Ok(matches
            .into_iter()
            .map(|span| self.candidate(span))
            .collect())
    }
}
