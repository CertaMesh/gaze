use gaze::{
    Action, Candidate, ClassRule, CleanDocument, CollisionMembership, ConflictTier, DefaultRule,
    DetectContext, DictionaryBundle, LocaleTag, PiiClass, Pipeline, Recognizer, SafetyNetPolicy,
    Scope, Session,
};

#[derive(Clone)]
struct Fixed {
    prefix: &'static str,
    candidates: Vec<Candidate>,
}

impl Recognizer for Fixed {
    fn id(&self) -> &str {
        "synthetic.fixed"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        text: &str,
        _: &DetectContext<'_>,
    ) -> Result<Vec<Candidate>, gaze_types::DetectError> {
        assert!(text.starts_with(self.prefix));
        Ok(self
            .candidates
            .iter()
            .cloned()
            .map(|candidate| {
                let span = candidate.span.start + self.prefix.len()
                    ..candidate.span.end + self.prefix.len();
                candidate.with_span(span)
            })
            .collect())
    }
}

fn candidate(start: usize, end: usize, class: PiiClass, id: &str) -> Candidate {
    Candidate::new(
        start..end,
        class,
        id,
        0.9,
        0,
        None,
        "counter",
        id,
        ConflictTier::None,
        vec![],
    )
}

#[test]
fn normalized_prefixes_keep_primary_recovered_and_residual_spans_raw() {
    // Normalization operates per scalar: NFD and CRLF stay unchanged, while
    // U+0344 expands, joiners disappear, and fullwidth characters shrink.
    for (raw_prefix, normalized_prefix) in [
        ("é ", "é "),
        ("e\u{0301} ", "e\u{0301} "),
        ("\u{0344} ", "\u{0308}\u{0301} "),
        ("x\u{200c}\u{200d} ", "x "),
        ("ｘ： ", "x: "),
        ("x\r\n", "x\r\n"),
        ("ｘ\u{0344}\u{200d}\r\n", "x\u{0308}\u{0301}\r\n"),
    ] {
        let raw = format!("{raw_prefix}password: \"left right\"\nmarker");
        // The default builder enables residual coverage, also used by setup's
        // default tokenize policy. Do not switch it on through test internals.
        let pipeline = Pipeline::builder()
            .recognizer(Fixed {
                prefix: normalized_prefix,
                candidates: vec![
                    candidate(11, 15, PiiClass::Name, "synthetic.left"),
                    candidate(
                        11,
                        21,
                        PiiClass::custom("password").unwrap(),
                        "synthetic.field",
                    ),
                    candidate(16, 29, PiiClass::Name, "synthetic.right"),
                ],
            })
            .rule(DefaultRule::new(Action::Tokenize))
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (clean, manifest, _, _) = pipeline
            .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session,
                &raw,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        let offset = raw_prefix.len();
        assert_eq!(
            manifest
                .iter()
                .map(|span| (span.raw_span.clone(), span.class.clone()))
                .collect::<Vec<_>>(),
            vec![
                (offset + 11..offset + 15, PiiClass::Name),
                (
                    offset + 15..offset + 16,
                    PiiClass::custom("password").unwrap()
                ),
                (offset + 16..offset + 29, PiiClass::Name),
            ],
            "{raw_prefix:?}"
        );
        let CleanDocument::Text(clean) = clean else {
            panic!("text result")
        };
        assert!(clean.starts_with(raw_prefix));
        assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
    }
}

#[test]
fn mixed_selection_classes_keep_family_tie_policy_and_residual_class() {
    let family = PiiClass::family("document");
    let field = PiiClass::custom("password").unwrap();
    for family_action in [Action::Preserve, Action::Tokenize] {
        let pipeline = Pipeline::builder()
            .recognizer(Fixed {
                prefix: "",
                candidates: vec![
                    candidate(0, 5, PiiClass::custom("alpha").unwrap(), "synthetic.alpha"),
                    candidate(3, 8, PiiClass::custom("beta").unwrap(), "synthetic.beta"),
                    candidate(9, 12, PiiClass::Name, "synthetic.name"),
                    candidate(14, 17, PiiClass::Email, "synthetic.email"),
                    candidate(15, 20, field.clone(), "synthetic.field"),
                ],
            })
            .register_collision(
                "synthetic.alpha",
                CollisionMembership::new("document", "alpha", 10, None),
            )
            .register_collision(
                "synthetic.beta",
                CollisionMembership::new("document", "beta", 10, None),
            )
            .rule(ClassRule::new(family.clone(), family_action))
            .rule(ClassRule::new(PiiClass::Name, Action::Preserve))
            .rule(DefaultRule::new(Action::Tokenize))
            .build()
            .unwrap();
        let raw = "xxxxxxxx xxxxx xxxxxx";
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (clean, manifest, _, _) = pipeline
            .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session,
                raw,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        let mut expected = vec![(14..17, PiiClass::Email), (17..20, field.clone())];
        if family_action == Action::Tokenize {
            expected.insert(0, (0..8, family.clone()));
        }
        assert_eq!(
            manifest
                .iter()
                .map(|span| (span.raw_span.clone(), span.class.clone()))
                .collect::<Vec<_>>(),
            expected
        );
        let CleanDocument::Text(clean) = clean else {
            panic!("text result")
        };
        assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
    }
}
