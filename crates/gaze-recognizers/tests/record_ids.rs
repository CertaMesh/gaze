//! Synthetic CRM/HR identifiers. No shape alone proves a person linkage.
use gaze::{CleanDocument, PiiClass, Scope, Session};
use gaze_assembly::CorePipelineConfig;

#[test]
fn labelled_person_ids_capture_only_whole_values_and_restore() {
    let core = CorePipelineConfig::new().build().unwrap();
    for (input, value, class) in [
        (
            r#"{"customer_id":"CUST-000123"}"#,
            "CUST-000123",
            "customer_id",
        ),
        (
            r#"{"customerId":"opaquevalue"}"#,
            "opaquevalue",
            "customer_id",
        ),
        (r#"{"customer_id":600123}"#, "600123", "customer_id"),
        (r#"{"employee_id":600124}"#, "600124", "employee_id"),
        (r#"{"contact_record_id":600125}"#, "600125", "record_id"),
        (
            r#"{"employee_id":"opaquevalue"}"#,
            "opaquevalue",
            "employee_id",
        ),
        (
            r#"{"contact_record_id":"opaquevalue"}"#,
            "opaquevalue",
            "record_id",
        ),
        (
            r#"{\"employee_id\":\"opaquevalue\"}"#,
            "opaquevalue",
            "employee_id",
        ),
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
            "employee_id=_opaque/REF-01",
            "_opaque/REF-01",
            "employee_id",
        ),
        (
            r#"{"employee_id":"opaque+YQ=="}"#,
            "opaque+YQ==",
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
