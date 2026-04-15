//! End-to-end: sanitize → restore → original values reappear.

use ghostwriter::{restore, sanitize, Context, RestoreRequest, SanitizeRequest};

fn ctx_markus() -> Context {
    Context {
        customer_name: Some("Markus Mueller".into()),
        customer_email: Some("mueller.markus@icloud.com".into()),
        customer_phone: Some("+49 151 23456789".into()),
    }
}

#[test]
fn spec_example_roundtrip() {
    let text = "Hi Artistfy, Markus Mueller here. Please resend to mueller.markus@icloud.com. If needed call +49 151 23456789. Alternate email: markus.mueller@example.de";

    let sanitized = sanitize(SanitizeRequest {
        text: text.into(),
        context: ctx_markus(),
    })
    .expect("sanitize");

    assert!(!sanitized.clean_text.contains("Markus Mueller"));
    assert!(!sanitized.clean_text.contains("mueller.markus@icloud.com"));
    assert!(!sanitized.clean_text.contains("+49 151 23456789"));

    let draft = "Hello <CUSTOMER_NAME>, we will resend the files to <CUSTOMER_EMAIL> today. If needed we will contact you at <CUSTOMER_PHONE>.".to_string();

    let restored = restore(RestoreRequest {
        text: draft,
        session_blob: sanitized.session_blob,
    })
    .expect("restore");

    assert_eq!(
        restored.restored_text,
        "Hello Markus Mueller, we will resend the files to mueller.markus@icloud.com today. If needed we will contact you at +49 151 23456789."
    );
}

#[test]
fn paraphrased_draft_leaves_invented_names_alone() {
    let sanitized = sanitize(SanitizeRequest {
        text: "Markus Mueller here".into(),
        context: ctx_markus(),
    })
    .unwrap();

    let restored = restore(RestoreRequest {
        text: "Hello Markus, see attached.".into(),
        session_blob: sanitized.session_blob,
    })
    .unwrap();

    assert_eq!(restored.restored_text, "Hello Markus, see attached.");
    assert!(restored
        .warnings
        .iter()
        .any(|w| w.0.contains("<CUSTOMER_NAME>") && w.0.contains("not used")));
}
