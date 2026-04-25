use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;

use clap::ValueEnum;
use gaze::{AuditFilter, AuditLogRow, SqliteLogger, AUDIT_RESTRICTED_COLUMNS};
use serde::Serialize;

use crate::error::CliError;

pub(crate) struct Args {
    pub(crate) audit_db: PathBuf,
    pub(crate) class: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) action: Option<String>,
    pub(crate) document_kind: Option<String>,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExportFormat {
    Jsonl,
}

pub(crate) fn query(args: Args) -> std::result::Result<(), CliError> {
    let rows = read_rows(&args)?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", AUDIT_RESTRICTED_COLUMNS.join("\t")).map_err(|_| CliError::Io)?;
    for row in rows {
        writeln!(
            stdout,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row.source,
            row.class,
            row.action,
            row.field_name.as_deref().unwrap_or(""),
            row.document_kind,
            row.conflict_loser,
            row.decided_by
        )
        .map_err(|_| CliError::Io)?;
    }
    Ok(())
}

pub(crate) fn export(
    args: Args,
    format: ExportFormat,
    output: Option<PathBuf>,
) -> std::result::Result<(), CliError> {
    let rows = read_rows(&args)?;
    match format {
        ExportFormat::Jsonl => write_jsonl(rows, output),
    }
}

fn read_rows(args: &Args) -> std::result::Result<Vec<AuditLogRow>, CliError> {
    let filter = AuditFilter {
        class: args.class.clone(),
        source: args.source.clone(),
        action: args.action.clone(),
        document_kind: args.document_kind.clone(),
    };
    SqliteLogger::query(&args.audit_db, &filter).map_err(|_| CliError::Pipeline)
}

fn write_jsonl(
    rows: Vec<AuditLogRow>,
    output: Option<PathBuf>,
) -> std::result::Result<(), CliError> {
    let mut writer: Box<dyn Write> = match output {
        Some(path) => Box::new(File::create(path).map_err(|_| CliError::Io)?),
        None => Box::new(io::stdout().lock()),
    };
    for row in rows {
        let row = JsonlRow::from(row);
        serde_json::to_writer(&mut writer, &row).map_err(|_| CliError::Io)?;
        writer.write_all(b"\n").map_err(|_| CliError::Io)?;
    }
    writer.flush().map_err(|_| CliError::Io)
}

#[derive(Serialize)]
struct JsonlRow {
    source: String,
    class: String,
    action: String,
    field_name: Option<String>,
    document_kind: String,
    conflict_loser: bool,
    decided_by: String,
}

impl From<AuditLogRow> for JsonlRow {
    fn from(row: AuditLogRow) -> Self {
        Self {
            source: row.source,
            class: row.class,
            action: row.action,
            field_name: row.field_name,
            document_kind: row.document_kind,
            conflict_loser: row.conflict_loser,
            decided_by: row.decided_by,
        }
    }
}
