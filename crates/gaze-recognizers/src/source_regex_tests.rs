use super::*;
use proc_macro2::{TokenStream, TokenTree};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

fn literals(tokens: TokenStream, corpus: &mut BTreeSet<String>) {
    for token in tokens {
        match token {
            TokenTree::Group(group) => literals(group.stream(), corpus),
            TokenTree::Literal(literal) => match syn::parse_str::<syn::Lit>(&literal.to_string()) {
                Ok(syn::Lit::Str(value)) => {
                    corpus.insert(value.value());
                }
                Ok(syn::Lit::ByteStr(value)) => {
                    if let Ok(value) = String::from_utf8(value.value()) {
                        corpus.insert(value);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

fn json_strings(value: &serde_json::Value, corpus: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::String(value) => {
            corpus.insert(value.clone());
        }
        serde_json::Value::Array(values) => {
            values.iter().for_each(|value| json_strings(value, corpus))
        }
        serde_json::Value::Object(values) => values
            .values()
            .for_each(|value| json_strings(value, corpus)),
        _ => {}
    }
}

fn patterns(value: &toml::Value, origin: &str, found: &mut Vec<(String, String)>) {
    match value {
        toml::Value::Table(table) => {
            for (key, value) in table {
                let path = format!("{origin}/{key}");
                if matches!(
                    key.as_str(),
                    "pattern" | "pattern_template" | "reject_match_regex"
                ) {
                    if let Some(pattern) = value.as_str() {
                        found.push((path.clone(), pattern.to_owned()));
                    }
                }
                patterns(value, &path, found);
            }
        }
        toml::Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                patterns(value, &format!("{origin}/{index}"), found);
            }
        }
        _ => {}
    }
}

fn expand(pattern: &str, buckets: &BTreeMap<String, BTreeSet<String>>) -> String {
    let placeholder =
        Regex::new(r"\{(?:locale\.([a-zA-Z0-9_-]+)|(locale_email_headers))\}").unwrap();
    placeholder
        .replace_all(pattern, |captures: &regex::Captures<'_>| {
            let key = captures
                .get(1)
                .map_or("email_headers", |matched| matched.as_str());
            let terms = buckets
                .get(key)
                .unwrap_or_else(|| panic!("unexpanded locale bucket {key}"));
            format!(
                "(?:{})",
                terms
                    .iter()
                    .map(|term| regex::escape(term))
                    .collect::<Vec<_>>()
                    .join("|")
            )
        })
        .into_owned()
}

fn captures(regex: &Regex, input: &str) -> Vec<Vec<Option<(usize, usize)>>> {
    regex
        .captures_iter(input)
        .map(|captures| {
            captures
                .iter()
                .map(|capture| capture.map(|capture| (capture.start(), capture.end())))
                .collect()
        })
        .collect()
}

#[test]
fn all_rulepack_regexes_preserve_captures_without_source_gaps() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let files = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(files.status.success());
    let mut corpus = BTreeSet::new();
    let mut regexes = Vec::new();
    let mut buckets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut inventory = Sha256::new();
    let mut corpus_files = 0;
    let mut rulepack_files = BTreeSet::new();
    for name in files
        .stdout
        .split(|&byte| byte == 0)
        .filter(|name| !name.is_empty())
    {
        let relative = PathBuf::from(String::from_utf8(name.to_vec()).unwrap());
        let bytes = std::fs::read(root.join(&relative)).unwrap();
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let path = relative.to_string_lossy();
        let rust = relative
            .extension()
            .is_some_and(|extension| extension == "rs");
        if rust || path.contains("/tests/") || path.contains("/fixtures/") {
            inventory.update(name);
            inventory.update([0]);
            inventory.update(&bytes);
            corpus_files += 1;
            if rust {
                match text.parse::<TokenStream>() {
                    Ok(tokens) => literals(tokens, &mut corpus),
                    Err(_) => {
                        corpus.insert(text.to_owned());
                    }
                }
            } else {
                corpus.insert(text.to_owned());
                for line in text.lines() {
                    corpus.insert(line.to_owned());
                    if let Ok(value) = serde_json::from_str(line) {
                        json_strings(&value, &mut corpus);
                    }
                }
                if let Ok(value) = serde_json::from_str(text) {
                    json_strings(&value, &mut corpus);
                }
            }
        }
        let toml_docs = if relative
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            vec![text]
        } else if relative
            .extension()
            .is_some_and(|extension| extension == "md")
        {
            text.split("```toml")
                .skip(1)
                .filter_map(|block| block.split_once("```").map(|(code, _)| code))
                .collect()
        } else {
            Vec::new()
        };
        for document in toml_docs {
            let Ok(value) = document.parse::<toml::Value>() else {
                continue;
            };
            if value.get("rulepack_id").is_none() && value.get("recognizers").is_none() {
                continue;
            }
            rulepack_files.insert(path.to_string());
            patterns(&value, &path, &mut regexes);
            if let Some(locale) = value.get("locale").and_then(toml::Value::as_table) {
                for (bucket, value) in locale {
                    if let Some(names) = value.get("names").and_then(toml::Value::as_array) {
                        buckets.entry(bucket.clone()).or_default().extend(
                            names
                                .iter()
                                .filter_map(toml::Value::as_str)
                                .map(str::to_owned),
                        );
                    }
                }
            }
        }
    }
    assert!(!regexes.is_empty());
    let ignorables = Regex::new(r"\p{Default_Ignorable_Code_Point}").unwrap();
    let before_filter = corpus.len();
    corpus.retain(|input| !ignorables.is_match(input));
    let excluded = before_filter - corpus.len();
    let literal_cases = corpus.len();
    // Deterministic arbitrary Unicode/punctuation fuzz, independent of detector output.
    let alphabet = "abcdefABCDEFxyzXYZ0123456789 @.+-_:/,;\"'{}[]()\n\r\téΩक👩🔬"
        .chars()
        .collect::<Vec<_>>();
    let mut seed = 0xc71a_49b2_86d3_5e01u64;
    for index in 0..4096 {
        let mut input = String::new();
        for _ in 0..index % 257 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            input.push(alphabet[seed as usize % alphabet.len()]);
        }
        assert!(!ignorables.is_match(&input));
        corpus.insert(input);
    }
    let mut inputs = Sha256::new();
    for input in &corpus {
        inputs.update(input.as_bytes());
        inputs.update([0]);
    }
    let mut maximum = (0.0f64, String::new(), 0, 0);
    let mut comparisons = 0usize;
    let mut matching_pairs = 0usize;
    let mut sizes = Vec::new();
    for (id, pattern) in &regexes {
        let pattern = expand(pattern, &buckets);
        let original = Regex::new(&pattern).unwrap_or_else(|error| panic!("{id}: {error}"));
        let transformed = compile(&pattern).unwrap_or_else(|error| panic!("{id}: {error}"));
        assert_eq!(original.captures_len(), transformed.captures_len(), "{id}");
        let original_size = regex_automata::meta::Regex::new(&pattern)
            .unwrap()
            .memory_usage();
        let transformed_size = regex_automata::meta::Regex::new(transformed.as_str())
            .unwrap()
            .memory_usage();
        let ratio = transformed_size as f64 / original_size as f64;
        assert!(ratio <= 8.0, "{id}: compiled memory grew {ratio}x");
        if ratio > maximum.0 {
            maximum = (ratio, id.clone(), original_size, transformed_size);
        }
        sizes.push(serde_json::json!({"regex": id, "original_bytes": original_size, "transformed_bytes": transformed_size}));
        for (index, input) in corpus.iter().enumerate() {
            let expected = captures(&original, input);
            matching_pairs += usize::from(!expected.is_empty());
            assert_eq!(
                expected,
                captures(&transformed, input),
                "{id}, corpus index {index}"
            );
            comparisons += 1;
        }
    }
    assert!(matching_pairs > 1000);
    let report = serde_json::json!({
        "schema": "gaze-source-regex-differential-v1", "regexes": regexes.len(), "rulepack_files": rulepack_files,
        "corpus_files": corpus_files, "repository_cases": literal_cases, "fuzz_attempts": 4096,
        "unique_cases": corpus.len(), "excluded_default_ignorable_cases": excluded,
        "source_inventory_sha256": format!("{:x}", inventory.finalize()),
        "corpus_sha256": format!("{:x}", inputs.finalize()), "comparisons": comparisons,
        "matching_pairs": matching_pairs, "capture_differences": 0,
        "compiled_memory_metric": "regex_automata::meta::Regex::memory_usage (immutable allocations, excludes per-search caches)",
        "max_compiled_memory_ratio": maximum.0, "max_ratio_regex": maximum.1,
        "max_ratio_original_bytes": maximum.2, "max_ratio_transformed_bytes": maximum.3,
        "total_original_compiled_bytes": sizes.iter().map(|row| row["original_bytes"].as_u64().unwrap()).sum::<u64>(),
        "total_extra_compiled_bytes_if_every_transformed_regex_is_used": sizes.iter().map(|row| row["transformed_bytes"].as_u64().unwrap()).sum::<u64>(),
        "compiled_sizes": sizes,
    });
    let output = root.join("target/separator-checks/source-regex-differential.json");
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!("DIFFERENTIAL DONE: {} regexes x {} cases = {} comparisons, 0 capture differences; max compiled memory ratio {}", regexes.len(), corpus.len(), comparisons, maximum.0);
}
