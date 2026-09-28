use gaze::{first_matching_action, Action, Context, PiiClass, RuleSpec, RulepackError};

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

pub(crate) fn class_has_tokenize_or_stricter_action(
    rules: &[RuleSpec],
    class: &PiiClass,
) -> Result<bool, RulepackError> {
    let found = first_matching_action(rules, |rule| match rule {
        RuleSpec::Class {
            class: rule_class,
            action,
        } if rule_class == class => Ok(Some(*action)),
        RuleSpec::Class { .. } | RuleSpec::Column { .. } => Ok(None),
        RuleSpec::Default { action } => Ok(Some(*action)),
        _ => Err(RulepackError::UnsupportedRuleSpec {
            variant: format!("{:?}", rule),
        }),
    })?;
    Ok(found.is_some_and(|(_, action)| {
        matches!(
            action,
            Action::Tokenize | Action::Redact | Action::FormatPreserve | Action::Generalize
        )
    }))
}

/// The first matching rule must keep record-supplied values restorable.
pub fn class_has_reversible_action(rules: &[RuleSpec], class: &PiiClass) -> bool {
    first_matching_action(rules, |rule| {
        Ok::<_, std::convert::Infallible>(match rule {
            RuleSpec::Class {
                class: named,
                action,
            } if named == class => Some(*action),
            RuleSpec::Default { action } => Some(*action),
            _ => None,
        })
    })
    .ok()
    .flatten()
    .is_some_and(|(_, action)| matches!(action, Action::Tokenize | Action::FormatPreserve))
}
