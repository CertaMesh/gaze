//! Harness-only public API sequence; copied verbatim when measuring old runtimes.
use super::*;
use gaze::{DefaultRule, Recognizer};
use gaze_recognizers::DictionaryRecognizer;

trait ContextBundleResult {
    fn context_bundle_result(self) -> Result<DictionaryBundle, Box<dyn std::error::Error>>;
}

impl ContextBundleResult for DictionaryBundle {
    fn context_bundle_result(self) -> Result<DictionaryBundle, Box<dyn std::error::Error>> {
        Ok(self)
    }
}

impl<E: std::error::Error + 'static> ContextBundleResult for Result<DictionaryBundle, E> {
    fn context_bundle_result(self) -> Result<DictionaryBundle, Box<dyn std::error::Error>> {
        self.map_err(Into::into)
    }
}

pub(super) fn handle(
    sequence: &str,
    mut request: Request,
) -> Result<Outcome, Box<dyn std::error::Error>> {
    let raw = request
        .context_json
        .take()
        .ok_or("sequence requires dictionary context")?;
    let context = Context::from_json_str(&raw)?;
    let dictionaries = gaze::dictionary_bundle_from_context(&context).context_bundle_result()?;
    if dictionaries.iter().count() != 1 || dictionaries.get("sequence").is_none() {
        return Err("sequence requires exactly one dictionary".into());
    }
    let locales = vec![LocaleTag::Global];
    let mut recognizer = DictionaryRecognizer::new(
        "bench/dictionary-sequence",
        PiiClass::Name,
        "sequence",
        true,
        "counter",
    )
    .with_record_matching();
    if sequence != "fresh" {
        // Compile the literal record automaton before the builder changes its mode.
        // The detect result is deliberately not used to construct the gold.
        recognizer.detect(&request.text, &DetectContext::new(&locales, &dictionaries))?;
    }
    match sequence {
        "switch" | "fresh" => recognizer = recognizer.with_unicode_case_insensitive(),
        "literal" => {}
        _ => return Err("unknown dictionary sequence".into()),
    }
    let run = PolicyRun {
        pipeline: Pipeline::builder()
            .recognizer(recognizer)
            .rule(DefaultRule::new(Action::Tokenize))
            .build()?,
        locale_chain: LocaleChain::from_tags(locales),
        dictionaries,
    };
    handle_request_with_policy(
        BenchConfig::RuleFloorCore,
        &run.pipeline,
        request,
        Some(&run),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Request {
        Request {
            fixture_id: "dictionary-sequence-control".into(),
            locale_chain: vec!["global".into()],
            text: "Zavé Lune then ZAVÉ LUNE; Wavé Vune".into(),
            context_json: Some(
                r#"{"dictionaries":{"sequence":{"terms":["Zavé Lune"],"case_sensitive":true}}}"#
                    .into(),
            ),
            session_hex: None,
        }
    }

    #[test]
    fn fresh_and_literal_controls_use_real_clean_restore_and_manifest() {
        // The real pipeline also sweeps equivalent repeats of a detected name.
        for mode in ["fresh", "literal"] {
            let Outcome::Success(response) = handle(mode, request()).unwrap() else {
                panic!("control must complete");
            };
            assert_eq!(response.manifest_spans.len(), 2);
            assert!(response.restore.exact);
            assert_eq!(response.manifest_integrity.raw_value_mismatches, 0);
            assert_eq!(response.manifest_integrity.token_restore_failures, 0);
        }
    }

    #[test]
    fn malformed_sequence_is_an_error_instead_of_a_fresh_fallback() {
        assert!(handle("unknown", request()).is_err());
        let mut missing = request();
        missing.context_json = None;
        assert!(handle("switch", missing).is_err());
    }
}
