use std::{
    collections::HashSet,
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

const PRODUCTION_CRATES: &[&str] = &[
    "gaze",
    "gaze-types",
    "gaze-recognizers",
    "gaze-assembly",
    "gaze-cli",
];
const MARKER_PREFIX: &str = "// fixture-cited(";
const MARKER_SUFFIX: &str = ")";
const FIXTURE_PATTERNS: &[&str] = &[
    concat!("@", "example.invalid"),
    concat!("+1-555-", "01"),
    concat!("+44-7700-", "900"),
    concat!("+49 1555 ", "011"),
    concat!("Dr. ", "Schmidt"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureCitationError {
    MissingCitation { violations: Vec<Violation> },
    MissingTest { violations: Vec<Violation> },
    InvalidMarker { violations: Vec<Violation> },
    Io { path: PathBuf, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub path: PathBuf,
    pub line: usize,
    pub pattern: &'static str,
    pub text: String,
    pub citation: Option<Citation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub test_path: String,
    pub test_name: String,
}

pub type Result<T> = std::result::Result<T, FixtureCitationError>;

pub fn run() -> anyhow::Result<()> {
    let listed_tests = workspace_test_names()?;
    scan_root_with_tests(".", &listed_tests)?;
    println!("fixture_citation_lint: passed");
    Ok(())
}

pub fn scan_root_with_tests(root: impl AsRef<Path>, listed_tests: &HashSet<String>) -> Result<()> {
    let root = root.as_ref();
    let mut missing_citation = Vec::new();
    let mut missing_test = Vec::new();
    let mut invalid_marker = Vec::new();

    for file in production_files(root)? {
        let content = fs::read_to_string(&file).map_err(|error| io_error(&file, error))?;
        let active_lines = active_production_lines(&content);
        for (line_index, line) in active_lines.iter().enumerate() {
            let line_number = line_index + 1;
            let Some(pattern) = fixture_pattern(line) else {
                continue;
            };
            let marker_text = marker_for_line(&active_lines, line_index);
            let Some(marker_text) = marker_text else {
                missing_citation.push(violation(&file, line_number, pattern, line, None));
                continue;
            };
            let citation = match parse_marker(marker_text) {
                Some(citation) => citation,
                None => {
                    invalid_marker.push(violation(&file, line_number, pattern, line, None));
                    continue;
                }
            };
            if !listed_tests.contains(&citation.test_name) {
                missing_test.push(violation(&file, line_number, pattern, line, Some(citation)));
            }
        }
    }

    if !invalid_marker.is_empty() {
        return Err(FixtureCitationError::InvalidMarker {
            violations: invalid_marker,
        });
    }

    if !missing_citation.is_empty() {
        return Err(FixtureCitationError::MissingCitation {
            violations: missing_citation,
        });
    }

    if !missing_test.is_empty() {
        return Err(FixtureCitationError::MissingTest {
            violations: missing_test,
        });
    }

    Ok(())
}

fn workspace_test_names() -> anyhow::Result<HashSet<String>> {
    let output = Command::new("cargo")
        .arg("test")
        .arg("--workspace")
        .arg("--")
        .arg("--list")
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "failed to list workspace tests: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(parse_test_list(&String::from_utf8_lossy(&output.stdout)))
}

pub fn parse_test_list(output: &str) -> HashSet<String> {
    output
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .map(str::to_owned)
        .collect()
}

fn production_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for crate_name in PRODUCTION_CRATES {
        let src = root.join("crates").join(crate_name).join("src");
        if !src.exists() {
            continue;
        }
        collect_rs_files(&src, &mut files)?;
    }
    files.sort();
    Ok(files)
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(|error| io_error(dir, error))? {
        let entry = entry.map_err(|error| io_error(dir, error))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && !is_test_only_file(&path)
        {
            files.push(path);
        }
    }
    Ok(())
}

fn is_test_only_file(path: &Path) -> bool {
    matches!(
        path.file_stem().and_then(|stem| stem.to_str()),
        Some("tests" | "test_support")
    )
}

fn active_production_lines(content: &str) -> Vec<String> {
    let mut active = Vec::new();
    let mut pending_cfg_test = false;
    let mut test_mod_depth: Option<isize> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if test_mod_depth.is_some() {
            update_depth(&mut test_mod_depth, line);
            active.push(String::new());
            continue;
        }

        if trimmed.starts_with("#[cfg(test)]") {
            pending_cfg_test = true;
            active.push(String::new());
            continue;
        }

        if pending_cfg_test && trimmed.starts_with("mod ") {
            pending_cfg_test = false;
            let mut depth = 0;
            apply_brace_delta(&mut depth, line);
            if depth > 0 {
                test_mod_depth = Some(depth);
            }
            active.push(String::new());
            continue;
        }

        pending_cfg_test = false;
        active.push(line.to_owned());
    }

    active
}

fn update_depth(depth: &mut Option<isize>, line: &str) {
    if let Some(value) = depth.as_mut() {
        apply_brace_delta(value, line);
        if *value <= 0 {
            *depth = None;
        }
    }
}

fn apply_brace_delta(depth: &mut isize, line: &str) {
    for ch in line.chars() {
        match ch {
            '{' => *depth += 1,
            '}' => *depth -= 1,
            _ => {}
        }
    }
}

fn fixture_pattern(line: &str) -> Option<&'static str> {
    FIXTURE_PATTERNS
        .iter()
        .copied()
        .find(|pattern| line.contains(pattern))
}

fn marker_for_line(lines: &[String], line_index: usize) -> Option<&str> {
    if let Some(marker) = extract_marker(&lines[line_index]) {
        return Some(marker);
    }
    if line_index == 0 {
        return None;
    }
    extract_marker(&lines[line_index - 1])
}

fn extract_marker(line: &str) -> Option<&str> {
    let marker_start = line.find(MARKER_PREFIX)?;
    let after_prefix = marker_start + MARKER_PREFIX.len();
    let marker_end = line[after_prefix..].find(MARKER_SUFFIX)? + after_prefix;
    Some(&line[after_prefix..marker_end])
}

fn parse_marker(marker: &str) -> Option<Citation> {
    let (test_path, test_name) = marker.split_once(':')?;
    if test_path.is_empty()
        || test_name.is_empty()
        || !test_path.ends_with(".rs")
        || !test_name.contains("::")
    {
        return None;
    }
    Some(Citation {
        test_path: test_path.to_owned(),
        test_name: test_name.to_owned(),
    })
}

fn violation(
    path: &Path,
    line: usize,
    pattern: &'static str,
    text: &str,
    citation: Option<Citation>,
) -> Violation {
    Violation {
        path: path.to_path_buf(),
        line,
        pattern,
        text: text.trim().to_owned(),
        citation,
    }
}

fn io_error(path: &Path, error: io::Error) -> FixtureCitationError {
    FixtureCitationError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

impl fmt::Display for FixtureCitationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FixtureCitationError::MissingCitation { violations } => {
                writeln!(formatter, "FixtureCitationMissingCitation")?;
                write_violations(formatter, violations)
            }
            FixtureCitationError::MissingTest { violations } => {
                writeln!(formatter, "FixtureCitationMissingTest")?;
                write_violations(formatter, violations)
            }
            FixtureCitationError::InvalidMarker { violations } => {
                writeln!(formatter, "FixtureCitationInvalidMarker")?;
                write_violations(formatter, violations)
            }
            FixtureCitationError::Io { path, message } => {
                write!(formatter, "{}: {}", path.display(), message)
            }
        }
    }
}

impl Error for FixtureCitationError {}

fn write_violations(formatter: &mut fmt::Formatter<'_>, violations: &[Violation]) -> fmt::Result {
    for violation in violations {
        write!(
            formatter,
            "{}:{} matched {}: {}",
            violation.path.display(),
            violation.line,
            violation.pattern,
            violation.text
        )?;
        if let Some(citation) = &violation.citation {
            write!(
                formatter,
                " cited {}:{}",
                citation.test_path, citation.test_name
            )?;
        }
        writeln!(formatter)?;
    }
    Ok(())
}
