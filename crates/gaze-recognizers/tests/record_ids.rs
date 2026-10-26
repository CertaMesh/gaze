//! Synthetic CRM/HR identifiers. No shape alone proves a person linkage.
use gaze::{CleanDocument, DictionaryBundle, LocaleTag, PiiClass, SafetyNetPolicy, Scope, Session};
use gaze_assembly::CorePipelineConfig;

#[test]
fn grouped_personal_identifiers_keep_the_complete_owned_value() {
    let core = CorePipelineConfig::new().build().unwrap();
    for (cue, value, class) in [
        ("customer ID", "FABLE 407", "customer_id"),
        ("Kundennummer", "FÄBEL\u{00a0}407", "customer_id"),
        ("employee ID", "VIOLET 508", "employee_id"),
        ("Personalnummer", "VIOLE\u{0301}T 508", "employee_id"),
        ("member ID", "BRONZE 609", "record_id"),
        ("Mitgliedsnummer", "BRÖNZE 609", "record_id"),
    ] {
        let input = format!("{cue}: {value}; status=active");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (clean, spans, _, trace, _) = core
            .pipeline()
            .clean_text_with_safety_net_policy_detect_context_and_protection_evidence(
                &session,
                &input,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        let CleanDocument::Text(clean) = clean else {
            panic!("text")
        };
        assert_eq!(spans.len(), 1);
        assert_eq!(&input[spans[0].raw_span.clone()], value);
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0].source_ids(), &[format!("{class}.labelled")]);
        assert_eq!(trace[0].class(), &PiiClass::custom(class).unwrap());
        let entries = session.snapshot_entries();
        assert_eq!(
            entries.len(),
            1,
            "whole value leaked: {input:?} -> {clean:?}"
        );
        assert_eq!(entries[0].raw, value);
        assert_eq!(entries[0].class, PiiClass::custom(class).unwrap());
        assert_eq!(clean, format!("{cue}: {}; status=active", entries[0].token));
        assert_eq!(
            core.pipeline()
                .restore_strict_text(&session, &clean)
                .unwrap(),
            input
        );
    }
}

#[test]
fn quoted_personal_scalars_preserve_every_raw_byte() {
    let core = CorePipelineConfig::new().build().unwrap();
    for (key, class) in [
        ("customer_id", "customer_id"),
        ("employeeId", "employee_id"),
        ("contact_record_id", "record_id"),
    ] {
        for raw in [
            "LILAC é984321",
            "@FABLE984321",
            r"FABLE\u00e9984321",
            r"FABLE\u0065\u0301984321",
            r"FABLE\t984321",
            r#"FABLE\"984321"#,
            r"FABLE\\984321",
            r"FABLE984321\\",
        ] {
            for embedded in [false, true] {
                let value = if embedded {
                    serde_json::to_string(raw).unwrap()
                        [1..serde_json::to_string(raw).unwrap().len() - 1]
                        .to_owned()
                } else {
                    raw.to_owned()
                };
                let input = if embedded {
                    format!(r#"{{"payload":"{{\"{key}\":\"{value}\"}}"}}"#)
                } else {
                    format!(r#"{{"{key}":"{value}"}}"#)
                };
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap()
                else {
                    panic!("text")
                };
                let entries = session.snapshot_entries();
                assert_eq!(entries.len(), 1, "{input:?} -> {clean:?}");
                assert_eq!(entries[0].raw, value, "quoted suffix leaked: {input:?}");
                assert_eq!(entries[0].class, PiiClass::custom(class).unwrap());
                assert_eq!(clean, input.replacen(&value, &entries[0].token, 1));
                assert_eq!(
                    core.pipeline()
                        .restore_strict_text(&session, &clean)
                        .unwrap(),
                    input
                );
                let public = input.replace(key, "artifact_key");
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &public).unwrap()
                else {
                    panic!("text")
                };
                assert_eq!(clean, public);
                assert!(session.snapshot_entries().is_empty());
            }
        }
    }
}

#[test]
fn quoted_log_values_and_unterminated_values_never_leave_a_partial_suffix() {
    let core = CorePipelineConfig::new().build().unwrap();
    for cue in ["customer ID", "Personalnummer", "Mitgliedsnummer"] {
        for quote in ['\"', '\''] {
            let value = "LILAC é984321";
            let input =
                format!("{cue}: {quote}{value}{quote}; ORDER_ID=BENCH976543; repeat {value}");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap()
            else {
                panic!("text")
            };
            let entries = session.snapshot_entries();
            assert_eq!(entries.len(), 1, "{input:?} -> {clean:?}");
            assert_eq!(entries[0].raw, value);
            assert_eq!(
                clean,
                format!(
                    "{cue}: {quote}{}{quote}; ORDER_ID=BENCH976543; repeat {}",
                    entries[0].token, entries[0].token
                )
            );
            assert_eq!(
                core.pipeline()
                    .restore_strict_text(&session, &clean)
                    .unwrap(),
                input
            );
            let CleanDocument::Text(repeated) = core.pseudonymize_text(&session, &input).unwrap()
            else {
                panic!("text")
            };
            assert_eq!(repeated, clean);
        }
        let raw = "LILAC é984321; ORDER_ID=BENCH976543";
        let input = format!("{cue}: \"{raw}");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap() else {
            panic!("text")
        };
        let entries = session.snapshot_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].raw, raw);
        assert_eq!(clean, format!("{cue}: \"{}", entries[0].token));
        assert_eq!(
            core.pipeline()
                .restore_strict_text(&session, &clean)
                .unwrap(),
            input
        );
    }
}

#[test]
fn adjacent_personal_fields_keep_their_own_values_and_repeat_seeds() {
    let core = CorePipelineConfig::new().build().unwrap();
    for (first_cue, first_class, next_cue, second_class) in [
        ("customer ID", "customer_id", "customer ID", "customer_id"),
        ("customer ID", "customer_id", "CUSTOMER ID", "customer_id"),
        (
            "employee ID",
            "employee_id",
            "person record ID",
            "record_id",
        ),
        ("member ID", "record_id", "employeeId", "employee_id"),
        ("Personalnummer", "employee_id", "Kunden-ID", "customer_id"),
    ] {
        for separator in [" ", "; ", "\n"] {
            let first = "FABLE984321";
            let second = "VIOLET976543";
            let input = format!(
                "{first_cue}: {first}{separator}{next_cue}={second}; later lookup {second}"
            );
            let session = Session::new(Scope::Ephemeral).unwrap();
            let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap()
            else {
                panic!("text")
            };
            let entries = session.snapshot_entries();
            assert_eq!(
                entries.len(),
                2,
                "field boundary swallowed: {input:?} -> {clean:?}"
            );
            let a = entries
                .iter()
                .find(|entry| entry.raw == first)
                .expect("first owned value");
            let b = entries
                .iter()
                .find(|entry| entry.raw == second)
                .expect("second owned value");
            assert_eq!(a.class, PiiClass::custom(first_class).unwrap());
            assert_eq!(b.class, PiiClass::custom(second_class).unwrap());
            assert_eq!(
                clean,
                format!(
                    "{first_cue}: {}{separator}{next_cue}={}; later lookup {}",
                    a.token, b.token, b.token
                )
            );
            assert_eq!(
                core.pipeline()
                    .restore_strict_text(&session, &clean)
                    .unwrap(),
                input
            );
        }
    }
}

#[test]
fn public_assignments_and_word_only_schema_values_stay_outside_personal_fields() {
    let core = CorePipelineConfig::new().build().unwrap();
    for cue in ["customer ID", "employee ID", "member ID"] {
        for key in [
            "ORDER_ID",
            "version",
            "sensorAlias",
            "InventarCode",
            "PUBLIC KEY",
        ] {
            let input = format!("{cue}: FABLE984321 {key}=BENCH976543");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap()
            else {
                panic!("text")
            };
            let entries = session.snapshot_entries();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].raw, "FABLE984321");
            assert_eq!(
                clean,
                format!("{cue}: {} {key}=BENCH976543", entries[0].token)
            );
        }
        for value in ["FABLE VIOLET", "token", "", "\"\""] {
            let input = format!("{cue}: {value}; ORDER_ID=BENCH976543");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap()
            else {
                panic!("text")
            };
            assert_eq!(clean, input);
            assert!(session.snapshot_entries().is_empty());
        }
    }
}

#[test]
fn bounded_placeholders_keep_typed_veto_evidence_and_leave_next_cues_visible() {
    let core = CorePipelineConfig::new().build().unwrap();
    for (cue, class) in [
        ("customer ID", "customer_id"),
        ("employee ID", "employee_id"),
        ("member ID", "record_id"),
    ] {
        let input = format!("{cue}: FABLE VIOLET; {cue}=LILAC984321");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (clean, spans, _, trace, events) = core
            .pipeline()
            .clean_text_with_safety_net_policy_detect_context_and_protection_evidence(
                &session,
                &input,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        let CleanDocument::Text(clean) = clean else {
            panic!("text")
        };
        assert_eq!(spans.len(), 1);
        assert_eq!(&input[spans[0].raw_span.clone()], "LILAC984321");
        assert_eq!(trace[0].source_ids(), &[format!("{class}.labelled")]);
        let veto = events
            .iter()
            .find(|event| {
                matches!(
                    event.outcome,
                    gaze::CandidateOutcome::Vetoed {
                        reason: gaze_types::ValidatorFailReason::RegexGuardRejected
                    }
                )
            })
            .expect("placeholder guard remains auditable");
        assert_eq!(veto.recognizer_id, format!("{class}.labelled"));
        assert_eq!(&input[veto.raw_span.clone()], "FABLE VIOLET");
        let entry = &session.snapshot_entries()[0];
        assert_eq!(clean, format!("{cue}: FABLE VIOLET; {cue}={}", entry.token));
    }
}

#[test]
fn labelled_person_ids_capture_only_whole_values_and_restore() {
    let core = CorePipelineConfig::new().build().unwrap();
    for (input, value, class) in [
        (
            r#"{"customer_id":"CUST-000123"}"#,
            "CUST-000123",
            "customer_id",
        ),
        (r#"{"customer_id":600123}"#, "600123", "customer_id"),
        (r#"{"employee_id":600124}"#, "600124", "employee_id"),
        (r#"{"contact_record_id":600125}"#, "600125", "record_id"),
        ("Kundennummer: K-12345", "K-12345", "customer_id"),
        ("customer number is A7-B8", "A7-B8", "customer_id"),
        ("employee_no=EMP-000123", "EMP-000123", "employee_id"),
        ("customer_record_id=C-000123", "C-000123", "customer_id"),
        ("employee_record_id=E-000123", "E-000123", "employee_id"),
        ("Personalnummer ist P-0042", "P-0042", "employee_id"),
        (r#"{"employeeId":"000123"}"#, "000123", "employee_id"),
        ("Mitarbeiter-ID: staff7", "staff7", "employee_id"),
        ("member_id: MEM-000123", "MEM-000123", "record_id"),
        ("Mitgliedsnummer: M-12345", "M-12345", "record_id"),
        ("person_record_id=PER-000123", "PER-000123", "record_id"),
        ("Personendatensatz-ID: SYN-01234", "SYN-01234", "record_id"),
        ("Kontaktdatensatz-ID: SYN-05678", "SYN-05678", "record_id"),
        (
            r#"{"contactId":"00000000-0000-4000-8000-000000000123"}"#,
            "00000000-0000-4000-8000-000000000123",
            "record_id",
        ),
        (
            r#"{"crm":{"customer_id":"AB12 CD34 EF56"},"amount":12.50}"#,
            "AB12 CD34 EF56",
            "customer_id",
        ),
        (r#"{\"customer_id\":\"C-12345\"}"#, "C-12345", "customer_id"),
        (
            "customer_id=C-12345\nemployee_id=E-12345",
            "C-12345",
            "customer_id",
        ),
        (
            "customer_id: abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
            "customer_id",
        ),
        ("customer_id: AB/12_34.56", "AB/12_34.56", "customer_id"),
        (
            r#"{"customer_id":"AB\/12_34.56"}"#,
            r"AB\/12_34.56",
            "customer_id",
        ),
        ("customer_id=C+000123", "C+000123", "customer_id"),
        ("customer_id=+YQ==", "+YQ==", "customer_id"),
        (
            r#"{"employee_id":"opaque+YQ=="}"#,
            "opaque+YQ==",
            "employee_id",
        ),
        (
            "employee_id=_opaque/REF-01",
            "_opaque/REF-01",
            "employee_id",
        ),
        ("customer_id: SYN-é000123", "SYN-é000123", "customer_id"),
        ("customer_id: SYN-é12/34.56", "SYN-é12/34.56", "customer_id"),
        (
            r#"{"customer_id":"C\u002d000123"}"#,
            r"C\u002d000123",
            "customer_id",
        ),
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = core.pseudonymize_text(&session, input).unwrap() else {
            panic!("text")
        };
        assert!(
            !clean.contains(value),
            "value leaked: {input:?} -> {clean:?}"
        );
        assert!(
            clean.contains(&format!(":Custom:{class}_")),
            "wrong class: {clean:?}"
        );
        assert!(
            clean.starts_with(input.split(value).next().unwrap()),
            "label changed: {clean:?}"
        );
        assert!(
            session.snapshot_entries().iter().any(|entry| {
                entry.raw == value && entry.class == PiiClass::custom(class).unwrap()
            }),
            "manifest did not retain the complete typed original: {input:?}"
        );
        // Replacing the emitted token with the entire expected value must preserve
        // both label and suffix. Merely checking contains(value) misses prefix leaks.
        if !input.contains("\nemployee_id=") {
            let start = clean.find('<').unwrap();
            let end = start + clean[start..].find('>').unwrap() + 1;
            assert_eq!(
                format!("{}{}{}", &clean[..start], value, &clean[end..]),
                input
            );
        }
        assert_eq!(
            core.pipeline()
                .restore_strict_text(&session, &clean)
                .unwrap(),
            input
        );
    }
}

#[test]
fn rejected_placeholders_do_not_consume_later_person_id_cues() {
    let core = CorePipelineConfig::new().build().unwrap();
    for (placeholder, key, class) in [
        ("customer ID token", "CUSTOMER_ID", "customer_id"),
        ("customer ID token", "customer_id", "customer_id"),
        ("employee ID placeholder", "EMPLOYEE_ID", "employee_id"),
        ("employee ID placeholder", "employee_id", "employee_id"),
        ("contact record ID value", "CONTACT_RECORD_ID", "record_id"),
        ("contact record ID value", "contact_record_id", "record_id"),
    ] {
        let input = format!("{placeholder} {key}=801234");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = core.pseudonymize_text(&session, &input).unwrap() else {
            panic!("text")
        };
        assert!(!clean.contains("801234"), "later value leaked: {input:?}");
        assert!(clean.starts_with(&format!("{placeholder} {key}=")));
        let entries = session.snapshot_entries();
        assert_eq!(entries.len(), 1, "placeholder must stay raw: {input:?}");
        assert_eq!(entries[0].raw, "801234");
        assert_eq!(entries[0].class, PiiClass::custom(class).unwrap());
        assert_eq!(
            core.pipeline()
                .restore_strict_text(&session, &clean)
                .unwrap(),
            input
        );

        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = core.pseudonymize_text(&session, placeholder).unwrap()
        else {
            panic!("text")
        };
        assert_eq!(clean, placeholder);
        assert!(session.snapshot_entries().is_empty());
    }
}

#[test]
fn public_identifiers_lookalike_keys_and_cross_line_cues_stay_raw() {
    let core = CorePipelineConfig::new().build().unwrap();
    for input in [
        "order_id=ORD-000123",
        "invoice_id=INV-000123",
        "ticket_id=TKT-000123",
        "issue_id=ISS-000123",
        "build_id=BUILD-000123",
        "commit_id=abcdef0123456789",
        "tracking_id=TRACK-000123",
        "record_id=REC-000123",
        "unique_id=U-000123",
        r#"{"id":"00000000-0000-4000-8000-000000000123"}"#,
        "version=1.2.3",
        "amount=123.45",
        "customer_count=123",
        "employee_salary=123.45",
        "notcustomer_id=C-000123",
        "customer_id_suffix=C-000123",
        "order_customer_id=C-000123",
        "employee_number_of_orders=123",
        "customer id:\nBUILD-000123",
        "employee number:\n123",
        "member id:\n123",
        "CUST-000123",
        "EMP-000123",
        r#"{"customer_id":null,"employeeId":true,"contact_record_id":false}"#,
        r#"{"customer_id":true,"employeeId":false,"contact_record_id":null}"#,
        r#"{"customer_id":false,"employeeId":null,"contact_record_id":true}"#,
        r#"[session A "customer ID" token]"#,
        r#"The "employee ID" placeholder is pending."#,
        r#"The "contact record ID" value is pending."#,
        r#"Die "Kundennummer" Platzhalter ist ausstehend."#,
        r#"Die "Personalnummer" Wert ist ausstehend."#,
        r#"Die "Personendatensatz-ID" Kennung ist ausstehend."#,
        "[session A customer ID token]",
        "customer ID placeholder",
        "customer number: example",
        "employee ID value",
        "Personalnummer: Platzhalter",
        "member ID token",
        "contact record ID identifier",
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = core.pseudonymize_text(&session, input).unwrap() else {
            panic!("text")
        };
        assert_eq!(clean, input, "unlinked ID was protected");
    }
}

#[test]
fn alphabetic_and_symbol_only_application_keys_are_public_placeholders() {
    let core = CorePipelineConfig::new().build().unwrap();
    for input in [
        r#"{"customerId":"opaquevalue"}"#,
        r#"{"employee_id":"opaquevalue"}"#,
        r#"{"contact_record_id":"opaquevalue"}"#,
        r#"{\"employee_id\":\"opaquevalue\"}"#,
        r#"{"customer_id":"indigo crystal"}"#,
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = core.pseudonymize_text(&session, input).unwrap() else {
            panic!("text")
        };
        assert_eq!(clean, input);
        assert!(session.snapshot_entries().is_empty());
    }
}
