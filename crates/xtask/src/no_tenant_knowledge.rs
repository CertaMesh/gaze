use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

const PRODUCTION_CRATES: &[&str] = &["gaze", "gaze-recognizers", "gaze-assembly", "gaze-cli"];
const ALLOW_MARKER: &str = "// allow(tenant-fixture)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantKnowledgeError {
    DenylistHit { violations: Vec<Violation> },
    AllowMarkerInProductionScope { violations: Vec<Violation> },
    Io { path: PathBuf, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub path: PathBuf,
    pub line: usize,
    pub pattern: &'static str,
    pub text: String,
}

pub type Result<T> = std::result::Result<T, TenantKnowledgeError>;

pub fn run() -> anyhow::Result<()> {
    scan_root(".")?;
    println!("no_tenant_knowledge: passed");
    Ok(())
}

pub fn scan_root(root: impl AsRef<Path>) -> Result<()> {
    let root = root.as_ref();
    let mut allow_marker_violations = Vec::new();
    let mut denylist_violations = Vec::new();

    for file in production_files(root)? {
        let content = fs::read_to_string(&file).map_err(|error| io_error(&file, error))?;
        for (line_index, line) in content.lines().enumerate() {
            if line.contains(ALLOW_MARKER) {
                allow_marker_violations.push(Violation {
                    path: file.clone(),
                    line: line_index + 1,
                    pattern: ALLOW_MARKER,
                    text: line.trim().to_owned(),
                });
            }
            if let Some(pattern) = denylist_pattern(line) {
                denylist_violations.push(Violation {
                    path: file.clone(),
                    line: line_index + 1,
                    pattern,
                    text: line.trim().to_owned(),
                });
            }
        }
    }

    if !allow_marker_violations.is_empty() {
        return Err(TenantKnowledgeError::AllowMarkerInProductionScope {
            violations: allow_marker_violations,
        });
    }

    if !denylist_violations.is_empty() {
        return Err(TenantKnowledgeError::DenylistHit {
            violations: denylist_violations,
        });
    }

    Ok(())
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
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn denylist_pattern(line: &str) -> Option<&'static str> {
    if line.contains("order_id") {
        Some("order_id")
    } else if line.contains("Order_42") {
        Some("Order_42")
    } else if line.contains("Song_42") {
        Some("Song_42")
    } else if line.contains("User_7") {
        Some("User_7")
    } else {
        None
    }
}

fn io_error(path: &Path, error: io::Error) -> TenantKnowledgeError {
    TenantKnowledgeError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

impl fmt::Display for TenantKnowledgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TenantKnowledgeError::DenylistHit { violations } => {
                writeln!(formatter, "NoTenantKnowledgeDenylistHit")?;
                write_violations(formatter, violations)
            }
            TenantKnowledgeError::AllowMarkerInProductionScope { violations } => {
                writeln!(formatter, "AllowMarkerInProductionScope")?;
                write_violations(formatter, violations)
            }
            TenantKnowledgeError::Io { path, message } => {
                write!(formatter, "{}: {}", path.display(), message)
            }
        }
    }
}

impl Error for TenantKnowledgeError {}

fn write_violations(formatter: &mut fmt::Formatter<'_>, violations: &[Violation]) -> fmt::Result {
    for violation in violations {
        writeln!(
            formatter,
            "{}:{} matched {}: {}",
            violation.path.display(),
            violation.line,
            violation.pattern,
            violation.text
        )?;
    }
    Ok(())
}
