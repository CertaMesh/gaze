use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use crate::repo::{production_files, tenant_knowledge_file, PRODUCTION_CRATES};
const ALLOW_MARKER: &str = "// allow(tenant-fixture)";
// Denylist literals split via concat!() so this source file does not contain
// the contiguous strings the gate scans for. This is meta-Potemkin avoidance:
// the gate's own implementation must not appear to violate its own discipline,
// even though crates/xtask/ is excluded from the gate's scan scope per plan S3.
const DENYLIST: &[&str] = &[
    concat!("ord", "er_id"),
    concat!("Ord", "er_42"),
    concat!("Son", "g_42"),
    concat!("Use", "r_7"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantKnowledgeError {
    DenylistHit { violations: Vec<Violation> },
    AllowMarkerInProductionScope { violations: Vec<Violation> },
    EmptyScan,
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
    let mut cases_checked = 0usize;

    for file in
        production_files(root, PRODUCTION_CRATES, tenant_knowledge_file).map_err(|error| {
            TenantKnowledgeError::Io {
                path: error.path,
                message: error.message,
            }
        })?
    {
        cases_checked += 1;
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

    if cases_checked == 0 {
        return Err(TenantKnowledgeError::EmptyScan);
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

fn denylist_pattern(line: &str) -> Option<&'static str> {
    DENYLIST
        .iter()
        .copied()
        .find(|pattern| line.contains(pattern))
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
            TenantKnowledgeError::EmptyScan => {
                write!(formatter, "no_tenant_knowledge: no cases iterated")
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
