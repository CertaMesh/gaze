use std::collections::HashMap;

use crate::{Candidate, FamilyPolicyTable, LocaleTag};

const DEFAULT_WINDOW_CHARS: usize = 64;

#[derive(Debug, Clone, Default)]
pub(crate) struct AnchorResolver {
    cues: HashMap<LocaleTag, HashMap<String, AnchorCueBundle>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnchorCueBundle {
    names: Vec<String>,
    window_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AnchorOutcome {
    Found,
    Missing { family: String, anchor_key: String },
    NotRequired,
}

impl AnchorResolver {
    pub(crate) fn register(
        &mut self,
        locale: LocaleTag,
        anchor_key: impl Into<String>,
        names: Vec<String>,
        window_chars: Option<u16>,
    ) {
        let mut names = names
            .into_iter()
            .filter_map(|name| {
                let trimmed = name.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_ascii_lowercase())
            })
            .collect::<Vec<_>>();
        names.sort_by_key(|name| std::cmp::Reverse(name.len()));
        names.dedup();
        if names.is_empty() {
            return;
        }

        self.cues.entry(locale).or_default().insert(
            anchor_key.into(),
            AnchorCueBundle {
                names,
                window_chars: window_chars
                    .map(usize::from)
                    .unwrap_or(DEFAULT_WINDOW_CHARS),
            },
        );
    }

    pub(crate) fn resolve(
        &self,
        candidate: &Candidate,
        input: &str,
        policy: &FamilyPolicyTable,
        locale_chain: &[LocaleTag],
    ) -> AnchorOutcome {
        let Some(membership) = policy.membership(&candidate.recognizer_id) else {
            return AnchorOutcome::NotRequired;
        };
        let Some(anchor_key) = membership.mandatory_anchor.as_deref() else {
            return AnchorOutcome::NotRequired;
        };

        for locale in locale_chain {
            let Some(bundle) = self
                .cues
                .get(locale)
                .and_then(|by_key| by_key.get(anchor_key))
            else {
                continue;
            };
            if bundle.matches(candidate, input) {
                return AnchorOutcome::Found;
            }
        }

        AnchorOutcome::Missing {
            family: membership.family.clone(),
            anchor_key: anchor_key.to_string(),
        }
    }
}

impl AnchorCueBundle {
    fn matches(&self, candidate: &Candidate, input: &str) -> bool {
        if candidate.span.start > candidate.span.end || candidate.span.end > input.len() {
            return false;
        }
        let before_start =
            byte_index_n_chars_before(input, candidate.span.start, self.window_chars);
        let after_end = byte_index_n_chars_after(input, candidate.span.end, self.window_chars);
        let window = input[before_start..after_end].to_ascii_lowercase();
        self.names
            .iter()
            .any(|cue| contains_cue_with_boundary(&window, cue))
    }
}

fn byte_index_n_chars_before(input: &str, end: usize, count: usize) -> usize {
    input[..end]
        .char_indices()
        .rev()
        .nth(count.saturating_sub(1))
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn byte_index_n_chars_after(input: &str, start: usize, count: usize) -> usize {
    input[start..]
        .char_indices()
        .nth(count)
        .map(|(index, _)| start + index)
        .unwrap_or(input.len())
}

fn contains_cue_with_boundary(window: &str, cue: &str) -> bool {
    let mut offset = 0;
    while let Some(relative) = window[offset..].find(cue) {
        let start = offset + relative;
        let end = start + cue.len();
        if is_boundary(window[..start].chars().next_back())
            && is_boundary(window[end..].chars().next())
        {
            return true;
        }
        offset = end;
    }
    false
}

fn is_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| !ch.is_alphanumeric() && ch != '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze_types::{CollisionMembership, ConflictTier, PiiClass};

    #[test]
    fn resolves_found_missing_and_not_required() {
        let policy = crate::registry::FamilyPolicyTable::from_memberships(HashMap::from([(
            "iban.structural".to_string(),
            CollisionMembership::new("payment-card-or-iban", "iban", 10, Some("iban".to_string())),
        )]));
        let candidate = Candidate::new(
            5..24,
            PiiClass::custom("iban"),
            "iban.structural",
            0.9,
            80,
            None,
            "counter",
            "iban.structural",
            ConflictTier::None,
            Vec::new(),
        );
        let mut resolver = AnchorResolver::default();

        assert_eq!(
            resolver.resolve(
                &candidate,
                "IBAN DE70 8807 9565 3194",
                &policy,
                &[LocaleTag::EnUs]
            ),
            AnchorOutcome::Missing {
                family: "payment-card-or-iban".to_string(),
                anchor_key: "iban".to_string()
            }
        );

        resolver.register(LocaleTag::EnUs, "iban", vec!["IBAN".to_string()], None);
        assert_eq!(
            resolver.resolve(
                &candidate,
                "IBAN DE70 8807 9565 3194",
                &policy,
                &[LocaleTag::EnUs]
            ),
            AnchorOutcome::Found
        );

        let untracked = Candidate::new(
            0..4,
            PiiClass::Email,
            "email.global",
            0.9,
            90,
            None,
            "counter",
            "email.global",
            ConflictTier::None,
            Vec::new(),
        );
        assert_eq!(
            resolver.resolve(&untracked, "test", &policy, &[LocaleTag::EnUs]),
            AnchorOutcome::NotRequired
        );
    }
}
