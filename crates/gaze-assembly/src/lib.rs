use std::collections::{BTreeMap, BTreeSet};

use gaze::{
    Action, ClassRule, ColumnRule, Context, DefaultRule, DetectorKind, LocaleChain, PiiClass,
    Pipeline, PolicyError, RawMatch, RuleSpec, Rulepack, RulepackError,
};
use gaze_recognizers::{
    DictionaryRecognizer, NerOptions, NerRecognizer, NormalizerKind, RegexDetector, ValidatorKind,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("policy error: {0}")]
    Policy(#[from] gaze::PolicyError),
    #[error("rulepack error: {0}")]
    Rulepack(#[from] gaze::RulepackError),
    #[error("pipeline error: {0}")]
    Pipeline(#[from] gaze::Error),
}

pub fn build_pipeline(
    policy: &gaze::Policy,
    context: &Context,
    rulepacks: &[Rulepack],
    active_locales: &LocaleChain,
    ner_threshold: Option<f32>,
) -> Result<Pipeline, BuildError> {
    let mut builder = Pipeline::builder();
    let mut registered_dictionaries = BTreeSet::<String>::new();
    let locale_headers = merged_locale_email_headers(rulepacks, active_locales);

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
                let pattern = lower_regex_pattern(
                    &recognizer.id,
                    pattern,
                    pattern_template,
                    &locale_headers,
                )?;
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

    for rule in &policy.rules {
        builder = match rule {
            RuleSpec::Class { class, action } => {
                builder.rule(ClassRule::new(class.clone(), *action))
            }
            RuleSpec::Column { column, action } => builder.rule(ColumnRule::new(column, *action)),
            RuleSpec::Default { action } => builder.rule(DefaultRule::new(*action)),
        };
    }

    if let Some(ner) = &policy.ner {
        if let Some(path) = &ner.model_dir {
            let detector = NerRecognizer::load_with_options(
                path,
                NerOptions {
                    locale: ner.locale.clone(),
                    threshold: ner_threshold.unwrap_or(ner.threshold),
                },
            )
            .map_err(|err| PolicyError::NerLoad(err.to_string()))?;
            builder = builder.recognizer(detector);
        }
    }

    Ok(builder.build()?)
}

fn class_for_dictionary(
    policy: &gaze::Policy,
    context: &Context,
    dictionary_name: &str,
    original_class: PiiClass,
) -> Result<PiiClass, RulepackError> {
    let Some(override_class) = context.class_map.get(dictionary_name) else {
        return Ok(original_class);
    };
    if override_class == &original_class {
        return Ok(original_class);
    }
    if class_has_tokenize_or_stricter_action(&policy.rules, override_class) {
        Ok(override_class.clone())
    } else {
        Err(RulepackError::ClassMapOverrideClash {
            dict: dictionary_name.to_string(),
            old_class: original_class,
            new_class: override_class.clone(),
            uncovered_rule: format!(
                "no tokenize-or-stricter action rule covers {:?}",
                override_class
            ),
        })
    }
}

fn class_has_tokenize_or_stricter_action(rules: &[RuleSpec], class: &PiiClass) -> bool {
    for rule in rules {
        let action = match rule {
            RuleSpec::Class {
                class: rule_class,
                action,
            } if rule_class == class => Some(action),
            RuleSpec::Default { action } => Some(action),
            _ => None,
        };
        if let Some(action) = action {
            return matches!(
                action,
                Action::Tokenize | Action::Redact | Action::FormatPreserve | Action::Generalize
            );
        }
    }
    false
}

fn lower_regex_pattern(
    id: &str,
    pattern: Option<String>,
    pattern_template: Option<String>,
    locale_headers: &[String],
) -> Result<String, RulepackError> {
    match (pattern, pattern_template) {
        (Some(pattern), None) => Ok(pattern),
        (None, Some(template)) => lower_pattern_template(id, &template, locale_headers),
        _ => Err(RulepackError::RegexPatternChoice { id: id.to_string() }),
    }
}

fn lower_pattern_template(
    id: &str,
    template: &str,
    locale_headers: &[String],
) -> Result<String, RulepackError> {
    let mut lowered = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        lowered.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return Err(RulepackError::UnknownPatternTemplatePlaceholder {
                id: id.to_string(),
                placeholder: after.to_string(),
            });
        };
        let placeholder = &after[..end];
        match placeholder {
            "locale_email_headers" => lowered.push_str(&format!(
                "({})",
                locale_headers
                    .iter()
                    .map(|name| regex::escape(name))
                    .collect::<Vec<_>>()
                    .join("|")
            )),
            other => {
                return Err(RulepackError::UnknownPatternTemplatePlaceholder {
                    id: id.to_string(),
                    placeholder: other.to_string(),
                })
            }
        }
        rest = &after[end + 1..];
    }
    lowered.push_str(rest);
    Ok(lowered)
}

fn merged_locale_email_headers(
    rulepacks: &[Rulepack],
    active_locales: &LocaleChain,
) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = BTreeSet::new();

    for active_locale in active_locales.as_slice() {
        for rulepack in rulepacks {
            if !rulepack.default_locales.contains(active_locale) {
                continue;
            }
            let Some(headers) = rulepack
                .locale
                .as_ref()
                .and_then(|locale| locale.email_headers.as_ref())
            else {
                continue;
            };
            for name in &headers.names {
                if seen.insert(name.clone()) {
                    names.push(name.clone());
                }
            }
        }
    }

    if names.is_empty() {
        default_email_headers()
    } else {
        names
    }
}

fn default_email_headers() -> Vec<String> {
    vec![
        "From".to_string(),
        "To".to_string(),
        "Cc".to_string(),
        "Bcc".to_string(),
        "Reply-To".to_string(),
        "Sender".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze::{
        Action, CleanDocument, LocaleTag, RawDocument, Scope, Session, SessionPolicy, SessionScope,
    };

    fn policy() -> gaze::Policy {
        gaze::Policy {
            session: SessionPolicy {
                scope: SessionScope::Ephemeral,
                ttl_secs: None,
            },
            detectors: Vec::new(),
            dictionaries: Vec::new(),
            rules: vec![
                RuleSpec::Class {
                    class: PiiClass::Name,
                    action: Action::Tokenize,
                },
                RuleSpec::Default {
                    action: Action::Preserve,
                },
            ],
            ner: None,
            rulepacks: gaze::RulepackPolicy {
                bundled: Vec::new(),
                paths: Vec::new(),
            },
            locale: Some(vec![LocaleTag::DeDe]),
        }
    }

    fn empty_context() -> Context {
        Context {
            dictionaries: std::collections::HashMap::new(),
            class_map: std::collections::HashMap::new(),
            fields: serde_json::Map::new(),
        }
    }

    fn policy_with_registered_dictionary(rules: Vec<RuleSpec>) -> gaze::Policy {
        gaze::Policy {
            session: SessionPolicy {
                scope: SessionScope::Ephemeral,
                ttl_secs: None,
            },
            detectors: vec![gaze::DetectorSpec {
                kind: DetectorKind::Dictionary,
                name: "alpha".to_string(),
                pattern: None,
                class: PiiClass::custom("foo"),
                dictionary_name: Some("dict_alpha".to_string()),
                case_sensitive: true,
                token_family: "counter".to_string(),
            }],
            dictionaries: Vec::new(),
            rules,
            ner: None,
            rulepacks: gaze::RulepackPolicy {
                bundled: Vec::new(),
                paths: Vec::new(),
            },
            locale: Some(vec![LocaleTag::Global]),
        }
    }

    fn context_with_alpha_override() -> Context {
        Context {
            dictionaries: std::collections::HashMap::from([(
                "dict_alpha".to_string(),
                gaze::ContextDictionary {
                    terms: vec!["context-song-123".to_string()],
                    case_sensitive: true,
                },
            )]),
            class_map: std::collections::HashMap::from([(
                "dict_alpha".to_string(),
                PiiClass::custom("bar"),
            )]),
            fields: serde_json::Map::new(),
        }
    }

    #[test]
    fn t20_context_class_map_overrides_policy_dict_class() {
        let policy = policy_with_registered_dictionary(vec![
            RuleSpec::Class {
                class: PiiClass::custom("bar"),
                action: Action::Tokenize,
            },
            RuleSpec::Default {
                action: Action::Preserve,
            },
        ]);
        let context = context_with_alpha_override();
        let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
        let pipeline =
            build_pipeline(&policy, &context, &[], &active_locales, None).expect("pipeline");
        let dictionaries = gaze::DictionaryBundle::from_context(&context);
        let fields = serde_json::Map::new();
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = pipeline
            .redact_with_detect_context(
                &session,
                RawDocument::Text("track context-song-123".to_string()),
                active_locales.as_slice(),
                &dictionaries,
                &fields,
            )
            .expect("redact");

        let CleanDocument::Text(text) = clean else {
            panic!("expected text");
        };
        assert!(regex::Regex::new(r"^track <[0-9a-f]{8}:Custom:bar_\d+>$")
            .unwrap()
            .is_match(&text));
    }

    #[test]
    fn t20a_class_map_override_fails_closed_when_action_rule_uncovered() {
        let policy = policy_with_registered_dictionary(vec![
            RuleSpec::Class {
                class: PiiClass::custom("foo"),
                action: Action::Tokenize,
            },
            RuleSpec::Default {
                action: Action::Preserve,
            },
        ]);
        let context = context_with_alpha_override();
        let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);

        let err = match build_pipeline(&policy, &context, &[], &active_locales, None) {
            Ok(_) => panic!("uncovered class_map override must fail closed"),
            Err(err) => err,
        };

        assert!(matches!(
            err,
            BuildError::Rulepack(RulepackError::ClassMapOverrideClash {
                dict,
                old_class,
                new_class,
                ..
            }) if dict == "dict_alpha"
                && old_class == PiiClass::custom("foo")
                && new_class == PiiClass::custom("bar")
        ));
    }

    #[test]
    fn pattern_template_lowers_correctly_under_locale_chain_de() {
        let core = Rulepack::load(gaze::RulepackSource::Embedded(
            gaze_recognizers::embedded("core").expect("core"),
        ))
        .expect("core");
        let de = Rulepack::load(gaze::RulepackSource::Embedded(
            gaze_recognizers::embedded("locale-de").expect("locale-de"),
        ))
        .expect("de");
        let email_header = Rulepack::parse(
            r#"
schema_version = "0.1.0"
rulepack_id = "email-header"
rulepack_version = "0.4.1"
default_locales = ["global"]

[[recognizers]]
id = "email.header.name"
class = "Name"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "regex"
pattern_template = '''(?m)^{locale_email_headers}:\s+([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+)\s+<[^>]+>'''
capture_groups = [2]

[recognizers.scoring]
base = 0.90
priority = 100

[recognizers.token]
family = "email.header.name"
"#,
        )
        .expect("email header rulepack");
        let policy = policy();
        let active_locales =
            LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), Some(&[LocaleTag::DeDe]));
        let pipeline = build_pipeline(
            &policy,
            &empty_context(),
            &[core, de, email_header],
            &active_locales,
            None,
        )
        .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = pipeline
            .redact(
                &session,
                RawDocument::Text("Von: Dana Weber <user@example.invalid>".into()),
            )
            .expect("redact");

        let CleanDocument::Text(text) = clean else {
            panic!("expected text");
        };
        assert!(regex::Regex::new(
            r"^Von: <[0-9a-f]{8}:Name_\d+> <[a-z0-9._%+\-]+@example\.invalid>$"
        )
        .unwrap()
        .is_match(&text));
    }

    #[test]
    fn pattern_template_unknown_placeholder_fails_closed() {
        let rulepack = Rulepack::parse(
            r#"
schema_version = "0.1.0"
rulepack_id = "bad-template"
rulepack_version = "0.4.1"
default_locales = ["global"]

[[recognizers]]
id = "bad.template"
class = "Name"
enabled = true

[recognizers.match]
kind = "regex"
pattern_template = '''{unknown_placeholder}: (.+)'''
"#,
        )
        .expect("parse");

        let policy = policy();
        let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
        let err = match build_pipeline(
            &policy,
            &empty_context(),
            &[rulepack],
            &active_locales,
            None,
        ) {
            Ok(_) => panic!("unknown placeholder must fail"),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            BuildError::Rulepack(RulepackError::UnknownPatternTemplatePlaceholder {
                placeholder,
                ..
            }) if placeholder == "unknown_placeholder"
        ));
    }
}
