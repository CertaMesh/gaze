//! Every ```toml block in the policy documentation loads with the real loader.
//!
//! Each block carries a marker comment on the line directly before its fence.
//! The marker convention is documented in CONTRIBUTING.md under
//! "TOML blocks in policy docs":
//!
//! - `<!-- policy -->`: a complete policy, loaded with `Policy::load_for_cli`.
//! - `<!-- policy-fragment -->`: policy tables; `[session]` and a default
//!   `[[rule]]` are appended when the block has none.
//! - `<!-- policy-fragment: custom-recognizer -->`: keys of one
//!   `[[policy.custom_recognizers]]` entry, wrapped into a minimal regex entry.
//! - `<!-- policy-fragment: rulepack -->`: rulepack tables, parsed by
//!   `Rulepack::parse` under a minimal rulepack header.
//! - `<!-- policy-fragment: recognizer -->`: sub-tables of one rulepack
//!   `[[recognizers]]` entry, wrapped into a minimal regex recognizer.
//! - `<!-- not-a-policy: <name> -->`: skipped; `<name>` must be listed in
//!   [`NOT_A_POLICY`] with the page and exact body SHA-256, so changes need review.
//!
//! Wrappers only add tables and keys, so `deny_unknown_fields` still rejects a
//! renamed key in the documented text. A block without a marker fails.

use gaze::{Policy, Rulepack};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use sha2::{Digest, Sha256};

const PAGES: &[(&str, &str)] = &[
    (
        "docs/reference/policy.md",
        include_str!("../../../docs/reference/policy.md"),
    ),
    (
        "docs/how-to/manual-policy.md",
        include_str!("../../../docs/how-to/manual-policy.md"),
    ),
    (
        "docs/how-to/rust-library.md",
        include_str!("../../../docs/how-to/rust-library.md"),
    ),
    (
        "docs/how-to/policy/policy-examples.md",
        include_str!("../../../docs/how-to/policy/policy-examples.md"),
    ),
    (
        "docs/how-to/policy/custom-recognizers.md",
        include_str!("../../../docs/how-to/policy/custom-recognizers.md"),
    ),
];

/// Every `<!-- not-a-policy: <name> -->` block, by page, name and body SHA-256.
const NOT_A_POLICY: &[(&str, &str, &str)] = &[(
    "docs/how-to/rust-library.md",
    "cargo-dependencies",
    "dd34746f96dfa450b8b80aed9a06401da125d40aac82409ba95a973f64c74dad",
)];

const RULEPACK_HEADER: &str = "schema_version = \"0.1.0\"\n\
rulepack_id = \"docs.fragment\"\n\
rulepack_version = \"0.0.0\"\n\n";

const BASE_RECOGNIZER: &str = "[[recognizers]]\n\
id = \"docs.fragment\"\n\
class = \"custom:docs_fragment\"\n\
locale_basis = \"format\"\n\n\
[recognizers.match]\n\
kind = \"regex\"\n\
pattern = '\\bACCT-[0-9]{6}\\b'\n\n";

const BASE_CUSTOM_RECOGNIZER: &str = "[[policy.custom_recognizers]]\n\
kind = \"regex\"\n\
name = \"docs_fragment\"\n\
class = \"custom:docs_fragment\"\n";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Marker {
    Policy,
    PolicyFragment,
    CustomRecognizer,
    RulepackFragment,
    RecognizerFragment,
    NotAPolicy(String),
}

struct Block {
    page: &'static str,
    line: usize,
    marker: Result<Marker, String>,
    body: String,
}

fn parse_marker(line: Option<&str>) -> Result<Marker, String> {
    let line = line.map(str::trim).unwrap_or("");
    let Some(inner) = line
        .strip_prefix("<!--")
        .and_then(|rest| rest.strip_suffix("-->"))
        .map(str::trim)
    else {
        return Err("no marker on the line before the fence".to_string());
    };
    match inner {
        "policy" => Ok(Marker::Policy),
        "policy-fragment" => Ok(Marker::PolicyFragment),
        "policy-fragment: custom-recognizer" => Ok(Marker::CustomRecognizer),
        "policy-fragment: rulepack" => Ok(Marker::RulepackFragment),
        "policy-fragment: recognizer" => Ok(Marker::RecognizerFragment),
        other => match other.strip_prefix("not-a-policy:").map(str::trim) {
            Some(name) if !name.is_empty() => Ok(Marker::NotAPolicy(name.to_string())),
            _ => Err(format!("unknown marker `{line}`")),
        },
    }
}

/// Extracts TOML fences from the Markdown event stream, including all containers.
fn toml_blocks(page: &'static str, text: &str) -> Vec<Block> {
    let mut events = Parser::new(text).into_offset_iter();
    let mut blocks = Vec::new();
    let mut marker = None;
    while let Some((event, range)) = events.next() {
        let line = text[..range.start].bytes().filter(|&b| b == b'\n').count() + 1;
        match event {
            Event::Html(html) => marker = Some((line, parse_marker(Some(&html)))),
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                if info
                    .split(|c: char| c.is_whitespace() || matches!(c, ',' | '{'))
                    .next()
                    .map(str::to_ascii_lowercase)
                    .as_deref()
                    == Some("toml") =>
            {
                let marker = marker
                    .take()
                    .filter(|(marker_line, _)| *marker_line + 1 == line)
                    .map(|(_, marker)| marker)
                    .unwrap_or_else(|| parse_marker(None));
                let mut body = String::new();
                for (event, _) in events.by_ref() {
                    match event {
                        Event::Text(content) => body.push_str(&content),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                }
                blocks.push(Block {
                    page,
                    line,
                    marker,
                    body,
                });
            }
            _ => {}
        }
    }
    blocks
}

fn load_policy(text: &str) -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let path = dir.path().join("policy.toml");
    std::fs::write(&path, text).map_err(|err| err.to_string())?;
    Policy::load_for_cli(&path)
        .map(|_| ())
        .map_err(|err| format!("{err:?}"))
}

/// Appends `[session]` and a default `[[rule]]` when the fragment lacks them.
fn wrap_policy_fragment(fragment: &str) -> Result<String, String> {
    let table: toml::Table = fragment
        .parse()
        .map_err(|err: toml::de::Error| err.to_string())?;
    let mut text = fragment.to_string();
    if !table.contains_key("session") {
        text.push_str("\n[session]\nscope = \"conversation\"\n");
    }
    if !table.contains_key("rule") {
        text.push_str("\n[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n");
    }
    Ok(text)
}

fn check(block: &Block) -> Result<Option<String>, String> {
    let marker = block.marker.clone()?;
    match marker {
        Marker::Policy => load_policy(&block.body).map(|_| None),
        Marker::PolicyFragment => load_policy(&wrap_policy_fragment(&block.body)?).map(|_| None),
        Marker::CustomRecognizer => {
            let fragment = format!("{BASE_CUSTOM_RECOGNIZER}{}", block.body);
            load_policy(&wrap_policy_fragment(&fragment)?).map(|_| None)
        }
        Marker::RulepackFragment => Rulepack::parse(&format!("{RULEPACK_HEADER}{}", block.body))
            .map(|_| None)
            .map_err(|err| format!("{err:?}")),
        Marker::RecognizerFragment => {
            Rulepack::parse(&format!("{RULEPACK_HEADER}{BASE_RECOGNIZER}{}", block.body))
                .map(|_| None)
                .map_err(|err| format!("{err:?}"))
        }
        Marker::NotAPolicy(name) => {
            let Some((_, _, expected)) = NOT_A_POLICY
                .iter()
                .find(|(page, allowed, _)| *page == block.page && *allowed == name)
            else {
                return Err(format!(
                    "not-a-policy `{name}` is not allowlisted for {}",
                    block.page
                ));
            };
            let actual = format!("{:x}", Sha256::digest(block.body.as_bytes()));
            if actual != *expected {
                return Err(format!(
                    "not-a-policy body SHA-256 changed for `{name}`: expected {expected}, got {actual}; review the excerpt before updating NOT_A_POLICY"
                ));
            }
            Ok(Some(name))
        }
    }
}

#[test]
fn every_policy_doc_toml_block_is_marked_and_loads() {
    let mut failures = Vec::new();
    let mut skipped = Vec::new();
    let mut checked = 0;
    for (page, text) in PAGES {
        let blocks = toml_blocks(page, text);
        assert!(!blocks.is_empty(), "{page} has no ```toml blocks");
        for block in blocks {
            match check(&block) {
                Ok(Some(name)) => skipped.push((block.page, name)),
                Ok(None) => checked += 1,
                Err(err) => failures.push(format!("{}:{}: {err}", block.page, block.line)),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} doc toml block(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    let mut expected: Vec<(&str, String)> = NOT_A_POLICY
        .iter()
        .map(|(page, name, _)| (*page, (*name).to_string()))
        .collect();
    expected.sort();
    skipped.sort();
    assert_eq!(
        skipped, expected,
        "not-a-policy blocks must match NOT_A_POLICY exactly"
    );
    assert_eq!(checked, 32, "policy doc block inventory changed");
}

fn assert_invalid_policy_is_checked(prefix: &str, open: &str, close: &str) {
    for marker in ["", "<!-- policy-fragment -->\n"] {
        let page = format!("{prefix}{marker}{open}\n[ner]\ntreshold = 0.3\n{close}\n");
        let blocks = toml_blocks("fixture.md", &page);
        assert_eq!(blocks.len(), 1, "policy block must remain visible: {open}");
        let error = check(&blocks[0]).expect_err("invalid policy must fail");
        let expected = if marker.is_empty() {
            "no marker on the line before the fence"
        } else {
            "treshold"
        };
        assert!(error.contains(expected), "unexpected failure: {error}");

        if !marker.is_empty() {
            let fixed = page.replace("treshold", "threshold");
            let blocks = toml_blocks("fixture.md", &fixed);
            assert_eq!(blocks.len(), 1);
            assert_eq!(check(&blocks[0]), Ok(None));
        }
    }
}

#[test]
fn four_backtick_block_does_not_hide_following_policy() {
    assert_invalid_policy_is_checked("````text\nexample\n````\n\n", "```toml", "```");
}

#[test]
fn spaced_toml_info_string_is_checked() {
    assert_invalid_policy_is_checked("", "``` toml", "```");
    assert_invalid_policy_is_checked("", "```  toml example", "```");
}

#[test]
fn uppercase_toml_info_string_is_checked() {
    assert_invalid_policy_is_checked("", "```TOML", "```");
}

#[test]
fn comma_toml_info_string_is_checked() {
    assert_invalid_policy_is_checked("", "```toml,ignore", "```");
}

#[test]
fn brace_toml_info_string_is_checked() {
    assert_invalid_policy_is_checked("", "```toml{.example}", "```");
}

#[test]
fn tilde_policy_fences_are_checked() {
    assert_invalid_policy_is_checked("", "~~~toml", "~~~");
    assert_invalid_policy_is_checked("", "   ~~~~ toml", "   ~~~~~  ");
}

#[test]
fn policy_fences_require_matching_delimiters_and_valid_indentation() {
    let page = "<!-- policy-fragment -->\n   ````toml\n   [ner]\n   threshold = 0.3\n   ```\n   ~~~~\n   ```` trailing\n   `````  \n";
    let blocks = toml_blocks("fixture.md", page);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].line, 2);
    assert_eq!(
        blocks[0].body,
        "[ner]\nthreshold = 0.3\n```\n~~~~\n```` trailing\n"
    );
    for open in ["    ```toml", "\t```toml", "```toml`invalid"] {
        let page = format!("{open}\n[ner]\nthreshold = 0.3\n");
        assert!(toml_blocks("fixture.md", &page).is_empty(), "{open}");
    }
}

/// The fence scanner, marker parser and fragment wrappers reject what they
/// must: an unmarked block, an unknown marker, and a renamed key in a fragment.
#[test]
fn doc_toml_block_checker_rejects_unmarked_and_renamed_keys() {
    let page = "\
```toml
[session]
scope = \"conversation\"
```

<!-- policy-fragmnet -->
```toml
[ner]
threshold = 0.3
```

> <!-- policy-fragment -->
> ```toml
> [ner]
> treshold = 0.3
> ```

<!-- policy-fragment: recognizer -->
```toml
[recognizers.validator]
knd = \"luhn\"
```

<!-- policy-fragment: custom-recognizer -->
```toml
patern = 'ACCT-[0-9]{6}'
```
";
    let blocks = toml_blocks("fixture.md", page);
    assert_eq!(blocks.len(), 5);
    for block in &blocks {
        assert!(
            check(block).is_err(),
            "block at line {} must fail",
            block.line
        );
    }
    let fixed = page
        .replace("fragmnet", "fragment")
        .replace("treshold", "threshold")
        .replace("knd", "kind")
        .replace("patern", "pattern");
    let fixed = fixed.replacen(
        "```toml\n[session]",
        "<!-- policy -->\n```toml\n[session]",
        1,
    );
    let fixed = fixed.replacen(
        "scope = \"conversation\"\n```",
        "scope = \"conversation\"\n\n[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n```",
        1,
    );
    for block in toml_blocks("fixture.md", &fixed) {
        assert_eq!(check(&block), Ok(None), "block at line {}", block.line);
    }
}

/// policy.md shows a deliberate pair: a currency pattern that misses
/// `5000€` because `\b` cannot sit between `€` and the end of text, and the
/// digit-guarded rewrite that matches it. Both load; only one matches.
#[test]
fn policy_md_boundary_pitfall_pair_behaves_as_documented() {
    let section = PAGES[0]
        .1
        .split_once("#### Pitfall: `\\b` next to non-word characters")
        .expect("pitfall section")
        .1;
    let blocks = toml_blocks("docs/reference/policy.md", section);
    let patterns: Vec<String> = blocks
        .iter()
        .take(2)
        .map(|block| {
            let table: toml::Table = block.body.parse().expect("pattern fragment parses");
            table["pattern"]
                .as_str()
                .expect("pattern string")
                .to_string()
        })
        .collect();
    assert_eq!(
        patterns.len(),
        2,
        "pitfall shows a broken and a fixed pattern"
    );
    let broken = regex::Regex::new(&patterns[0]).expect("broken pattern compiles");
    let fixed = regex::Regex::new(&patterns[1]).expect("fixed pattern compiles");
    assert!(!broken.is_match("5000€"), "broken pattern must miss 5000€");
    assert_eq!(
        fixed.find("5000€").map(|found| found.as_str()),
        Some("5000€")
    );
}

fn assert_container_policy_is_checked(prefix: &str, line_prefix: &str) {
    for marker in ["", "<!-- policy-fragment -->\n"] {
        let content = format!("{marker}```toml\n[ner]\ntreshold = 0.3\n```\n");
        let page = format!(
            "{prefix}{}",
            content
                .lines()
                .map(|line| format!("{line_prefix}{line}\n"))
                .collect::<String>()
        );
        let blocks = toml_blocks("fixture.md", &page);
        assert_eq!(
            blocks.len(),
            1,
            "container must expose its TOML block: {page}"
        );
        let error = check(&blocks[0]).expect_err("invalid container policy must fail");
        let expected = if marker.is_empty() {
            "no marker on the line before the fence"
        } else {
            "treshold"
        };
        assert!(error.contains(expected), "unexpected failure: {error}");
        if !marker.is_empty() {
            let fixed = page.replace("treshold", "threshold");
            let blocks = toml_blocks("fixture.md", &fixed);
            assert_eq!(blocks.len(), 1);
            assert_eq!(check(&blocks[0]), Ok(None));
        }
    }
}

#[test]
fn list_item_policy_fences_are_checked() {
    assert_container_policy_is_checked("- Example\n\n", "    ");
}

#[test]
fn indented_blockquote_policy_fences_are_checked() {
    assert_container_policy_is_checked("", "  > ");
}

#[test]
fn nested_blockquote_policy_fences_are_checked() {
    assert_container_policy_is_checked("", "> > ");
}

#[test]
fn cargo_exception_rejects_replacement_policy_body() {
    let page = "# Use the `gaze setup` policy from Rust\n\n<!-- not-a-policy: cargo-dependencies -->\n```toml\n[ner]\ntreshold = 0.3\n```\n";
    let blocks = toml_blocks("docs/how-to/rust-library.md", page);
    assert_eq!(blocks.len(), 1);
    let error = check(&blocks[0]).expect_err("Cargo exception must reject a changed body");
    assert!(
        error.contains("not-a-policy body SHA-256 changed"),
        "unexpected failure: {error}"
    );
}

#[test]
fn cargo_exception_requires_exact_page_and_body() {
    let (page, text) = PAGES[2];
    let mut block = toml_blocks(page, text)
        .into_iter()
        .find(|block| matches!(block.marker, Ok(Marker::NotAPolicy(_))))
        .expect("Cargo dependency excerpt");
    assert_eq!(check(&block), Ok(Some("cargo-dependencies".to_string())));
    block.page = "fixture.md";
    let error = check(&block).expect_err("exception must be scoped to its page");
    assert!(
        error.contains("not allowlisted"),
        "unexpected failure: {error}"
    );
    block.page = page;
    block.body.push('\n');
    let error = check(&block).expect_err("even a whitespace body change needs review");
    assert!(
        error.contains("not-a-policy body SHA-256 changed"),
        "unexpected failure: {error}"
    );
}
