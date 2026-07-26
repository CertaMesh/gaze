use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use serde::{Deserialize, Serialize};

const GENERATOR_ID: &str = "gaze-en-de-negative-v1";
const LICENSE_ORIGIN: &str = "project-authored-synthetic-CC0-1.0";
const RECORDS_PER_LANGUAGE_CATEGORY: usize = 64;
const LANGUAGES: [&str; 2] = ["en", "de"];
const CATEGORIES: [&str; 8] = [
    "temporal_numeric",
    "documentation_network",
    "code_log_syntax",
    "public_entities",
    "generic_roles",
    "commerce_identifiers",
    "unicode_mixed_language",
    "invalid_identifiers",
];

#[derive(Debug, ClapArgs)]
pub(crate) struct Args {
    /// Seed for deterministic template selection.
    #[arg(long, default_value_t = 0)]
    seed: u64,

    /// Regenerate in memory and compare with the committed corpus without writing.
    #[arg(long)]
    verify: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OracleSpan {
    start: usize,
    end: usize,
    class: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Document {
    id: String,
    generator_id: String,
    seed: u64,
    language: String,
    category: String,
    license_origin: String,
    text: String,
    oracle_spans: Vec<OracleSpan>,
}

pub(crate) fn run(args: Args) -> Result<()> {
    let generated = generate_corpus(args.seed)?;
    let path = corpus_path();
    if args.verify {
        let committed = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if generated != committed {
            bail!("negative corpus differs from deterministic regeneration");
        }
        println!(
            "generate-negative-corpus: verified seed {} (1024 documents; en=512, de=512; 128/category)",
            args.seed
        );
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, generated).with_context(|| format!("failed to write {}", path.display()))?;
    println!(
        "generate-negative-corpus: wrote {} (1024 documents; en=512, de=512; 128/category)",
        path.display()
    );
    Ok(())
}

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/negative_corpus/en_de_negative.jsonl")
}

fn generate_corpus(seed: u64) -> Result<String> {
    let mut rng = SplitMix64::new(seed);
    let mut documents =
        Vec::with_capacity(LANGUAGES.len() * CATEGORIES.len() * RECORDS_PER_LANGUAGE_CATEGORY);

    for language in LANGUAGES {
        for category in CATEGORIES {
            for ordinal in 0..RECORDS_PER_LANGUAGE_CATEGORY {
                documents.push(Document {
                    id: format!("neg-{language}-{}-{ordinal:04}", category.replace('_', "-")),
                    generator_id: GENERATOR_ID.to_owned(),
                    seed,
                    language: language.to_owned(),
                    category: category.to_owned(),
                    license_origin: LICENSE_ORIGIN.to_owned(),
                    text: render_text(language, category, ordinal, &mut rng)?,
                    oracle_spans: Vec::new(),
                });
            }
        }
    }

    validate_documents(&documents)?;
    let mut corpus = String::new();
    for document in documents {
        corpus.push_str(&serde_json::to_string(&document)?);
        corpus.push('\n');
    }
    Ok(corpus)
}

fn render_text(
    language: &str,
    category: &str,
    ordinal: usize,
    rng: &mut SplitMix64,
) -> Result<String> {
    match category {
        "temporal_numeric" => Ok(temporal_numeric(language, ordinal, rng)),
        "documentation_network" => Ok(documentation_network(language, ordinal, rng)),
        "code_log_syntax" => Ok(code_log_syntax(language, ordinal, rng)),
        "public_entities" => Ok(public_entities(language, ordinal, rng)),
        "generic_roles" => Ok(generic_roles(language, ordinal, rng)),
        "commerce_identifiers" => Ok(commerce_identifiers(language, ordinal, rng)),
        "unicode_mixed_language" => Ok(unicode_mixed_language(language, ordinal, rng)),
        "invalid_identifiers" => Ok(invalid_identifiers(language, ordinal, rng)),
        _ => bail!("unknown negative-corpus category {category}"),
    }
}

fn temporal_numeric(language: &str, ordinal: usize, rng: &mut SplitMix64) -> String {
    let year = 2030 + rng.bounded(7);
    let month = 1 + rng.bounded(9);
    let day = 10 + rng.bounded(18);
    let hour = 8 + rng.bounded(10);
    let minute = rng.bounded(60);
    let price_major = 10 + rng.bounded(900);
    let price_minor = rng.bounded(100);
    let length_major = 1 + rng.bounded(90);
    let length_minor = rng.bounded(10);
    let major = 1 + rng.bounded(9);
    let minor = rng.bounded(20);
    let patch = rng.bounded(30);
    if language == "en" {
        format!(
            "Numeric benchmark {ordinal:02}: {year}-{month:02}-{day:02} at {hour:02}:{minute:02}; price EUR {price_major}.{price_minor:02}; length {length_major}.{length_minor} cm; package version v{major}.{minor}.{patch}."
        )
    } else {
        format!(
            "Numerischer Messfall {ordinal:02}: {day:02}.{month:02}.{year} um {hour:02}:{minute:02} Uhr; Preis {price_major},{price_minor:02} EUR; Länge {length_major},{length_minor} cm; Paketversion v{major}.{minor}.{patch}."
        )
    }
}

fn documentation_network(language: &str, ordinal: usize, rng: &mut SplitMix64) -> String {
    let prefixes = ["192.0.2", "198.51.100", "203.0.113"];
    let domains = [
        "docs.example.invalid",
        "service.example.com",
        "test.example.org",
        "sample.example.net",
    ];
    let ipv4 = format!("{}.{}", rng.pick(&prefixes), 1 + rng.bounded(253));
    let ipv6 = format!(
        "2001:db8:{:x}::{:x}",
        rng.bounded(65_535),
        1 + rng.bounded(255)
    );
    let domain = rng.pick(&domains);
    if language == "en" {
        format!(
            "Documentation route {ordinal:02}: RFC-reserved IPv4 {ipv4}, IPv6 {ipv6}, and example domain {domain}; these values identify no person or deployed endpoint."
        )
    } else {
        format!(
            "Dokumentationsroute {ordinal:02}: RFC-reservierte IPv4-Adresse {ipv4}, IPv6-Adresse {ipv6} und Beispieldomain {domain}; diese Werte bezeichnen weder eine Person noch einen produktiven Endpunkt."
        )
    }
}

fn code_log_syntax(language: &str, ordinal: usize, rng: &mut SplitMix64) -> String {
    let trace = format!(
        "00000000-0000-4000-8000-{:012x}",
        rng.next_u64() & 0x0000_ffff_ffff_ffff
    );
    let hash = format!("{:016x}{:016x}", rng.next_u64(), rng.next_u64());
    let major = 1 + rng.bounded(9);
    let minor = rng.bounded(20);
    let patch = rng.bounded(30);
    let retries = 1 + rng.bounded(8);
    if language == "en" {
        format!(
            "Code/log sample {ordinal:02}: `let retry_limit = {retries};` INFO component=cache status=ok trace_id={trace} digest={hash} package=v{major}.{minor}.{patch}."
        )
    } else {
        format!(
            "Code-/Log-Beispiel {ordinal:02}: `let retry_limit = {retries};` INFO komponente=cache status=ok trace_id={trace} digest={hash} paket=v{major}.{minor}.{patch}."
        )
    }
}

fn public_entities(language: &str, ordinal: usize, rng: &mut SplitMix64) -> String {
    const EN_PLACES: [(&str, &str); 8] = [
        ("Berlin", "Germany"),
        ("Ottawa", "Canada"),
        ("Tokyo", "Japan"),
        ("Brasília", "Brazil"),
        ("Vienna", "Austria"),
        ("Zürich", "Switzerland"),
        ("Oslo", "Norway"),
        ("Lisbon", "Portugal"),
    ];
    const DE_PLACES: [(&str, &str); 8] = [
        ("Berlin", "Deutschland"),
        ("Ottawa", "Kanada"),
        ("Tokio", "Japan"),
        ("Brasília", "Brasilien"),
        ("Wien", "Österreich"),
        ("Zürich", "Schweiz"),
        ("Oslo", "Norwegen"),
        ("Lissabon", "Portugal"),
    ];
    let organizations = ["ISO", "IETF", "W3C", "UNESCO"];
    let organization = rng.pick(&organizations);
    let place_index = rng.bounded(EN_PLACES.len() as u64) as usize;
    if language == "en" {
        let (city, country) = EN_PLACES[place_index];
        format!(
            "Public-entity statement {ordinal:02}: {city} is a city in {country}; {organization} is named only as a public organization, never as a personal affiliation."
        )
    } else {
        let (city, country) = DE_PLACES[place_index];
        format!(
            "Aussage zu öffentlichen Entitäten {ordinal:02}: {city} ist eine Stadt in {country}; {organization} wird ausschließlich als öffentliche Organisation genannt, nicht als persönliche Zugehörigkeit."
        )
    }
}

fn generic_roles(language: &str, ordinal: usize, rng: &mut SplitMix64) -> String {
    let en_roles = [
        "engineer",
        "reviewer",
        "operator",
        "administrator",
        "doctor",
        "professor",
        "technician",
        "team lead",
    ];
    let de_roles = [
        "Fachkraft",
        "Prüfperson",
        "Bedienperson",
        "Administration",
        "ärztliche Fachkraft",
        "Lehrkraft",
        "technische Fachkraft",
        "Teamleitung",
    ];
    let index = rng.bounded(en_roles.len() as u64) as usize;
    if language == "en" {
        format!(
            "Generic-role sentence {ordinal:02}: the {} checks the shared checklist; the title denotes a function and refers to no individual.",
            en_roles[index]
        )
    } else {
        format!(
            "Satz mit allgemeiner Rolle {ordinal:02}: Die {} prüft die gemeinsame Checkliste; die Bezeichnung meint eine Funktion und keine bestimmte Person.",
            de_roles[index]
        )
    }
}

fn commerce_identifiers(language: &str, ordinal: usize, rng: &mut SplitMix64) -> String {
    let order = format!("ORDER-{:04}-{:06}", 9000 + ordinal, rng.bounded(1_000_000));
    let invoice = format!("INVOICE-TEST-{:08}", rng.bounded(100_000_000));
    let sku = format!("SKU-DEMO-{:05}", rng.bounded(100_000));
    let batch = format!("BATCH-SAMPLE-{:05}", rng.bounded(100_000));
    if language == "en" {
        format!(
            "Unassigned commerce syntax {ordinal:02}: order {order}, invoice {invoice}, stock item {sku}, and batch {batch}; the identifiers are synthetic workflow keys with no customer or account linkage."
        )
    } else {
        format!(
            "Nicht zugeordnete Handelssyntax {ordinal:02}: Auftrag {order}, Rechnung {invoice}, Lagerartikel {sku} und Charge {batch}; die Kennungen sind synthetische Ablaufschlüssel ohne Kunden- oder Kontobezug."
        )
    }
}

fn unicode_mixed_language(language: &str, ordinal: usize, rng: &mut SplitMix64) -> String {
    let symbols = ["—", "…", "§", "→", "±", "×", "№", "∞"];
    let symbol = rng.pick(&symbols);
    if language == "en" {
        format!(
            "Unicode punctuation {ordinal:02} {symbol} «Café, Straße, naïve façade, Größe, piñata, déjà vu»; Deutsch + English remain generic words without a personal referent."
        )
    } else {
        format!(
            "Unicode-Zeichensetzung {ordinal:02} {symbol} »Café, Straße, naïve façade, Größe, piñata, déjà vu«; Deutsch + English bleiben allgemeine Wörter ohne Personenbezug."
        )
    }
}

fn invalid_identifiers(language: &str, ordinal: usize, rng: &mut SplitMix64) -> String {
    let en_phones = ["+1-555-01X0", "+44-7700-900X00"];
    let suffix = rng.bounded(10);
    if language == "en" {
        format!(
            "Validator-negative set {ordinal:02}: phone {}-{suffix}; postal 12A; payment 4111-1111-1111-1112; tax 00-0000000; account GB00 TEST 0000 0000 00. Every lookalike is deliberately invalid and unassigned.",
            rng.pick(&en_phones)
        )
    } else {
        format!(
            "Validator-negativer Satz {ordinal:02}: Telefon +49 1555 01122X3-{suffix}; Postleitzahl 12A4; Zahlung 4111-1111-1111-1112; Steuer 12X45678901; Konto DE00 TEST 0000 0000 0000 00. Jeder ähnliche Wert ist absichtlich ungültig und nicht vergeben."
        )
    }
}

fn validate_documents(documents: &[Document]) -> Result<()> {
    let expected_total = LANGUAGES.len() * CATEGORIES.len() * RECORDS_PER_LANGUAGE_CATEGORY;
    if documents.len() != expected_total {
        bail!(
            "expected {expected_total} documents, got {}",
            documents.len()
        );
    }

    let mut ids = HashSet::with_capacity(documents.len());
    let mut languages = BTreeMap::<&str, usize>::new();
    let mut categories = BTreeMap::<&str, usize>::new();
    for document in documents {
        if !ids.insert(document.id.as_str()) {
            bail!("duplicate negative-corpus id {}", document.id);
        }
        if !document.oracle_spans.is_empty() {
            bail!("negative-only document {} has oracle spans", document.id);
        }
        if document.text.trim().is_empty() {
            bail!("negative-only document {} has empty text", document.id);
        }
        *languages.entry(&document.language).or_default() += 1;
        *categories.entry(&document.category).or_default() += 1;
    }

    let expected_language = CATEGORIES.len() * RECORDS_PER_LANGUAGE_CATEGORY;
    for language in LANGUAGES {
        if languages.get(language) != Some(&expected_language) {
            bail!("language {language} does not have {expected_language} documents");
        }
    }
    let expected_category = LANGUAGES.len() * RECORDS_PER_LANGUAGE_CATEGORY;
    for category in CATEGORIES {
        if categories.get(category) != Some(&expected_category) {
            bail!("category {category} does not have {expected_category} documents");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn bounded(&mut self, upper: u64) -> u64 {
        debug_assert!(upper > 0);
        self.next_u64() % upper
    }

    fn pick<'a>(&mut self, values: &'a [&str]) -> &'a str {
        values[self.bounded(values.len() as u64) as usize]
    }
}

#[cfg(test)]
fn parse_corpus(corpus: &str) -> Result<Vec<Document>> {
    corpus
        .lines()
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line)
                .with_context(|| format!("invalid JSONL record at line {}", index + 1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMITTED: &str = include_str!("../fixtures/negative_corpus/en_de_negative.jsonl");

    #[test]
    fn regeneration_is_byte_for_byte_deterministic() {
        assert_eq!(generate_corpus(0).expect("generate corpus"), COMMITTED);
        assert_eq!(generate_corpus(0).unwrap(), generate_corpus(0).unwrap());
    }

    #[test]
    fn seed_changes_content_without_changing_stable_ids() {
        let seed_zero = parse_corpus(&generate_corpus(0).unwrap()).unwrap();
        let seed_one = parse_corpus(&generate_corpus(1).unwrap()).unwrap();
        assert_ne!(seed_zero, seed_one);
        assert_eq!(
            seed_zero
                .iter()
                .map(|document| &document.id)
                .collect::<Vec<_>>(),
            seed_one
                .iter()
                .map(|document| &document.id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn every_document_has_zero_oracle_pii_spans() {
        let documents = parse_corpus(COMMITTED).expect("parse committed corpus");
        assert_eq!(documents.len(), 1_024);
        assert!(documents
            .iter()
            .all(|document| document.oracle_spans.is_empty()));
    }

    #[test]
    fn ids_are_stable_and_unique() {
        let documents = parse_corpus(COMMITTED).expect("parse committed corpus");
        let ids: HashSet<_> = documents.iter().map(|document| &document.id).collect();
        assert_eq!(ids.len(), documents.len());
        assert_eq!(
            documents.first().map(|document| document.id.as_str()),
            Some("neg-en-temporal-numeric-0000")
        );
        assert_eq!(
            documents.last().map(|document| document.id.as_str()),
            Some("neg-de-invalid-identifiers-0063")
        );
    }

    #[test]
    fn languages_and_categories_meet_declared_minimums() {
        let documents = parse_corpus(COMMITTED).expect("parse committed corpus");
        let mut languages = BTreeMap::<&str, usize>::new();
        let mut categories = BTreeMap::<&str, usize>::new();
        for document in &documents {
            *languages.entry(&document.language).or_default() += 1;
            *categories.entry(&document.category).or_default() += 1;
        }
        assert_eq!(languages, BTreeMap::from([("de", 512), ("en", 512)]));
        for category in CATEGORIES {
            assert_eq!(
                categories.get(category),
                Some(&(RECORDS_PER_LANGUAGE_CATEGORY * 2))
            );
        }
    }

    #[test]
    fn metadata_contract_is_complete() {
        let documents = parse_corpus(COMMITTED).expect("parse committed corpus");
        assert!(documents.iter().all(|document| {
            document.generator_id == GENERATOR_ID
                && document.seed == 0
                && LANGUAGES.contains(&document.language.as_str())
                && CATEGORIES.contains(&document.category.as_str())
                && document.license_origin == LICENSE_ORIGIN
        }));
    }
}
