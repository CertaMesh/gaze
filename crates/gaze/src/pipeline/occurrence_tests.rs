use super::*;

#[test]
fn primary_occurrences_retain_distinct_original_evidence_and_ownership() {
    let pipeline = Pipeline::builder()
        .detector(RepeatedEmail)
        .rule(crate::rule::DefaultRule::new(Action::Tokenize))
        .build().unwrap();
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let clean = pipeline.redact_text_with_manifest_uncached(
        &mut ProtectionTarget::Live(&session),
        "alice@example.invalid alice@example.invalid",
        None,
        DocumentKind::Text,
        &[crate::LocaleTag::Global],
        &DictionaryBundle::default(),
        None,
    ).unwrap();
    assert_eq!(clean.manifest.records().len(), 2);
    assert_eq!(clean.manifest.evidence_count(), 2);
    assert!(clean.manifest.records().iter().all(|r| r.owned && matches!(r.origin, occurrence::Origin::Selection { .. })));
    assert_ne!(clean.manifest.records()[0].id, clean.manifest.records()[1].id);
    assert_eq!(session.restore_strict_text(&clean.text).unwrap(), "alice@example.invalid alice@example.invalid");
}

struct RepeatedEmail;
impl Detector for RepeatedEmail {
    fn detect(&self, text: &str) -> Vec<Detection> {
        text.match_indices("alice@example.invalid")
            .map(|(start, value)| Detection::new(start..start + value.len(), PiiClass::Email, "synthetic.email"))
            .collect()
    }
}
