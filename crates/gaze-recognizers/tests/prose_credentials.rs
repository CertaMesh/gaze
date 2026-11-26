//! Prose credential disclosures and base64-encoded private keys. Values are invented and
//! cannot authenticate anywhere; the key bodies are filler text, not key material.
use gaze::{DictionaryBundle, LocaleTag, SafetyNetPolicy, Scope, Session};
use gaze_assembly::CorePipelineConfig;

fn core() -> gaze_assembly::CorePipeline {
    CorePipelineConfig::new()
        .with_bundled_rulepack("secrets")
        .with_locale(&[LocaleTag::parse("de-DE").unwrap()])
        .build()
        .unwrap()
}

/// The credential spans of `raw` in source order, after a strict restore round trip.
fn credentials(raw: &str) -> Vec<(String, String)> {
    let core = core();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, spans, _, _) = core
        .pipeline()
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            raw,
            &[LocaleTag::Global, LocaleTag::parse("de-DE").unwrap()],
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
        .filter_map(|span| match &span.class {
            gaze::PiiClass::Custom(class) if class == "password" || class == "security_token" => {
                Some((class.clone(), raw[span.raw_span.clone()].to_string()))
            }
            _ => None,
        })
        .collect()
}

fn assert_credential(raw: &str, class: &str, value: &str) {
    assert_eq!(
        credentials(raw),
        [(class.to_string(), value.to_string())],
        "{raw:?}"
    );
}

fn assert_no_credential(raw: &str) {
    assert_eq!(credentials(raw), [], "{raw:?}");
}

#[test]
fn a_password_named_in_prose_is_one_span_without_its_sentence_punctuation() {
    for (raw, value) in [
        (
            "I reset my password to Lantern#5821 as the old one expired.",
            "Lantern#5821",
        ),
        ("Das neue Passwort lautet: Fjord7?Tal.", "Fjord7?Tal"),
        ("Her account password is Quill9204!", "Quill9204!"),
        (
            "Mein Passwort für das Kundenportal lautet \"Birke!7740\", bitte merken.",
            "Birke!7740",
        ),
        (
            "Das Passwort für den Admin-Bereich ist immer noch 'Kompass_381'.",
            "Kompass_381",
        ),
        (
            "He set the WLAN-Passwort to Moos4471 yesterday.",
            "Moos4471",
        ),
        (
            "The temporary passphrase, which you chose, was 'nine lamps 42'.",
            "nine lamps 42",
        ),
        (
            "Bitte verwenden Sie das Passwort Wald&Weg93, um sich anzumelden.",
            "Wald&Weg93",
        ),
    ] {
        assert_credential(raw, "password", value);
    }
}

#[test]
fn password_words_without_a_credential_value_stay_raw() {
    for raw in [
        "Your password must contain at least 12 characters and one digit.",
        "Use your password to log in, then open the reset-link.",
        "Klicken Sie auf \"Passwort vergessen?\" und folgen Sie dem Link.",
        "Das Passwort ist abgelaufen und muss erneuert werden!",
        "For a new password, visit https://example.invalid/reset now.",
        "The password for the account was sent to anna@example.invalid today.",
        "Password policy (version 2 applies): rotate every 90 days.",
        "Your password: ********",
        "'password' => Hash::make('secret'),",
    ] {
        assert_no_credential(raw);
    }
}

#[test]
fn a_security_token_or_code_behind_a_security_cue_is_one_span() {
    for (raw, value) in [
        (
            "Ihr Sicherheitstoken, der Code QX7-PLM4-TR2, wurde bestätigt.",
            "QX7-PLM4-TR2",
        ),
        (
            "Der Sicherheitscode für den Zugang zum Portal lautet B7KD29QXM.",
            "B7KD29QXM",
        ),
        (
            "Please confirm the security token Z4QW8RT2PL for the transfer.",
            "Z4QW8RT2PL",
        ),
        (
            "Sein Security-Token lautet 5RGQ8WX2VN, sagte er.",
            "5RGQ8WX2VN",
        ),
        ("The access code is \"k3j9-ww2p-x7\".", "k3j9-ww2p-x7"),
        (
            "Your verification code is 482913 and expires in 10 minutes.",
            "482913",
        ),
        ("Hier der Einmal-Sicherheitscode: 1432154", "1432154"),
    ] {
        assert_credential(raw, "security_token", value);
    }
}

#[test]
fn short_numbers_and_words_behind_a_security_cue_stay_raw() {
    for raw in [
        "Der Sicherheitscode auf der Rückseite der Karte hat 3 Ziffern: 123.",
        "Der Sicherheitscode Ihrer Karte lautet 4821.",
        "The access code is 123456789 according to the ledger.",
        "The tokenizer cl100k_base counts 2048 tokens per request.",
        "Security token rotation is scheduled for next Monday.",
        "Session token expired at 12:30:45 on the gateway.",
        "API key v2 endpoints are documented below.",
        "API key name: ci-deploy-07",
        "API key rotation job rotate-keys-2026 finished.",
        "Der Sicherheitscode-Hinweis für Konto SEC-4821 wurde entfernt.",
    ] {
        assert_no_credential(raw);
    }
}

const PRIVATE_PEM: &str = "-----BEGIN RSA PRIVATE KEY-----\nAAAAfillerfillerfillerfillerfillerfillerfillerfillerfiller0123\nBBBBfillerfillerfillerfillerfillerfillerfillerfiller4567\n-----END RSA PRIVATE KEY-----\n";

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let word = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(ALPHABET[(word >> (18 - 6 * index)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn a_base64_encoded_private_key_is_protected_whole_at_every_alignment() {
    for prefix in ["", "k", "ke"] {
        let blob = base64(format!("{prefix}{PRIVATE_PEM}").as_bytes());
        let raw = format!("data:\n  tls.key: {blob}\n  tls.crt: none\n");
        assert_credential(&raw, "security_token", &blob);
        assert_credential(
            &format!("SIGNING_KEY_B64={blob}\n"),
            "security_token",
            &blob,
        );
    }
    let blob = base64(PRIVATE_PEM.as_bytes());
    let wrapped = blob
        .as_bytes()
        .chunks(76)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(wrapped.contains('\n'));
    assert_credential(
        &format!("{wrapped}\nkind: Secret\n"),
        "security_token",
        &wrapped,
    );
    assert_credential(
        &format!("{{\"tls.key\":\"{}\"}}", blob.replace('/', "\\/")),
        "security_token",
        &blob.replace('/', "\\/"),
    );
}

#[test]
fn a_base64_encoded_public_certificate_stays_raw() {
    let certificate = PRIVATE_PEM
        .replace("RSA PRIVATE KEY", "CERTIFICATE")
        .replace("AAAA", "MIIB");
    assert_no_credential(&format!("tls.crt: {}\n", base64(certificate.as_bytes())));
}
