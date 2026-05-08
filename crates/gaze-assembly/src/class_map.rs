use gaze::{Action, Context, PiiClass, RuleSpec, RulepackError};

pub(crate) fn class_for_dictionary(
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
    if class_has_tokenize_or_stricter_action(&policy.rules, override_class)? {
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

fn class_has_tokenize_or_stricter_action(
    rules: &[RuleSpec],
    class: &PiiClass,
) -> Result<bool, RulepackError> {
    for rule in rules {
        let action = match rule {
            RuleSpec::Class {
                class: rule_class,
                action,
            } if rule_class == class => Some(action),
            RuleSpec::Class { .. } => None,
            RuleSpec::Column { .. } => None,
            RuleSpec::Default { action } => Some(action),
            _ => {
                return Err(RulepackError::UnsupportedRuleSpec {
                    variant: format!("{:?}", rule),
                })
            }
        };
        if let Some(action) = action {
            return Ok(matches!(
                action,
                Action::Tokenize | Action::Redact | Action::FormatPreserve | Action::Generalize
            ));
        }
    }
    Ok(false)
}
