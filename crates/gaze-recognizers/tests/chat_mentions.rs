//! Slack and Discord user mentions. IDs and display names are invented.
use gaze::{DictionaryBundle, LocaleTag, SafetyNetPolicy, Scope, Session};
use gaze_assembly::CorePipelineConfig;

/// The username spans of `raw` under the default `core` bundle, after a strict restore round trip.
fn usernames(raw: &str) -> Vec<String> {
    let core = CorePipelineConfig::new()
        .with_locale(&[LocaleTag::parse("en-US").unwrap()])
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, spans, _, _) = core
        .pipeline()
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            raw,
            &[LocaleTag::Global, LocaleTag::parse("en-US").unwrap()],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap();
    let gaze::CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(
        core.pipeline()
            .restore_strict_text(&session, &clean)
            .unwrap(),
        raw
    );
    spans
        .iter()
        .filter(|span| matches!(&span.class, gaze::PiiClass::Custom(class) if class == "username"))
        .map(|span| raw[span.raw_span.clone()].to_string())
        .collect()
}

#[test]
fn a_user_mention_protects_the_whole_value_inside_its_markup() {
    for (raw, value) in [
        ("<@U07RW7V7TU> can you check the deploy?", "U07RW7V7TU"),
        ("Thanks <@W0AGGCEFX>, merged.", "W0AGGCEFX"),
        // A display name, or a name written into the ID, never stays visible beside a token.
        (
            "cc <@U02QX7LMNP|edda.lindqvist> for review",
            "U02QX7LMNP|edda.lindqvist",
        ),
        ("ping <@U02EDDA_LINDQVIST> again", "U02EDDA_LINDQVIST"),
        (
            "<@418829201733066752> joined the call",
            "418829201733066752",
        ),
        ("hey <@!418829201733066752>, ready?", "418829201733066752"),
        (
            r#"{"type":"message","text":"<@U07RW7V7TU> approved the plan"}"#,
            "U07RW7V7TU",
        ),
    ] {
        assert_eq!(usernames(raw), [value], "{raw:?}");
    }
}

#[test]
fn mention_markup_without_a_person_stays_raw() {
    for raw in [
        "Alerts go to <#C0WBJXRVX0|general> now.",
        "<!here> standup in five minutes",
        "Paging <!subteam^S0AV3W33XQ|@oncall>.",
        "<@&1476165865706792983> patch day is tomorrow.",
        "Continue in <#107548143513636632>.",
        "Ship it <:partyparrot:431573057380746530>",
        "Maintenance starts <t:1773103654:R>.",
        "Runbook: <https://example.invalid/runbook/4142|runbook>",
        "Workspace T0LXJVNA23 enabled SSO.",
        "Callable<@UIThread Void> task = () -> null;",
        "Map<@NonNull String, Long> counts;",
    ] {
        assert_eq!(usernames(raw), Vec::<String>::new(), "{raw:?}");
    }
}
