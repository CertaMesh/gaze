//! `ResponseTranslator`: rewrite owner-side index snippets into the active session namespace.
//! Fails closed if any domain alias or raw value outside inserted session tokens would remain.

use std::collections::HashSet;

use crate::error::DenyReason;
use crate::model::{AgentSearchHit, IndexSearchHit};
use crate::session::RedactionSession;
use crate::traits::ResponseTranslator;
use crate::util::contains_domain_alias;

/// Translates owner-side hits into the active session namespace. Every domain alias is
/// replaced by a session token minted for the same `(class, raw_value)`; previously
/// seen entities reuse the existing session token, newly-discovered ones get a fresh
/// one. Fails closed if any index alias or raw value outside the tokens inserted by
/// this translation would survive into agent output. Pre-existing token literals
/// receive no exemption.
pub struct SessionResponseTranslator;

impl ResponseTranslator for SessionResponseTranslator {
    fn translate(
        session: &RedactionSession,
        hits: Vec<IndexSearchHit>,
    ) -> Result<Vec<AgentSearchHit>, DenyReason> {
        let mut translated = Vec::with_capacity(hits.len());
        for hit in hits {
            // Locate replacements in the original text: a token literal already present
            // in owner prose must never acquire replacement provenance.
            let mut aliases = HashSet::new();
            let mut occurrences = Vec::new();
            let mut mentioned = vec![false; hit.entities.len()];
            for (index, entity) in hit.entities.iter().enumerate() {
                if entity.domain_alias.is_empty() {
                    return Err(DenyReason::TranslatorFailed);
                }
                // Preserve first-entity-wins behavior for duplicate aliases.
                if !aliases.insert(entity.domain_alias.as_str()) {
                    continue;
                }
                for (start, _) in hit.snippet.char_indices() {
                    if hit.snippet[start..].starts_with(&entity.domain_alias) {
                        occurrences.push((start..start + entity.domain_alias.len(), index));
                        mentioned[index] = true;
                    }
                }
            }
            occurrences.sort_unstable_by_key(|(range, _)| range.start);
            if occurrences
                .windows(2)
                .any(|pair| pair[0].0.end > pair[1].0.start)
            {
                return Err(DenyReason::TranslatorFailed);
            }

            // Mint in entity order, independently of snippet order, once per entity.
            let mut tokens = vec![None; hit.entities.len()];
            for (index, entity) in hit.entities.iter().enumerate() {
                if mentioned[index] {
                    tokens[index] = Some(
                        session
                            .tokenize(&entity.class, &entity.raw_value)
                            .map_err(|_| DenyReason::TranslatorFailed)?,
                    );
                }
            }
            let mut snippet = String::with_capacity(hit.snippet.len());
            let mut inserted = Vec::with_capacity(occurrences.len());
            let mut cursor = 0;
            for (range, index) in occurrences {
                snippet.push_str(&hit.snippet[cursor..range.start]);
                let start = snippet.len();
                snippet.push_str(
                    tokens[index]
                        .as_deref()
                        .ok_or(DenyReason::TranslatorFailed)?,
                );
                inserted.push(start..snippet.len());
                cursor = range.end;
            }
            snippet.push_str(&hit.snippet[cursor..]);

            // Keep the alias guard over the entire output, including inserted bytes.
            if contains_domain_alias(&snippet) {
                return Err(DenyReason::TranslatorFailed);
            }
            for entity in &hit.entities {
                let raw = entity.raw_value.as_bytes();
                // Empty raw values remain a fail-closed input, as with str::contains.
                if raw.is_empty() {
                    return Err(DenyReason::TranslatorFailed);
                }
                // Byte windows include overlapping matches and never slice UTF-8 text.
                // A full raw occurrence must fit inside ONE inserted token range.
                for (start, window) in snippet.as_bytes().windows(raw.len()).enumerate() {
                    if window == raw
                        && !inserted
                            .iter()
                            .any(|range| range.start <= start && start + raw.len() <= range.end)
                    {
                        return Err(DenyReason::TranslatorFailed);
                    }
                }
            }

            translated.push(AgentSearchHit {
                doc_id: hit.doc_id,
                snippet,
            });
        }
        Ok(translated)
    }
}
