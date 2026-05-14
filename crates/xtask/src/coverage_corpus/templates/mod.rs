#[derive(Debug, Clone)]
pub struct Template {
    pub id: String,
    pub context: String,
    pub locale_chain: Vec<String>,
    pub length_tier: String,
    pub structural_family: String,
    pub body: String,
    pub expected_classes: Vec<String>,
    pub expected_emissions: Vec<ExpectedEmission>,
    pub notes: String,
    pub source_citation: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExpectedEmission {
    pub class_id: String,
    pub recognizer_id: String,
    pub recognizer_version_id: String,
}

#[derive(Debug, Clone, Copy)]
struct ScenarioSpec {
    id: &'static str,
    context: &'static str,
    locale: &'static [&'static str],
    tier: &'static str,
    family: &'static str,
    classes: &'static [&'static str],
    intent: &'static str,
}

const GLOBAL: &[&str] = &["global"];
const EN: &[&str] = &["en-US", "global"];
const DE: &[&str] = &["de-DE", "global"];

const BASE_CONTEXTS: &[(&str, &[&str], &str)] = &[
    ("log-line", GLOBAL, "Log line"),
    ("ocr-output", GLOBAL, "OCR line"),
    ("prose_en_v2", EN, "English prose"),
    ("prose_de_v2", DE, "German prose"),
    ("support_email_de_v2", DE, "German support email"),
    ("tool_json_v2", GLOBAL, "Tool-call JSON"),
];

const BASE_CLASS_SETS: &[&[&str]] = &[
    &["Email", "custom:iban"],
    &["Email", "custom:ip_address"],
    &["custom:phone", "Email"],
    &["custom:credit_card", "Email"],
    &["Email", "custom:phone"],
    &["custom:postal_code", "Email"],
    &["custom:eth_address", "Email"],
    &["Email", "custom:iban", "Email"],
    &["custom:ip_address", "custom:eth_address"],
    &["custom:credit_card", "custom:phone"],
];

const STRUCTURAL_SCENARIOS: &[ScenarioSpec] = &[
    spec(
        "conversational-en-001",
        "conversational-en",
        EN,
        "Snippet",
        "conversational",
        &["Name", "Email"],
        "chat address handoff",
    ),
    spec(
        "conversational-en-002",
        "conversational-en",
        EN,
        "Snippet",
        "conversational",
        &["Email", "custom:phone"],
        "casual phone handoff",
    ),
    spec(
        "conversational-en-003",
        "conversational-en",
        EN,
        "Snippet",
        "conversational",
        &["Name", "custom:postal_code"],
        "shipping cue",
    ),
    spec(
        "conversational-en-004",
        "conversational-en",
        EN,
        "Snippet",
        "conversational",
        &["Email", "custom:ip_address"],
        "agent triage",
    ),
    spec(
        "conversational-en-005",
        "conversational-en",
        EN,
        "Page",
        "conversational",
        &["Name", "Email", "custom:credit_card"],
        "support chat payment",
    ),
    spec(
        "conversational-en-006",
        "conversational-en",
        EN,
        "Page",
        "conversational",
        &["Name", "custom:iban", "custom:ip_address"],
        "banking chat",
    ),
    spec(
        "conversational-en-007",
        "conversational-en",
        EN,
        "Page",
        "conversational",
        &["Email", "custom:eth_address", "Name"],
        "crypto handoff",
    ),
    spec(
        "conversational-en-008",
        "conversational-en",
        EN,
        "Page",
        "conversational",
        &["custom:phone", "custom:postal_code", "Email"],
        "address correction",
    ),
    spec(
        "conversational-en-009",
        "conversational-en",
        EN,
        "MultiPage",
        "conversational",
        &[
            "Name",
            "Email",
            "custom:phone",
            "custom:postal_code",
            "Name",
            "Email",
        ],
        "long support transcript",
    ),
    spec(
        "conversational-en-010",
        "conversational-en",
        EN,
        "MultiPage",
        "conversational",
        &["Email", "custom:iban", "custom:eth_address", "Name"],
        "settlement transcript",
    ),
    spec(
        "conversational-en-011",
        "conversational-en",
        EN,
        "MultiPage",
        "conversational",
        &[
            "Name",
            "Email",
            "custom:credit_card",
            "custom:ip_address",
            "custom:phone",
        ],
        "checkout escalation",
    ),
    spec(
        "conversational-en-012",
        "conversational-en",
        EN,
        "MultiPage",
        "conversational",
        &["Email", "custom:postal_code", "Name", "custom:iban"],
        "account recovery",
    ),
    spec(
        "conversational-de-001",
        "conversational-de",
        DE,
        "Snippet",
        "conversational",
        &["Name", "Email"],
        "German chat cue",
    ),
    spec(
        "conversational-de-002",
        "conversational-de",
        DE,
        "Snippet",
        "conversational",
        &["Email", "custom:phone"],
        "German phone handoff",
    ),
    spec(
        "conversational-de-003",
        "conversational-de",
        DE,
        "Snippet",
        "conversational",
        &["Name", "custom:postal_code"],
        "German address cue",
    ),
    spec(
        "conversational-de-004",
        "conversational-de",
        DE,
        "Snippet",
        "conversational",
        &["Email", "custom:iban"],
        "German bank cue",
    ),
    spec(
        "conversational-de-005",
        "conversational-de",
        DE,
        "Page",
        "conversational",
        &["Name", "custom:iban", "custom:postal_code"],
        "bank chat",
    ),
    spec(
        "conversational-de-006",
        "conversational-de",
        DE,
        "Page",
        "conversational",
        &["Name", "Email", "custom:ip_address"],
        "footer signature",
    ),
    spec(
        "conversational-de-007",
        "conversational-de",
        DE,
        "Page",
        "conversational",
        &["Email", "custom:phone", "Name"],
        "callback planning",
    ),
    spec(
        "conversational-de-008",
        "conversational-de",
        DE,
        "Page",
        "conversational",
        &["custom:credit_card", "Email", "Name"],
        "checkout transcript",
    ),
    spec(
        "conversational-de-009",
        "conversational-de",
        DE,
        "MultiPage",
        "conversational",
        &["Name", "custom:iban", "Email", "Name", "Email"],
        "long German support",
    ),
    spec(
        "conversational-de-010",
        "conversational-de",
        DE,
        "MultiPage",
        "conversational",
        &["Name", "Email", "custom:credit_card", "custom:phone"],
        "payment escalation",
    ),
    spec(
        "conversational-de-011",
        "conversational-de",
        DE,
        "MultiPage",
        "conversational",
        &["Email", "custom:postal_code", "Name", "custom:iban"],
        "address and bank",
    ),
    spec(
        "conversational-de-012",
        "conversational-de",
        DE,
        "MultiPage",
        "conversational",
        &["Name", "Email", "custom:ip_address", "custom:phone"],
        "security review",
    ),
    spec(
        "code-mixed-001",
        "code-mixed",
        GLOBAL,
        "Snippet",
        "code-mixed",
        &["Email", "custom:ip_address"],
        "inline shell",
    ),
    spec(
        "code-mixed-002",
        "code-mixed",
        GLOBAL,
        "Snippet",
        "code-mixed",
        &["custom:iban", "Email"],
        "JSON config",
    ),
    spec(
        "code-mixed-003",
        "code-mixed",
        GLOBAL,
        "Snippet",
        "code-mixed",
        &["custom:credit_card", "Email"],
        "curl command",
    ),
    spec(
        "code-mixed-004",
        "code-mixed",
        GLOBAL,
        "Snippet",
        "code-mixed",
        &["custom:eth_address", "Email"],
        "deployment arg",
    ),
    spec(
        "code-mixed-005",
        "code-mixed",
        GLOBAL,
        "Snippet",
        "code-mixed",
        &["custom:ip_address", "custom:iban"],
        "env var",
    ),
    spec(
        "code-mixed-006",
        "code-mixed",
        GLOBAL,
        "Snippet",
        "code-mixed",
        &["Email", "custom:credit_card"],
        "CLI flag",
    ),
    spec(
        "code-mixed-007",
        "code-mixed",
        GLOBAL,
        "Page",
        "code-mixed",
        &["Email", "Email", "custom:iban", "custom:ip_address"],
        "Python literals",
    ),
    spec(
        "code-mixed-008",
        "code-mixed",
        GLOBAL,
        "Page",
        "code-mixed",
        &["Name", "Email", "custom:eth_address"],
        "contract comment",
    ),
    spec(
        "code-mixed-009",
        "code-mixed",
        GLOBAL,
        "Page",
        "code-mixed",
        &["Email", "custom:credit_card", "custom:phone"],
        "YAML plus prose",
    ),
    spec(
        "code-mixed-010",
        "code-mixed",
        GLOBAL,
        "Page",
        "code-mixed",
        &["custom:ip_address", "custom:ip_address", "Email"],
        "config map",
    ),
    spec(
        "code-mixed-011",
        "code-mixed",
        GLOBAL,
        "Page",
        "code-mixed",
        &["custom:iban", "custom:eth_address", "Email"],
        "migration note",
    ),
    spec(
        "code-mixed-012",
        "code-mixed",
        GLOBAL,
        "Page",
        "code-mixed",
        &["Name", "custom:postal_code", "Email"],
        "SQL comment",
    ),
    spec(
        "code-mixed-013",
        "code-mixed",
        GLOBAL,
        "MultiPage",
        "code-mixed",
        &[
            "Email",
            "Email",
            "custom:iban",
            "custom:iban",
            "custom:ip_address",
        ],
        "compose and readme",
    ),
    spec(
        "code-mixed-014",
        "code-mixed",
        GLOBAL,
        "MultiPage",
        "code-mixed",
        &["Name", "Email", "custom:credit_card", "custom:postal_code"],
        "migration changelog",
    ),
    spec(
        "code-mixed-015",
        "code-mixed",
        GLOBAL,
        "MultiPage",
        "code-mixed",
        &["Email", "custom:iban", "custom:eth_address", "Name"],
        "deployment postmortem",
    ),
    spec(
        "code-mixed-016",
        "code-mixed",
        GLOBAL,
        "MultiPage",
        "code-mixed",
        &[
            "custom:ip_address",
            "Email",
            "custom:phone",
            "custom:credit_card",
        ],
        "incident dump",
    ),
    spec(
        "code-mixed-017",
        "code-mixed",
        GLOBAL,
        "MultiPage",
        "code-mixed",
        &[
            "Email",
            "custom:eth_address",
            "custom:eth_address",
            "custom:iban",
        ],
        "wallet script",
    ),
    spec(
        "code-mixed-018",
        "code-mixed",
        GLOBAL,
        "MultiPage",
        "code-mixed",
        &["Name", "Email", "custom:ip_address", "custom:postal_code"],
        "seed fixture",
    ),
    spec(
        "high-density-en-001",
        "high-density-en",
        EN,
        "Snippet",
        "high-density",
        &["Email", "Email", "custom:phone", "custom:credit_card"],
        "four spans tight",
    ),
    spec(
        "high-density-en-002",
        "high-density-en",
        EN,
        "Snippet",
        "high-density",
        &["Name", "Email", "Name", "Email"],
        "name email pairs",
    ),
    spec(
        "high-density-en-003",
        "high-density-en",
        EN,
        "Snippet",
        "high-density",
        &["custom:postal_code", "custom:phone", "Email", "Name"],
        "contact list",
    ),
    spec(
        "high-density-en-004",
        "high-density-en",
        EN,
        "Page",
        "high-density",
        &[
            "Email",
            "Email",
            "Email",
            "custom:iban",
            "custom:iban",
            "custom:credit_card",
            "Name",
            "Name",
        ],
        "cross-class density",
    ),
    spec(
        "high-density-en-005",
        "high-density-en",
        EN,
        "Page",
        "high-density",
        &[
            "Email",
            "Email",
            "custom:phone",
            "custom:phone",
            "custom:postal_code",
            "custom:postal_code",
        ],
        "US contact density",
    ),
    spec(
        "high-density-en-006",
        "high-density-en",
        EN,
        "Page",
        "high-density",
        &[
            "Name",
            "Name",
            "Email",
            "custom:credit_card",
            "custom:ip_address",
        ],
        "escalation density",
    ),
    spec(
        "high-density-de-001",
        "high-density-de",
        DE,
        "Snippet",
        "high-density",
        &["Email", "Email", "custom:iban", "custom:phone"],
        "German dense snippet",
    ),
    spec(
        "high-density-de-002",
        "high-density-de",
        DE,
        "Snippet",
        "high-density",
        &[
            "custom:postal_code",
            "custom:postal_code",
            "custom:phone",
            "Name",
        ],
        "address fragments",
    ),
    spec(
        "high-density-de-003",
        "high-density-de",
        DE,
        "Snippet",
        "high-density",
        &["Name", "Email", "custom:iban", "Name"],
        "bank line",
    ),
    spec(
        "high-density-de-004",
        "high-density-de",
        DE,
        "Page",
        "high-density",
        &[
            "Email",
            "Email",
            "Email",
            "custom:iban",
            "custom:iban",
            "Email",
            "Email",
            "custom:postal_code",
        ],
        "German roster",
    ),
    spec(
        "high-density-de-005",
        "high-density-de",
        DE,
        "Page",
        "high-density",
        &[
            "custom:iban",
            "custom:iban",
            "custom:iban",
            "Name",
            "Name",
            "Email",
        ],
        "statement density",
    ),
    spec(
        "high-density-de-006",
        "high-density-de",
        DE,
        "Page",
        "high-density",
        &[
            "Email",
            "Email",
            "custom:phone",
            "custom:postal_code",
            "Name",
        ],
        "handoff density",
    ),
    spec(
        "high-density-global-001",
        "high-density-global",
        GLOBAL,
        "Snippet",
        "high-density",
        &[
            "custom:ip_address",
            "custom:ip_address",
            "custom:ip_address",
            "Email",
        ],
        "IP burst",
    ),
    spec(
        "high-density-global-002",
        "high-density-global",
        GLOBAL,
        "Snippet",
        "high-density",
        &[
            "custom:credit_card",
            "custom:iban",
            "custom:credit_card",
            "custom:iban",
        ],
        "payment IDs",
    ),
    spec(
        "high-density-global-003",
        "high-density-global",
        GLOBAL,
        "Snippet",
        "high-density",
        &[
            "custom:eth_address",
            "Email",
            "custom:ip_address",
            "custom:eth_address",
        ],
        "crypto burst",
    ),
    spec(
        "high-density-global-004",
        "high-density-global",
        GLOBAL,
        "Page",
        "high-density",
        &[
            "custom:ip_address",
            "custom:ip_address",
            "custom:ip_address",
            "custom:eth_address",
            "custom:eth_address",
            "Email",
            "Email",
        ],
        "mixer trace",
    ),
    spec(
        "high-density-global-005",
        "high-density-global",
        GLOBAL,
        "Page",
        "high-density",
        &[
            "custom:eth_address",
            "custom:eth_address",
            "custom:credit_card",
            "custom:credit_card",
            "Email",
            "Email",
        ],
        "on-ramp density",
    ),
    spec(
        "high-density-global-006",
        "high-density-global",
        GLOBAL,
        "Page",
        "high-density",
        &[
            "Email",
            "custom:iban",
            "custom:ip_address",
            "custom:credit_card",
            "custom:eth_address",
        ],
        "mixed ledger",
    ),
    spec(
        "distractor-en-001",
        "distractor-en",
        EN,
        "Snippet",
        "distractor-heavy",
        &["Email", "custom:credit_card"],
        "order id noise",
    ),
    spec(
        "distractor-en-002",
        "distractor-en",
        EN,
        "Snippet",
        "distractor-heavy",
        &["custom:phone", "Email"],
        "invalid phone neighbors",
    ),
    spec(
        "distractor-en-003",
        "distractor-en",
        EN,
        "Snippet",
        "distractor-heavy",
        &["Name", "Email"],
        "brand-name noise",
    ),
    spec(
        "distractor-en-004",
        "distractor-en",
        EN,
        "Page",
        "distractor-heavy",
        &["Name", "Email", "custom:phone"],
        "marketing lookalikes",
    ),
    spec(
        "distractor-en-005",
        "distractor-en",
        EN,
        "Page",
        "distractor-heavy",
        &["Email", "Name", "custom:credit_card"],
        "trailing PII triple",
    ),
    spec(
        "distractor-en-006",
        "distractor-en",
        EN,
        "Page",
        "distractor-heavy",
        &["custom:postal_code", "Email", "custom:ip_address"],
        "numbered prose",
    ),
    spec(
        "distractor-de-001",
        "distractor-de",
        DE,
        "Snippet",
        "distractor-heavy",
        &["custom:iban", "Email"],
        "IBAN-like references",
    ),
    spec(
        "distractor-de-002",
        "distractor-de",
        DE,
        "Snippet",
        "distractor-heavy",
        &["Email", "Name"],
        "German names among products",
    ),
    spec(
        "distractor-de-003",
        "distractor-de",
        DE,
        "Snippet",
        "distractor-heavy",
        &["custom:phone", "custom:iban"],
        "invoice numbers",
    ),
    spec(
        "distractor-de-004",
        "distractor-de",
        DE,
        "Page",
        "distractor-heavy",
        &["Name", "custom:postal_code"],
        "ZIP-like noise",
    ),
    spec(
        "distractor-de-005",
        "distractor-de",
        DE,
        "Page",
        "distractor-heavy",
        &["custom:iban", "custom:phone", "Email"],
        "invoice template",
    ),
    spec(
        "distractor-de-006",
        "distractor-de",
        DE,
        "Page",
        "distractor-heavy",
        &["Email", "Name", "custom:postal_code"],
        "German copy",
    ),
    spec(
        "distractor-global-001",
        "distractor-global",
        GLOBAL,
        "Snippet",
        "distractor-heavy",
        &["custom:ip_address", "Email"],
        "version tag noise",
    ),
    spec(
        "distractor-global-002",
        "distractor-global",
        GLOBAL,
        "Snippet",
        "distractor-heavy",
        &["custom:credit_card", "Email"],
        "test card neighbors",
    ),
    spec(
        "distractor-global-003",
        "distractor-global",
        GLOBAL,
        "Snippet",
        "distractor-heavy",
        &["custom:eth_address", "Email"],
        "hex blob noise",
    ),
    spec(
        "distractor-global-004",
        "distractor-global",
        GLOBAL,
        "Page",
        "distractor-heavy",
        &["custom:eth_address", "Email"],
        "non-checksum hex",
    ),
    spec(
        "distractor-global-005",
        "distractor-global",
        GLOBAL,
        "Page",
        "distractor-heavy",
        &["custom:ip_address", "custom:ip_address", "Email"],
        "reserved address mix",
    ),
    spec(
        "distractor-global-006",
        "distractor-global",
        GLOBAL,
        "Page",
        "distractor-heavy",
        &["custom:iban", "custom:credit_card", "Email"],
        "payment lookalikes",
    ),
    spec(
        "threaded-en-001",
        "threaded-en",
        EN,
        "Page",
        "threaded",
        &["Email", "Email", "Name", "Name"],
        "single quote layer",
    ),
    spec(
        "threaded-en-002",
        "threaded-en",
        EN,
        "Page",
        "threaded",
        &["Email", "Email", "Name", "custom:credit_card"],
        "refund thread",
    ),
    spec(
        "threaded-en-003",
        "threaded-en",
        EN,
        "Page",
        "threaded",
        &["Name", "Email", "custom:phone", "Name"],
        "handoff thread",
    ),
    spec(
        "threaded-en-004",
        "threaded-en",
        EN,
        "MultiPage",
        "threaded",
        &["Email", "Email", "Email", "Name", "Name", "custom:phone"],
        "three quote layers",
    ),
    spec(
        "threaded-en-005",
        "threaded-en",
        EN,
        "MultiPage",
        "threaded",
        &["Email", "Email", "Name", "custom:eth_address"],
        "chat export",
    ),
    spec(
        "threaded-en-006",
        "threaded-en",
        EN,
        "MultiPage",
        "threaded",
        &["Name", "Email", "custom:iban", "custom:postal_code"],
        "account thread",
    ),
    spec(
        "threaded-de-001",
        "threaded-de",
        DE,
        "Page",
        "threaded",
        &["Email", "Email", "Name", "Name", "custom:iban"],
        "German reply",
    ),
    spec(
        "threaded-de-002",
        "threaded-de",
        DE,
        "Page",
        "threaded",
        &["Email", "Email", "Name", "custom:phone"],
        "German callback thread",
    ),
    spec(
        "threaded-de-003",
        "threaded-de",
        DE,
        "Page",
        "threaded",
        &["Name", "Email", "custom:postal_code", "Name"],
        "German address thread",
    ),
    spec(
        "threaded-de-004",
        "threaded-de",
        DE,
        "MultiPage",
        "threaded",
        &[
            "Email",
            "Email",
            "Email",
            "Name",
            "Name",
            "custom:iban",
            "custom:postal_code",
        ],
        "deep German thread",
    ),
    spec(
        "threaded-de-005",
        "threaded-de",
        DE,
        "MultiPage",
        "threaded",
        &["Email", "Email", "Name", "custom:iban", "custom:iban"],
        "banking thread",
    ),
    spec(
        "threaded-de-006",
        "threaded-de",
        DE,
        "MultiPage",
        "threaded",
        &["Name", "Email", "custom:phone", "custom:credit_card"],
        "payment thread",
    ),
];

const fn spec(
    id: &'static str,
    context: &'static str,
    locale: &'static [&'static str],
    tier: &'static str,
    family: &'static str,
    classes: &'static [&'static str],
    intent: &'static str,
) -> ScenarioSpec {
    ScenarioSpec {
        id,
        context,
        locale,
        tier,
        family,
        classes,
        intent,
    }
}

pub fn phase_1_templates() -> serde_json::Result<Vec<Template>> {
    let mut templates = Vec::with_capacity(150);
    for (context, locale, label) in BASE_CONTEXTS {
        for (idx, classes) in BASE_CLASS_SETS.iter().enumerate() {
            let id = format!("{context}-v2-{number:03}", number = idx + 1);
            templates.push(build_template(
                &id,
                context,
                locale,
                "Snippet",
                context,
                classes,
                &format!("{label} deliberate replacement scenario {}", idx + 1),
            ));
        }
    }
    templates.extend(STRUCTURAL_SCENARIOS.iter().map(|spec| {
        build_template(
            spec.id,
            spec.context,
            spec.locale,
            spec.tier,
            spec.family,
            spec.classes,
            spec.intent,
        )
    }));
    Ok(templates)
}

fn build_template(
    id: &str,
    context: &str,
    locale: &[&str],
    length_tier: &str,
    structural_family: &str,
    classes: &[&str],
    intent: &str,
) -> Template {
    Template {
        id: id.to_string(),
        context: context.to_string(),
        locale_chain: locale.iter().map(|locale| (*locale).to_string()).collect(),
        length_tier: length_tier.to_string(),
        structural_family: structural_family.to_string(),
        body: scenario_body(
            context,
            locale.first().copied().unwrap_or("global"),
            length_tier,
            classes,
        ),
        expected_classes: unique_classes(classes),
        expected_emissions: classes
            .iter()
            .map(|class_id| {
                expected_emission(class_id, locale.first().copied().unwrap_or("global"))
            })
            .collect(),
        notes: format!("{structural_family} / {length_tier}: {intent}. Synthetic-only fixture."),
        source_citation: Some("synthetic scenario catalog; no real PII source".to_string()),
    }
}

fn unique_classes(classes: &[&str]) -> Vec<String> {
    let mut unique = Vec::new();
    for class_id in classes {
        if !unique.iter().any(|existing| existing == class_id) {
            unique.push((*class_id).to_string());
        }
    }
    unique
}

fn scenario_body(context: &str, locale: &str, tier: &str, classes: &[&str]) -> String {
    let prefix = match context {
        "tool_json_v2" => "{\"tool\":\"handoff\",\"args\":{\"payload\":\"".to_string(),
        "log-line" => "[2026-05-14T08:00:00Z] INFO scenario=".to_string(),
        "ocr-output" => "SCAN 001 / OCR CONF 0.98 / ".to_string(),
        _ if locale == "de-DE" => {
            "Hallo Team, bitte prueft diesen synthetischen Vorgang. ".to_string()
        }
        _ => "Support note for the synthetic review queue. ".to_string(),
    };
    let mut body = prefix;
    match tier {
        "Snippet" => {
            body.push_str(&class_sentence(locale, classes));
            body.push_str(" ref=CASE-7421 status=pending");
        }
        "Page" => {
            body.push_str("The following values are intentionally synthetic and citable. ");
            body.push_str(
                "Distractors: ORD-2026-00017, 9999-0000-1111-2222, v10.20.30, 0100-ABCD. ",
            );
            body.push_str(&class_sentence(locale, classes));
            body.push_str(" The agent must pseudonymize only the labeled spans and preserve the rest of the workflow context.");
        }
        "MultiPage" => {
            body.push_str(
                "Thread section A: the owner repeats context without adding real-world data. ",
            );
            body.push_str(&class_sentence(locale, classes));
            body.push_str(" Section B quotes prior text: > Please carry the same synthetic values through the manifest. ");
            body.push_str("Section C includes operational noise: build=2026.05.14, ticket=QX-771-ALPHA, hash=abcdef0123456789. ");
            body.push_str("Section D closes the loop with restore-sensitive language and no extra unlabeled PII.");
        }
        _ => unreachable!("validated by builder"),
    }
    if context == "tool_json_v2" {
        body.push_str("\"}}");
    }
    body
}

fn class_sentence(locale: &str, classes: &[&str]) -> String {
    let lead = if locale == "de-DE" {
        "Bitte antworte an"
    } else {
        "Please respond to"
    };
    let mut parts = Vec::new();
    for (idx, class_id) in classes.iter().enumerate() {
        if *class_id == "Name" {
            parts.push(format!("reply to {}", placeholder(class_id, idx)));
        } else {
            parts.push(format!(
                "{} {}",
                class_label(class_id, idx),
                placeholder(class_id, idx)
            ));
        }
    }
    format!("{lead} {}.", parts.join("; "))
}

fn class_label(class_id: &str, idx: usize) -> String {
    match class_id {
        "Email" => format!("email_{idx}"),
        "Name" => format!("recipient_{idx}"),
        "custom:phone" => format!("phone_{idx}"),
        "custom:iban" => format!("IBAN {idx}"),
        "custom:credit_card" => format!("card_{idx}"),
        "custom:ip_address" => format!("ip_{idx}"),
        "custom:eth_address" => format!("wallet_{idx}"),
        "custom:postal_code" => format!("postal_{idx}"),
        other => other.replace(':', "_"),
    }
}

fn placeholder(class_id: &str, idx: usize) -> String {
    format!("{{{{{class_id}#{idx}}}}}")
}

fn expected_emission(class_id: &str, locale: &str) -> ExpectedEmission {
    let recognizer_id = match class_id {
        "Email" => "email.global",
        "Name" => "name.agent_recipient",
        "custom:phone" if locale == "de-DE" => "phone.national.de",
        "custom:phone" if locale == "en-US" => "phone.national.us",
        "custom:phone" => "phone.structural",
        "custom:iban" => "iban.structural",
        "custom:credit_card" => "card.structural",
        "custom:ip_address" => "ip.v6",
        "custom:eth_address" => "eth.address",
        "custom:postal_code" if locale == "de-DE" => "postal.de",
        "custom:postal_code" if locale == "en-US" => "postal.us",
        "custom:postal_code" => "postal.us",
        other => other,
    };
    ExpectedEmission {
        class_id: class_id.to_string(),
        recognizer_id: recognizer_id.to_string(),
        recognizer_version_id: format!("{recognizer_id}.v1"),
    }
}
