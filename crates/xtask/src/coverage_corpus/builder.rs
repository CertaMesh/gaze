use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::generators::GeneratorRegistry;
use super::templates::{self, Template};
use super::COVERAGE_CORPUS_SEED;
use crate::repo::repo_root;

#[derive(Debug, Parser)]
pub struct Args {
    #[arg(long)]
    regenerate: bool,
    #[arg(long, default_value_t = COVERAGE_CORPUS_SEED)]
    seed: u64,
    #[arg(long)]
    only: Option<String>,
}

pub fn run(args: Args) -> Result<()> {
    if !args.regenerate {
        bail!("coverage-corpus requires --regenerate");
    }

    let templates = templates::phase_1_templates().context("load coverage corpus templates")?;
    let templates = filter_templates(templates, args.only.as_deref())?;
    let registry = GeneratorRegistry::default_phase_1();
    let corpus_dir = repo_root()?.join("crates/gaze-recognizers/testdata/coverage-loop/corpus");
    fs::create_dir_all(&corpus_dir).context("create coverage corpus directory")?;
    remove_generated_corpus_files(&corpus_dir)?;

    let mut fixture_count = 0_usize;
    let mut manifest_entries = Vec::new();
    for template in templates {
        manifest_entries.push(build_fixture(&registry, &template, args.seed, &corpus_dir)?);
        fixture_count += 1;
    }

    manifest_entries.sort_by(|left, right| left.fixture_id.cmp(&right.fixture_id));
    write_atomic(
        &repo_root()?.join("crates/gaze-recognizers/testdata/coverage-loop/build-manifest.json"),
        serde_json::to_string_pretty(&BuildManifest {
            schema_version: 1,
            seed: args.seed,
            fixture_count,
            corpus_sha256: corpus_sha256(&manifest_entries),
            fixtures: manifest_entries,
        })
        .context("serialize coverage corpus build manifest")?
        .as_bytes(),
    )?;

    println!(
        "built {fixture_count} fixtures across coverage-loop templates with seed {}",
        args.seed
    );
    Ok(())
}

fn filter_templates(templates: Vec<Template>, only: Option<&str>) -> Result<Vec<Template>> {
    let Some(only) = only else {
        return Ok(templates);
    };
    let selected = templates
        .into_iter()
        .filter(|template| template.context == only)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        bail!("no coverage corpus templates found for context {only:?}");
    }
    Ok(selected)
}

fn build_fixture(
    registry: &GeneratorRegistry,
    template: &Template,
    global_seed: u64,
    corpus_dir: &Path,
) -> Result<BuildManifestEntry> {
    validate_template(template)?;

    let fixture_id = template.id.clone();
    let template_seed = mixed_seed(global_seed, &template.id, 0, "template");
    let mut body = String::new();
    let mut spans = Vec::new();
    let mut cursor = 0_usize;
    let mut slot_idx = 0_usize;

    while let Some(open_rel) = template.body[cursor..].find("{{") {
        let open = cursor + open_rel;
        let close_rel = template.body[open + 2..]
            .find("}}")
            .with_context(|| format!("unclosed placeholder in template {}", template.id))?;
        let close = open + 2 + close_rel;
        body.push_str(&template.body[cursor..open]);

        let slot = &template.body[open + 2..close];
        let class_id = slot.split('#').next().unwrap_or(slot).trim();
        let locale = template.locale_chain.first().map(String::as_str);
        let generator = registry.lookup(class_id, locale).with_context(|| {
            format!(
                "no generator for class {class_id:?} and locale {:?} in template {}",
                locale, template.id
            )
        })?;
        let generator_seed = mixed_seed(global_seed, &template.id, 0, slot);
        let raw_label = generator.generate(generator_seed);
        let byte_start = body.len();
        body.push_str(&raw_label);
        let byte_end = body.len();

        let expected = template
            .expected_emissions
            .get(slot_idx)
            .with_context(|| format!("missing expected emission {slot_idx} in {}", template.id))?;
        if expected.class_id != class_id {
            bail!(
                "expected emission {slot_idx} in {} has class {}, placeholder has {class_id}",
                template.id,
                expected.class_id
            );
        }

        spans.push(LabelSpan {
            byte_start,
            byte_end,
            class_id: class_id.to_string(),
            raw_label,
            generator_id: generator.id(),
            generator_seed,
            license_origin: "synthetic-rust-generator",
            expected_recognizer_id: expected.recognizer_id.clone(),
            expected_recognizer_version_id: expected.recognizer_version_id.clone(),
        });

        slot_idx += 1;
        cursor = close + 2;
    }

    body.push_str(&template.body[cursor..]);
    if slot_idx == 0 {
        bail!("template {} has no placeholders", template.id);
    }
    if slot_idx != template.expected_emissions.len() {
        bail!(
            "template {} has {slot_idx} placeholders but {} expected emissions",
            template.id,
            template.expected_emissions.len()
        );
    }

    for span in &spans {
        if !body.is_char_boundary(span.byte_start) || !body.is_char_boundary(span.byte_end) {
            bail!("fixture {fixture_id} produced non-UTF-8-boundary span");
        }
    }

    let labels = Labels {
        schema_version: 1,
        fixture_id: fixture_id.clone(),
        context: template.context.clone(),
        locale_chain: template.locale_chain.clone(),
        length_tier: template.length_tier.clone(),
        structural_family: template.structural_family.clone(),
        spans,
        metadata: LabelMetadata {
            template_id: template.id.clone(),
            template_seed,
        },
    };
    let body_sha256 = hex_sha256(body.as_bytes());
    let generator_ids = labels
        .spans
        .iter()
        .map(|span| span.generator_id)
        .collect::<Vec<_>>();

    write_atomic(
        &corpus_dir.join(format!("{fixture_id}.txt")),
        body.as_bytes(),
    )?;
    write_atomic(
        &corpus_dir.join(format!("{fixture_id}.labels.json")),
        serde_json::to_string_pretty(&labels)
            .context("serialize fixture labels")?
            .as_bytes(),
    )?;

    Ok(BuildManifestEntry {
        fixture_id,
        template_id: template.id.clone(),
        fixture_idx: 0,
        seed: global_seed,
        template_seed,
        generator_ids,
        body_sha256,
    })
}

fn validate_template(template: &Template) -> Result<()> {
    if template.id.trim().is_empty() {
        bail!("coverage corpus template id must not be empty");
    }
    if template.context.trim().is_empty() {
        bail!(
            "coverage corpus template {} context must not be empty",
            template.id
        );
    }
    if template.locale_chain.is_empty() {
        bail!(
            "coverage corpus template {} locale_chain must not be empty",
            template.id
        );
    }
    if template.expected_classes.is_empty() {
        bail!(
            "coverage corpus template {} expected_classes must not be empty",
            template.id
        );
    }
    if !matches!(
        template.length_tier.as_str(),
        "Snippet" | "Page" | "MultiPage"
    ) {
        bail!(
            "coverage corpus template {} has invalid length_tier {}",
            template.id,
            template.length_tier
        );
    }
    if template.structural_family.trim().is_empty() {
        bail!(
            "coverage corpus template {} structural_family must not be empty",
            template.id
        );
    }
    let _ = &template.notes;
    let _ = &template.source_citation;
    Ok(())
}

fn mixed_seed(global_seed: u64, template_id: &str, fixture_idx: usize, slot: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(global_seed.to_le_bytes());
    hasher.update(template_id.as_bytes());
    hasher.update(fixture_idx.to_le_bytes());
    hasher.update(slot.as_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes(digest[..8].try_into().expect("sha256 digest has 32 bytes"))
}

fn remove_generated_corpus_files(corpus_dir: &Path) -> Result<()> {
    for entry in fs::read_dir(corpus_dir).context("read coverage corpus directory")? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.ends_with(".txt") || name.ends_with(".labels.json") {
            fs::remove_file(&path)
                .with_context(|| format!("remove stale corpus file {}", path.display()))?;
        }
    }
    Ok(())
}

fn hex_sha256(contents: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents);
    format!("{:x}", hasher.finalize())
}

fn corpus_sha256(entries: &[BuildManifestEntry]) -> String {
    let mut hasher = Sha256::new();
    for entry in entries {
        hasher.update(entry.fixture_id.as_bytes());
        hasher.update(entry.body_sha256.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp = tmp_path(path);
    fs::write(&tmp, contents).with_context(|| format!("write temp file {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} to {}", tmp.display(), path.display()))
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

#[derive(Debug, Serialize)]
struct Labels {
    schema_version: u8,
    fixture_id: String,
    context: String,
    locale_chain: Vec<String>,
    length_tier: String,
    structural_family: String,
    spans: Vec<LabelSpan>,
    metadata: LabelMetadata,
}

#[derive(Debug, Serialize)]
struct LabelSpan {
    byte_start: usize,
    byte_end: usize,
    class_id: String,
    raw_label: String,
    generator_id: &'static str,
    generator_seed: u64,
    license_origin: &'static str,
    expected_recognizer_id: String,
    expected_recognizer_version_id: String,
}

#[derive(Debug, Serialize)]
struct LabelMetadata {
    template_id: String,
    template_seed: u64,
}

#[derive(Debug, Serialize)]
struct BuildManifest {
    schema_version: u8,
    seed: u64,
    fixture_count: usize,
    corpus_sha256: String,
    fixtures: Vec<BuildManifestEntry>,
}

#[derive(Debug, Serialize)]
struct BuildManifestEntry {
    fixture_id: String,
    template_id: String,
    fixture_idx: usize,
    seed: u64,
    template_seed: u64,
    generator_ids: Vec<&'static str>,
    body_sha256: String,
}
