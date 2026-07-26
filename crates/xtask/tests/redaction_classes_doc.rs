use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use gaze::{PiiClass, RawMatch, Rulepack, RulepackSource};
use gaze_recognizers::{NormalizerKind, SafetyTier, ValidatorKind};
use syn::{Expr, Item, Pat, Stmt};

const DOC_PATH: &str = "docs/reference/redaction-classes.md";
const CORE_RULEPACK: &str = "core";
const CORE_EXTENDED_RULEPACK: &str = "core-extended";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RecognizerFields {
    id: String,
    matcher: String,
    class: String,
    locales: String,
    validator: String,
    normalizer: String,
    safety_tier: String,
    safe_default: String,
    base_score: String,
    priority: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RecognizerRow {
    bundles: String,
    fields: RecognizerFields,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ClassRow {
    variant: String,
    policy_name: String,
    priority: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct KindRow {
    variant: String,
    rulepack_names: BTreeSet<String>,
    feature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CollisionRow {
    family: String,
    recognizer_id: String,
    variant: String,
    precedence: String,
    mandatory_anchor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DefaultActivationRow {
    selection: String,
    locale_chain: String,
    auto_locale_gated: String,
    recognizer_ids: BTreeSet<String>,
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn read_workspace_file(relative: &str) -> String {
    let path = workspace_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn table_rows(document: &str, section: &str, expected_columns: usize) -> Vec<Vec<String>> {
    let start_marker = format!("<!-- redaction-classes-gate:{section}:start -->");
    let end_marker = format!("<!-- redaction-classes-gate:{section}:end -->");
    let (_, after_start) = document
        .split_once(&start_marker)
        .unwrap_or_else(|| panic!("missing start marker {start_marker} in {DOC_PATH}"));
    let (table, _) = after_start
        .split_once(&end_marker)
        .unwrap_or_else(|| panic!("missing end marker {end_marker} in {DOC_PATH}"));

    table
        .lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .filter_map(|line| {
            let cells = line
                .trim()
                .trim_matches('|')
                .split('|')
                .map(clean_cell)
                .collect::<Vec<_>>();
            if cells
                .iter()
                .all(|cell| cell.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '))
            {
                return None;
            }
            assert_eq!(
                cells.len(),
                expected_columns,
                "unexpected column count in {section} row: {line}"
            );
            Some(cells)
        })
        .skip(1)
        .collect()
}

fn clean_cell(cell: &str) -> String {
    let trimmed = cell.trim();
    if trimmed.starts_with('`') && trimmed.ends_with('`') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

fn class_name(class: &PiiClass) -> String {
    match class {
        PiiClass::Email => "Email".to_string(),
        PiiClass::Name => "Name".to_string(),
        PiiClass::Location => "Location".to_string(),
        PiiClass::Organization => "Organization".to_string(),
        PiiClass::Custom(name) => format!("custom:{name}"),
    }
}

fn matcher_name(matcher: &RawMatch) -> &'static str {
    match matcher {
        RawMatch::Regex { .. } => "regex",
        RawMatch::Dictionary { .. } => "dictionary",
        RawMatch::Ner { .. } => "ner",
        RawMatch::AnchoredMatch { .. } => "anchored_match",
        _ => panic!("new RawMatch variant must be documented"),
    }
}

fn recognizer_fields(recognizer: &gaze::RecognizerSpec) -> RecognizerFields {
    if let Some(validator) = &recognizer.validator {
        ValidatorKind::parse(&validator.kind).unwrap_or_else(|error| {
            panic!(
                "loaded recognizer {} has unsupported validator {}: {error}",
                recognizer.id, validator.kind
            )
        });
    }
    if let Some(normalizer) = &recognizer.normalizer {
        NormalizerKind::parse(&normalizer.kind).unwrap_or_else(|error| {
            panic!(
                "loaded recognizer {} has unsupported normalizer {}: {error}",
                recognizer.id, normalizer.kind
            )
        });
    }

    RecognizerFields {
        id: recognizer.id.clone(),
        matcher: matcher_name(&recognizer.matcher).to_string(),
        class: class_name(&recognizer.class),
        locales: recognizer
            .locales
            .iter()
            .map(|locale| locale.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        validator: recognizer
            .validator
            .as_ref()
            .map_or_else(|| "none".to_string(), |validator| validator.kind.clone()),
        normalizer: recognizer
            .normalizer
            .as_ref()
            .map_or_else(|| "none".to_string(), |normalizer| normalizer.kind.clone()),
        safety_tier: recognizer.safety_tier.as_str().to_string(),
        safe_default: if recognizer.safety_tier == SafetyTier::SafeDefault {
            "yes"
        } else {
            "no"
        }
        .to_string(),
        base_score: format!("{:.2}", recognizer.scoring.base),
        priority: recognizer.scoring.priority.to_string(),
    }
}

fn loaded_rulepack(name: &str) -> Rulepack {
    Rulepack::load(RulepackSource::Embedded(
        gaze_recognizers::embedded(name)
            .unwrap_or_else(|| panic!("embedded rulepack {name} must exist")),
    ))
    .unwrap_or_else(|error| panic!("embedded rulepack {name} must load: {error}"))
}

fn expected_recognizer_rows() -> BTreeSet<RecognizerRow> {
    let mut bundles_by_fields = BTreeMap::<RecognizerFields, BTreeSet<String>>::new();
    for bundle in [CORE_RULEPACK, CORE_EXTENDED_RULEPACK] {
        for recognizer in loaded_rulepack(bundle).recognizers {
            bundles_by_fields
                .entry(recognizer_fields(&recognizer))
                .or_default()
                .insert(bundle.to_string());
        }
    }

    bundles_by_fields
        .into_iter()
        .map(|(fields, bundles)| RecognizerRow {
            bundles: bundles.into_iter().collect::<Vec<_>>().join(", "),
            fields,
        })
        .collect()
}

fn documented_recognizer_rows(document: &str) -> BTreeSet<RecognizerRow> {
    table_rows(document, "recognizers", 13)
        .into_iter()
        .map(|cells| {
            assert!(
                !cells[3].is_empty() && cells[3] != "none",
                "recognizer {} must have a prose match description",
                cells[1]
            );
            RecognizerRow {
                bundles: cells[0].clone(),
                fields: RecognizerFields {
                    id: cells[1].clone(),
                    matcher: cells[2].clone(),
                    class: cells[4].clone(),
                    locales: cells[5].clone(),
                    validator: cells[6].clone(),
                    normalizer: cells[7].clone(),
                    safety_tier: cells[8].clone(),
                    safe_default: cells[9].clone(),
                    base_score: cells[10].clone(),
                    priority: cells[11].clone(),
                },
            }
        })
        .collect()
}

fn parse_rust_file(relative: &str) -> syn::File {
    let raw = read_workspace_file(relative);
    syn::parse_file(&raw).unwrap_or_else(|error| panic!("parse Rust source {relative}: {error}"))
}

fn enum_variants(relative: &str, enum_name: &str) -> BTreeMap<String, String> {
    let syntax = parse_rust_file(relative);
    let item = syntax
        .items
        .iter()
        .find_map(|item| match item {
            Item::Enum(item) if item.ident == enum_name => Some(item),
            _ => None,
        })
        .unwrap_or_else(|| panic!("enum {enum_name} not found in {relative}"));

    item.variants
        .iter()
        .map(|variant| {
            let feature = variant
                .attrs
                .iter()
                .find_map(|attribute| {
                    if !attribute.path().is_ident("cfg") {
                        return None;
                    }
                    match &attribute.meta {
                        syn::Meta::List(list) => {
                            let tokens = list.tokens.to_string();
                            tokens
                                .contains("phone-parser")
                                .then(|| "phone-parser".to_string())
                        }
                        _ => None,
                    }
                })
                .unwrap_or_else(|| "always".to_string());
            (variant.ident.to_string(), feature)
        })
        .collect()
}

fn class_priorities() -> BTreeMap<String, String> {
    let relative = "crates/gaze/src/resolver.rs";
    let syntax = parse_rust_file(relative);
    let function = syntax
        .items
        .iter()
        .find_map(|item| match item {
            Item::Fn(item) if item.sig.ident == "class_priority" => Some(item),
            _ => None,
        })
        .unwrap_or_else(|| panic!("class_priority not found in {relative}"));
    let match_expression = function
        .block
        .stmts
        .iter()
        .find_map(|statement| match statement {
            Stmt::Expr(Expr::Match(expression), _) => Some(expression),
            _ => None,
        })
        .expect("class_priority must contain a match expression");

    match_expression
        .arms
        .iter()
        .map(|arm| {
            let variant = match &arm.pat {
                Pat::Path(path) => path
                    .path
                    .segments
                    .last()
                    .expect("class path segment")
                    .ident
                    .to_string(),
                Pat::TupleStruct(tuple) => tuple
                    .path
                    .segments
                    .last()
                    .expect("custom class path segment")
                    .ident
                    .to_string(),
                _ => panic!("unsupported class_priority pattern"),
            };
            let priority = match arm.body.as_ref() {
                Expr::Lit(literal) => match &literal.lit {
                    syn::Lit::Int(integer) => integer.base10_digits().to_string(),
                    _ => panic!("class priority must be an integer"),
                },
                _ => panic!("class priority must be a literal"),
            };
            (variant, priority)
        })
        .collect()
}

fn expected_class_rows() -> BTreeSet<ClassRow> {
    let variants = enum_variants("crates/gaze-types/src/lib.rs", "PiiClass");
    let priorities = class_priorities();
    assert_eq!(
        variants.keys().cloned().collect::<BTreeSet<_>>(),
        priorities.keys().cloned().collect::<BTreeSet<_>>(),
        "PiiClass variants and resolver class_priority arms must stay exhaustive"
    );

    let actual_policy_pairs = [
        (PiiClass::Email, "Email", "email"),
        (PiiClass::Name, "Name", "name"),
        (PiiClass::Location, "Location", "location"),
        (PiiClass::Organization, "Organization", "organization"),
        (PiiClass::custom("reference"), "Custom", "custom:<name>"),
    ];
    actual_policy_pairs
        .into_iter()
        .map(|(actual, variant, policy_name)| {
            let parsed = if variant == "Custom" {
                PiiClass::from_policy_name("custom:reference")
            } else {
                PiiClass::from_policy_name(policy_name)
            };
            assert_eq!(
                parsed.as_ref(),
                Some(&actual),
                "documented PiiClass spelling must parse to the real enum"
            );
            ClassRow {
                variant: variant.to_string(),
                policy_name: policy_name.to_string(),
                priority: priorities
                    .get(variant)
                    .unwrap_or_else(|| panic!("missing class priority for {variant}"))
                    .clone(),
            }
        })
        .collect()
}

fn documented_class_rows(document: &str) -> BTreeSet<ClassRow> {
    table_rows(document, "pii-classes", 4)
        .into_iter()
        .map(|cells| ClassRow {
            variant: cells[0].clone(),
            policy_name: cells[1].clone(),
            priority: cells[2].clone(),
        })
        .collect()
}

fn validator_variant_name(kind: ValidatorKind) -> &'static str {
    match kind {
        ValidatorKind::EmailRfc => "EmailRfc",
        ValidatorKind::E164Phone => "E164Phone",
        ValidatorKind::E164PhoneNational(_) => "E164PhoneNational",
        ValidatorKind::Luhn => "Luhn",
        ValidatorKind::IbanMod97 => "IbanMod97",
        ValidatorKind::Ipv4Parse => "Ipv4Parse",
        ValidatorKind::Ipv6Parse => "Ipv6Parse",
        ValidatorKind::EthEip55 => "EthEip55",
        ValidatorKind::AadhaarVerhoeff => "AadhaarVerhoeff",
        ValidatorKind::FrNirMod97 => "FrNirMod97",
        ValidatorKind::DeSteuerIdMod1110 => "DeSteuerIdMod1110",
        ValidatorKind::BsnMod11 => "BsnMod11",
        ValidatorKind::CpfMod11 => "CpfMod11",
        ValidatorKind::CnpjMod11 => "CnpjMod11",
        ValidatorKind::UkNhsMod11 => "UkNhsMod11",
        _ => panic!("new ValidatorKind variant must be documented"),
    }
}

fn normalizer_variant_name(kind: NormalizerKind) -> &'static str {
    match kind {
        NormalizerKind::EmailCanonical => "EmailCanonical",
        NormalizerKind::IbanCanonical => "IbanCanonical",
        _ => panic!("new NormalizerKind variant must be documented"),
    }
}

fn documented_kind_rows(document: &str, section: &str) -> BTreeSet<KindRow> {
    table_rows(document, section, 5)
        .into_iter()
        .map(|cells| KindRow {
            variant: cells[0].clone(),
            rulepack_names: cells[1]
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            feature: cells[2].clone(),
        })
        .collect()
}

fn assert_validator_rows(document: &str) {
    let documented = documented_kind_rows(document, "validators");
    let syntax_variants = enum_variants("crates/gaze-types/src/lib.rs", "ValidatorKind");
    let documented_variants = documented
        .iter()
        .map(|row| (row.variant.clone(), row.feature.clone()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        documented_variants, syntax_variants,
        "ValidatorKind documentation drifted from the real enum"
    );

    for row in documented {
        assert!(
            !row.rulepack_names.is_empty(),
            "{} must document at least one rulepack name",
            row.variant
        );
        for name in row.rulepack_names {
            let kind = ValidatorKind::parse(&name)
                .unwrap_or_else(|error| panic!("documented validator {name} must parse: {error}"));
            assert_eq!(
                validator_variant_name(kind),
                row.variant,
                "validator rulepack name {name} maps to the wrong real enum variant"
            );
        }
    }
    assert!(
        ValidatorKind::parse("redaction_classes_doc_unknown").is_err(),
        "unknown validator names must fail closed"
    );
}

fn assert_normalizer_rows(document: &str) {
    let documented = documented_kind_rows(document, "normalizers");
    let syntax_variants = enum_variants("crates/gaze-recognizers/src/regex.rs", "NormalizerKind");
    let documented_variants = documented
        .iter()
        .map(|row| (row.variant.clone(), row.feature.clone()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        documented_variants, syntax_variants,
        "NormalizerKind documentation drifted from the real enum"
    );

    for row in documented {
        assert!(
            !row.rulepack_names.is_empty(),
            "{} must document at least one rulepack name",
            row.variant
        );
        for name in row.rulepack_names {
            let kind = NormalizerKind::parse(&name)
                .unwrap_or_else(|error| panic!("documented normalizer {name} must parse: {error}"));
            assert_eq!(
                normalizer_variant_name(kind),
                row.variant,
                "normalizer rulepack name {name} maps to the wrong real enum variant"
            );
        }
    }
    assert!(
        NormalizerKind::parse("redaction_classes_doc_unknown").is_err(),
        "unknown normalizer names must fail closed"
    );
}

fn expected_collision_rows() -> BTreeSet<CollisionRow> {
    loaded_rulepack(CORE_RULEPACK)
        .recognizers
        .into_iter()
        .filter_map(|recognizer| {
            recognizer.collision.map(|collision| CollisionRow {
                family: collision.family,
                recognizer_id: recognizer.id,
                variant: collision.variant,
                precedence: collision.precedence.to_string(),
                mandatory_anchor: collision
                    .mandatory_anchor
                    .unwrap_or_else(|| "none".to_string()),
            })
        })
        .collect()
}

fn documented_collision_rows(document: &str) -> BTreeSet<CollisionRow> {
    table_rows(document, "collisions", 6)
        .into_iter()
        .map(|cells| CollisionRow {
            family: cells[0].clone(),
            recognizer_id: cells[1].clone(),
            variant: cells[2].clone(),
            precedence: cells[3].clone(),
            mandatory_anchor: cells[4].clone(),
        })
        .collect()
}

fn activates(
    recognizer: &gaze::RecognizerSpec,
    active_locales: &[&str],
    auto_locale_gated: bool,
) -> bool {
    if !recognizer.enabled
        || !recognizer
            .locales
            .iter()
            .any(|locale| locale.as_str() == "global" || active_locales.contains(&locale.as_str()))
    {
        return false;
    }

    match recognizer.safety_tier {
        SafetyTier::SafeDefault => true,
        SafetyTier::LocaleGated => {
            auto_locale_gated
                || recognizer.locales.iter().any(|locale| {
                    locale.as_str() != "global" && active_locales.contains(&locale.as_str())
                })
        }
        SafetyTier::OptIn => false,
        _ => panic!("new SafetyTier variant must define default activation"),
    }
}

fn expected_default_activation_rows() -> BTreeSet<DefaultActivationRow> {
    [
        (
            "core",
            vec!["global"],
            false,
            loaded_rulepack(CORE_RULEPACK),
        ),
        (
            "core-extended compatibility alias",
            vec!["global", "en-US", "de-DE", "de-AT", "de-CH"],
            true,
            loaded_rulepack(CORE_EXTENDED_RULEPACK),
        ),
    ]
    .into_iter()
    .map(
        |(selection, locale_chain, auto_locale_gated, rulepack)| DefaultActivationRow {
            selection: selection.to_string(),
            locale_chain: locale_chain.join(", "),
            auto_locale_gated: if auto_locale_gated { "yes" } else { "no" }.to_string(),
            recognizer_ids: rulepack
                .recognizers
                .iter()
                .filter(|recognizer| activates(recognizer, &locale_chain, auto_locale_gated))
                .map(|recognizer| recognizer.id.clone())
                .collect(),
        },
    )
    .collect()
}

fn documented_default_activation_rows(document: &str) -> BTreeSet<DefaultActivationRow> {
    table_rows(document, "default-activation", 5)
        .into_iter()
        .map(|cells| DefaultActivationRow {
            selection: cells[0].clone(),
            locale_chain: cells[1].clone(),
            auto_locale_gated: cells[2].clone(),
            recognizer_ids: cells[3]
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
        })
        .collect()
}

fn assert_sets_equal<T>(documented: BTreeSet<T>, expected: BTreeSet<T>, label: &str)
where
    T: std::fmt::Debug + Ord,
{
    if documented == expected {
        return;
    }
    let missing = expected.difference(&documented).collect::<Vec<_>>();
    let extra = documented.difference(&expected).collect::<Vec<_>>();
    panic!("{label} documentation drift: missing rows {missing:#?}; extra rows {extra:#?}");
}

#[test]
fn redaction_classes_reference_matches_loaded_code() {
    let document = read_workspace_file(DOC_PATH);

    assert_sets_equal(
        documented_class_rows(&document),
        expected_class_rows(),
        "PiiClass",
    );
    assert_sets_equal(
        documented_recognizer_rows(&document),
        expected_recognizer_rows(),
        "embedded recognizer",
    );
    assert_validator_rows(&document);
    assert_normalizer_rows(&document);
    assert_sets_equal(
        documented_collision_rows(&document),
        expected_collision_rows(),
        "collision family",
    );
    assert_sets_equal(
        documented_default_activation_rows(&document),
        expected_default_activation_rows(),
        "default activation",
    );
}

#[test]
fn embedded_alias_and_enum_parsers_are_behavioral_inputs_to_the_gate() {
    let core = gaze_recognizers::embedded(CORE_RULEPACK).expect("embedded core");
    let extended =
        gaze_recognizers::embedded(CORE_EXTENDED_RULEPACK).expect("embedded core-extended");
    assert_eq!(
        core, extended,
        "core-extended must remain a compatibility alias of core"
    );
    assert_eq!(
        loaded_rulepack(CORE_RULEPACK),
        loaded_rulepack(CORE_EXTENDED_RULEPACK),
        "both embedded names must load the same real Rulepack"
    );
    assert_eq!(
        ValidatorKind::parse("email_rfc").expect("real validator"),
        ValidatorKind::EmailRfc
    );
    assert_eq!(
        NormalizerKind::parse("email_canonical").expect("real normalizer"),
        NormalizerKind::EmailCanonical
    );
}

#[test]
fn reference_path_is_inside_the_workspace() {
    assert!(Path::new(&workspace_root().join(DOC_PATH)).is_file());
}
