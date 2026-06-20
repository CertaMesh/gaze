//! Scan a folder of UTF-8 text files with a small gaze redaction pipeline.
//!
//! Run:
//!
//! ```text
//! cargo run -p gaze-pii --example scan_folder -- --path ./my-data
//! ```
//!
//! Omitting `--path` runs a tiny synthetic sample.

use std::collections::{BTreeMap, VecDeque};
use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, EmittedTokenSpan, LocaleTag, PiiClass, Pipeline,
    RawDocument, Scope, Session,
};
use gaze_recognizers::RegexDetector;

const SYNTHETIC_SAMPLE: &str =
    "Contact Dr. Schmidt at alice@example.invalid or +1-555-0100 about order ORD-1042.";

struct InputFile {
    label: String,
    text: String,
}

#[derive(Default)]
struct Summary {
    files: usize,
    skipped: usize,
    detections: usize,
    by_class: BTreeMap<String, usize>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = parse_path_arg()?;
    let pipeline = build_pipeline()?;
    let locale_chain = [LocaleTag::Global];
    let (files, skipped) = match path {
        Some(path) => read_text_files(&path)?,
        None => (
            vec![InputFile {
                label: "synthetic-sample.txt".to_string(),
                text: SYNTHETIC_SAMPLE.to_string(),
            }],
            0,
        ),
    };

    if files.is_empty() {
        println!("No UTF-8 text files found.");
        if skipped > 0 {
            println!("Skipped {skipped} non-text file(s).");
        }
        return Ok(());
    }

    let mut summary = Summary {
        skipped,
        ..Summary::default()
    };

    for file in files {
        let session = Session::new(Scope::Ephemeral)?;
        let (clean, manifest, _report) = pipeline.clean_with_safety_net(
            &session,
            RawDocument::Text(file.text),
            &locale_chain,
        )?;
        let tokenized = match clean {
            CleanDocument::Text(text) => text,
            CleanDocument::Structured(_) => unreachable!("text input returns text output"),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "text input returned an unsupported clean document variant",
                )
                .into());
            }
        };
        let counts = counts_by_class(&manifest);

        summary.files += 1;
        summary.detections += manifest.len();
        for (class, count) in &counts {
            *summary.by_class.entry(class.clone()).or_insert(0) += count;
        }

        println!("\n== {} ==", file.label);
        print_counts(&counts);
        println!("Tokenized output:");
        println!("{tokenized}");
    }

    print_summary(&summary);
    Ok(())
}

fn build_pipeline() -> Result<Pipeline, Box<dyn Error>> {
    let phone = PiiClass::custom("phone");
    let case_id = PiiClass::custom("case_id");

    Ok(Pipeline::builder()
        .detector(RegexDetector::emails()?)
        .detector(RegexDetector::with_source(
            r"(?:\+1-555-01\d{2}|\+44-7700-900\d{3}|\+49 1555 0\d{6})",
            phone.clone(),
            "example.phone",
        )?)
        .detector(RegexDetector::with_source(
            r"\b(?:ORD|CASE)-\d{4,}\b",
            case_id.clone(),
            "example.case_id",
        )?)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(phone, Action::Tokenize))
        .rule(ClassRule::new(case_id, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()?)
}

fn parse_path_arg() -> Result<Option<PathBuf>, Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let mut path = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--path" => {
                let Some(value) = args.next() else {
                    return Err(invalid_input("--path requires a directory"));
                };
                path = Some(PathBuf::from(value));
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                return Err(invalid_input(format!("unrecognized argument: {other}")));
            }
        }
    }

    Ok(path)
}

fn print_usage() {
    eprintln!("usage: cargo run -p gaze-pii --example scan_folder -- [--path <dir>]");
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidInput, message.into()).into()
}

fn read_text_files(root: &Path) -> Result<(Vec<InputFile>, usize), Box<dyn Error>> {
    if !root.is_dir() {
        return Err(invalid_input(format!(
            "--path must point to a directory: {}",
            root.display()
        )));
    }

    let mut files = Vec::new();
    let mut skipped = 0;
    let mut queue = VecDeque::from([root.to_path_buf()]);

    while let Some(dir) = queue.pop_front() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                queue.push_back(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }

            match fs::read_to_string(&path) {
                Ok(text) => files.push(InputFile {
                    label: path
                        .strip_prefix(root)
                        .unwrap_or(path.as_path())
                        .display()
                        .to_string(),
                    text,
                }),
                Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                    skipped += 1;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    files.sort_by(|left, right| left.label.cmp(&right.label));
    Ok((files, skipped))
}

fn counts_by_class(manifest: &[EmittedTokenSpan]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for span in manifest {
        *counts.entry(span.class.class_name()).or_insert(0) += 1;
    }
    counts
}

fn print_counts(counts: &BTreeMap<String, usize>) {
    if counts.is_empty() {
        println!("Detections: none");
        return;
    }

    println!("Detections:");
    for (class, count) in counts {
        println!("  {class}: {count}");
    }
}

fn print_summary(summary: &Summary) {
    println!("\nSummary:");
    println!("  files scanned: {}", summary.files);
    println!("  files skipped: {}", summary.skipped);
    println!("  detections: {}", summary.detections);
    if !summary.by_class.is_empty() {
        println!("  detections by class:");
        for (class, count) in &summary.by_class {
            println!("    {class}: {count}");
        }
    }
}
