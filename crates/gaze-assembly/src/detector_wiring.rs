use std::collections::{BTreeMap, BTreeSet, HashMap};

use gaze::{
    Context, DetectorKind, PiiClass, PipelineBuilder, PolicyError, RawMatch, Rulepack,
    RulepackError,
};
use gaze_recognizers::{DictionaryRecognizer, NormalizerKind, RegexDetector, ValidatorKind};

use crate::{class_for_dictionary, lower_regex_pattern, BuildError};

pub(crate) fn register_policy_detectors(
    mut builder: PipelineBuilder,
    policy: &gaze::Policy,
    context: &Context,
    registered_dictionaries: &mut BTreeSet<String>,
) -> Result<PipelineBuilder, BuildError> {
    for detector in &policy.detectors {
        builder = match &detector.kind {
            DetectorKind::Regex => builder.detector(RegexDetector::with_source(
                detector
                    .pattern
                    .as_deref()
                    .ok_or_else(|| PolicyError::BadDictionary {
                        name: detector.name.clone(),
                        reason: "regex recognizer missing pattern".to_string(),
                    })?,
                detector.class.clone(),
                &detector.name,
            )?),
            DetectorKind::Dictionary => {
                let dictionary_name = detector.dictionary_name.as_deref().ok_or_else(|| {
                    PolicyError::BadDictionary {
                        name: detector.name.clone(),
                        reason: "dictionary recognizer missing dictionary name".to_string(),
                    }
                })?;
                registered_dictionaries.insert(dictionary_name.to_string());
                let class =
                    class_for_dictionary(policy, context, dictionary_name, detector.class.clone())?;
                builder.recognizer(DictionaryRecognizer::new(
                    format!("dict/{}", detector.name),
                    class,
                    dictionary_name,
                    detector.case_sensitive,
                    detector.token_family.clone(),
                ))
            }
            DetectorKind::Unknown(kind) => {
                return Err(PolicyError::BadTtl(format!("unknown detector.kind '{kind}'")).into())
            }
        };
    }

    Ok(builder)
}

pub(crate) fn register_rulepack_recognizers(
    mut builder: PipelineBuilder,
    policy: &gaze::Policy,
    context: &Context,
    rulepacks: &[Rulepack],
    locale_vocab: &HashMap<String, Vec<String>>,
    registered_dictionaries: &mut BTreeSet<String>,
) -> Result<PipelineBuilder, BuildError> {
    let mut rulepack_recognizers = BTreeMap::<String, (String, gaze::RecognizerSpec)>::new();
    for rulepack in rulepacks {
        for recognizer in &rulepack.recognizers {
            tracing::info!(recognizer_id = %recognizer.id, "loaded rulepack recognizer");
            if let Some((first_pack, _)) = rulepack_recognizers.get(&recognizer.id) {
                return Err(RulepackError::DuplicateId {
                    id: recognizer.id.clone(),
                    first_pack: first_pack.clone(),
                    second_pack: rulepack.rulepack_id.clone(),
                }
                .into());
            }
            rulepack_recognizers.insert(
                recognizer.id.clone(),
                (rulepack.rulepack_id.clone(), recognizer.clone()),
            );
        }
    }

    for (_, recognizer) in rulepack_recognizers
        .into_values()
        .filter(|(_, recognizer)| recognizer.enabled)
    {
        match recognizer.matcher {
            RawMatch::Regex {
                pattern,
                pattern_template,
                capture_groups,
            } => {
                let pattern =
                    lower_regex_pattern(&recognizer.id, pattern, pattern_template, locale_vocab)?;
                let exclusions = recognizer
                    .context
                    .as_ref()
                    .map(|context| context.exclusions.clone())
                    .unwrap_or_default();
                let validator_kind = recognizer
                    .validator
                    .as_ref()
                    .map(|validator| ValidatorKind::parse(&validator.kind))
                    .transpose()?;
                let normalizer_kind = recognizer
                    .normalizer
                    .as_ref()
                    .map(|normalizer| NormalizerKind::parse(&normalizer.kind))
                    .transpose()?;
                builder = builder.recognizer(RegexDetector::with_rulepack_fields(
                    &pattern,
                    recognizer.class,
                    &recognizer.id,
                    recognizer.locales,
                    recognizer.scoring.base,
                    recognizer.scoring.priority,
                    recognizer.token.family.as_deref().unwrap_or("counter"),
                    capture_groups,
                    exclusions,
                    validator_kind,
                    normalizer_kind,
                )?);
            }
            RawMatch::Dictionary {
                terms,
                terms_file,
                terms_from_context,
                case_sensitive,
            } => {
                let dictionary_name = terms_from_context
                    .as_deref()
                    .unwrap_or(recognizer.id.as_str())
                    .to_string();
                let dictionary_has_inline_terms = !terms.is_empty() || terms_file.is_some();
                if !dictionary_has_inline_terms && terms_from_context.is_none() {
                    return Err(PolicyError::BadDictionary {
                        name: recognizer.id.clone(),
                        reason:
                            "dictionary matcher requires terms, terms_file, or terms_from_context"
                                .to_string(),
                    }
                    .into());
                }
                let id = recognizer.id;
                let token_family = recognizer
                    .token
                    .family
                    .unwrap_or_else(|| "counter".to_string());
                let locales = recognizer.locales;
                let base_score = recognizer.scoring.base;
                let priority = recognizer.scoring.priority;
                registered_dictionaries.insert(dictionary_name.clone());
                let class =
                    class_for_dictionary(policy, context, &dictionary_name, recognizer.class)?;
                builder = builder.recognizer(DictionaryRecognizer::with_rulepack_fields(
                    id,
                    class,
                    dictionary_name,
                    case_sensitive,
                    token_family,
                    locales,
                    base_score,
                    priority,
                ));
            }
            RawMatch::Ner { .. } => {
                return Err(RulepackError::UnsupportedMatcher("Ner".to_string()).into())
            }
        }
    }

    Ok(builder)
}

pub(crate) fn register_context_dictionaries(
    mut builder: PipelineBuilder,
    policy: &gaze::Policy,
    context: &Context,
    registered_dictionaries: &BTreeSet<String>,
) -> Result<PipelineBuilder, BuildError> {
    for name in context.dictionaries.keys() {
        if registered_dictionaries.contains(name) {
            continue;
        }
        let class = class_for_dictionary(policy, context, name, PiiClass::custom(name))?;
        builder = builder.recognizer(DictionaryRecognizer::new(
            format!("context/{name}"),
            class,
            name,
            context.dictionaries[name].case_sensitive,
            "counter",
        ));
    }

    Ok(builder)
}
