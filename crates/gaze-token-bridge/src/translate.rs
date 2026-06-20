//! Track C (gated) — `ResponseTranslator`: rewrite owner-side index snippets into the
//! active session namespace (mint fresh session tokens for newly-discovered entities),
//! fail closed if any domain alias or raw value would remain. Owner: sub-orchestrator C.
//! Contract: `crate::traits::ResponseTranslator`, `crate::util::contains_domain_alias`.
//! Reference: spike `ResponseTranslator`.

use crate::error::DenyReason;
use crate::model::{AgentSearchHit, IndexSearchHit};
use crate::session::RedactionSession;
use crate::traits::ResponseTranslator;
use crate::util::contains_domain_alias;

/// Translates owner-side hits into the active session namespace. Every domain alias is
/// replaced by a session token minted for the same `(class, raw_value)`; previously
/// seen entities reuse the existing session token, newly-discovered ones get a fresh
/// one. Fails closed if any index alias or raw value would survive into agent output.
pub struct SessionResponseTranslator;

impl ResponseTranslator for SessionResponseTranslator {
    fn translate(
        session: &RedactionSession,
        hits: Vec<IndexSearchHit>,
    ) -> Result<Vec<AgentSearchHit>, DenyReason> {
        let mut translated = Vec::with_capacity(hits.len());
        for hit in hits {
            let mut snippet = hit.snippet;
            for entity in &hit.entities {
                if snippet.contains(&entity.domain_alias) {
                    let token = session
                        .tokenize(&entity.class, &entity.raw_value)
                        .map_err(|_| DenyReason::TranslatorFailed)?;
                    snippet = snippet.replace(&entity.domain_alias, &token);
                }
            }

            // Fail closed: no index-domain alias may remain (translator leak guard) ...
            if contains_domain_alias(&snippet) {
                return Err(DenyReason::TranslatorFailed);
            }
            // ... and no entity raw value may survive into agent-visible output.
            if hit
                .entities
                .iter()
                .any(|entity| snippet.contains(&entity.raw_value))
            {
                return Err(DenyReason::TranslatorFailed);
            }

            translated.push(AgentSearchHit {
                doc_id: hit.doc_id,
                snippet,
            });
        }
        Ok(translated)
    }
}
