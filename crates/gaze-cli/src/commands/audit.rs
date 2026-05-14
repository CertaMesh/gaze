use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;

use chrono::DateTime;
use clap::ValueEnum;
use gaze_audit::{
    AuditFilter, AuditLogRow, LeakSuspectRow, SqliteLogger, AUDIT_RESTRICTED_COLUMNS,
    SAFETY_NET_RESTRICTED_COLUMNS,
};
use gaze_types::{AmbiguityRecord, ValidatorFailReason};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::CliError;

pub(crate) struct Args {
    pub(crate) audit_db: PathBuf,
    pub(crate) class: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) action: Option<String>,
    pub(crate) document_kind: Option<String>,
    pub(crate) from_iso8601: Option<String>,
    pub(crate) to_iso8601: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) has_ambiguity: bool,
    pub(crate) ambiguity_reason: Option<String>,
    pub(crate) collision_family: Option<String>,
    pub(crate) collision_variant: Option<String>,
}

pub(crate) struct PurgeArgs {
    pub(crate) audit_db: PathBuf,
    pub(crate) before: String,
    pub(crate) dry_run: bool,
}

pub(crate) struct SafetyNetArgs {
    pub(crate) audit_db: PathBuf,
    pub(crate) leak_kind: Option<String>,
    pub(crate) raw_label: Option<String>,
    pub(crate) mapped_class: Option<String>,
    pub(crate) field_path: Option<String>,
    pub(crate) from_iso8601: Option<String>,
    pub(crate) to_iso8601: Option<String>,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExportFormat {
    Jsonl,
}

#[derive(Serialize)]
struct PurgeResponse {
    dry_run: bool,
    matched: usize,
    deleted: usize,
}

pub(crate) fn query(args: Args) -> std::result::Result<(), CliError> {
    let rows = read_rows(&args)?;
    let mut stdout = io::stdout().lock();
    let include_ambiguity = rows.iter().any(|row| row.ambiguity_record.is_some());
    if include_ambiguity {
        writeln!(stdout, "{}\tambiguity", AUDIT_RESTRICTED_COLUMNS.join("\t"))
            .map_err(|_| CliError::Io)?;
    } else {
        writeln!(stdout, "{}", AUDIT_RESTRICTED_COLUMNS.join("\t")).map_err(|_| CliError::Io)?;
    }
    for row in rows {
        let base = format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row.source,
            row.recognizer_id.as_deref().unwrap_or(""),
            row.recognizer_version_id.as_deref().unwrap_or(""),
            row.class,
            row.action,
            row.field_name.as_deref().unwrap_or(""),
            row.document_kind,
            row.conflict_loser,
            row.decided_by,
            row.created_at
                .map(|created_at| created_at.to_string())
                .unwrap_or_default(),
            row.session_id.unwrap_or_default(),
            row.snapshot_scheme,
            row.snapshot_alg,
            row.snapshot_key_version
                .map(|version| version.to_string())
                .unwrap_or_default(),
            row.validator_fail_reason.as_deref().unwrap_or(""),
            row.ambiguity_record.as_deref().unwrap_or(""),
            row.collision_family.as_deref().unwrap_or(""),
            row.collision_variant.as_deref().unwrap_or(""),
            row.fallback_triggered.as_deref().unwrap_or("")
        );
        if include_ambiguity {
            writeln!(
                stdout,
                "{base}\t{}",
                ambiguity_display(&row.ambiguity_record)?
            )
            .map_err(|_| CliError::Io)?;
        } else {
            writeln!(stdout, "{base}").map_err(|_| CliError::Io)?;
        }
    }
    Ok(())
}

pub(crate) fn query_safety_net(args: SafetyNetArgs) -> std::result::Result<(), CliError> {
    let rows = read_safety_net_rows(&args)?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", SAFETY_NET_RESTRICTED_COLUMNS.join("\t")).map_err(|_| CliError::Io)?;
    for row in rows {
        writeln!(
            stdout,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row.id,
            row.safety_net_id,
            row.raw_label,
            row.mapped_class,
            row.leak_kind,
            row.span_len,
            row.document_kind,
            row.field_path.unwrap_or_default(),
            row.score.map(|score| score.to_string()).unwrap_or_default(),
            row.created_at,
            row.session_id.unwrap_or_default(),
            row.pipeline_class.unwrap_or_default(),
            row.safety_net_replay_hash.unwrap_or_default(),
            row.backend_id.unwrap_or_default(),
            row.backend_version.unwrap_or_default(),
            row.decoding_params_hash.unwrap_or_default(),
            row.telemetry_kind.unwrap_or_default()
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

pub(crate) fn purge(args: PurgeArgs) -> std::result::Result<(), CliError> {
    let before = parse_iso8601_utc_epoch_ms(&args.before)?;
    let logger = SqliteLogger::new(&args.audit_db).map_err(|_| CliError::Pipeline)?;
    let matched = logger
        .count_before(before)
        .map_err(|_| CliError::Pipeline)?;
    let deleted = if args.dry_run {
        0
    } else {
        logger
            .purge_before(before)
            .map_err(|_| CliError::Pipeline)?
    };
    let json = serde_json::to_string(&PurgeResponse {
        dry_run: args.dry_run,
        matched,
        deleted,
    })
    .map_err(|_| CliError::Pipeline)?;
    println!("{json}");
    Ok(())
}

fn read_rows(args: &Args) -> std::result::Result<Vec<AuditLogRow>, CliError> {
    // Keep CLI flag parse rejection identical to any future TOML audit-filter
    // default surface: invalid timestamps are PolicyConfig/exit 2, never clap.
    let from_epoch_ms = parse_iso8601_epoch_ms(args.from_iso8601.as_deref())?;
    let to_epoch_ms = parse_iso8601_epoch_ms(args.to_iso8601.as_deref())?;
    let filter = AuditFilter {
        class: args.class.clone(),
        source: args.source.clone(),
        action: args.action.clone(),
        document_kind: args.document_kind.clone(),
        raw_label: None,
        field_path: None,
        from_epoch_ms,
        to_epoch_ms,
        session_id: args.session_id.clone(),
        snapshot_scheme: None,
        snapshot_alg: None,
        snapshot_key_version: None,
        has_ambiguity: args.has_ambiguity.then_some(true),
        ambiguity_reason: args
            .ambiguity_reason
            .as_deref()
            .map(normalize_kebab_variant),
        collision_family: args.collision_family.clone(),
        collision_variant: args.collision_variant.clone(),
        recognizer_id: None,
        recognizer_version_id: None,
    };
    SqliteLogger::query(&args.audit_db, &filter).map_err(|_| CliError::Pipeline)
}

fn read_safety_net_rows(
    args: &SafetyNetArgs,
) -> std::result::Result<Vec<LeakSuspectRow>, CliError> {
    let from_epoch_ms = parse_iso8601_epoch_ms(args.from_iso8601.as_deref())?;
    let to_epoch_ms = parse_iso8601_epoch_ms(args.to_iso8601.as_deref())?;
    let filter = AuditFilter {
        class: args.mapped_class.clone(),
        source: None,
        action: args.leak_kind.clone(),
        document_kind: None,
        raw_label: args.raw_label.clone(),
        field_path: args.field_path.clone(),
        from_epoch_ms,
        to_epoch_ms,
        session_id: None,
        snapshot_scheme: None,
        snapshot_alg: None,
        snapshot_key_version: None,
        has_ambiguity: None,
        ambiguity_reason: None,
        collision_family: None,
        collision_variant: None,
        recognizer_id: None,
        recognizer_version_id: None,
    };
    SqliteLogger::query_safety_net(&args.audit_db, &filter).map_err(|_| CliError::Pipeline)
}

fn parse_iso8601_epoch_ms(value: Option<&str>) -> std::result::Result<Option<i64>, CliError> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|datetime| datetime.timestamp_millis())
                .map_err(|_| {
                    CliError::PolicyConfigDetail(format!(
                        "invalid audit ISO 8601 timestamp: {value:?}"
                    ))
                })
        })
        .transpose()
}

fn parse_iso8601_utc_epoch_ms(input: &str) -> std::result::Result<i64, CliError> {
    if input.is_empty() || !input.ends_with('Z') || !input.contains('T') || input.contains('t') {
        return Err(CliError::AuditPurgeIso8601 {
            input: input.to_string(),
        });
    }
    DateTime::parse_from_rfc3339(input)
        .map(|datetime| datetime.timestamp_millis())
        .map_err(|_| CliError::AuditPurgeIso8601 {
            input: input.to_string(),
        })
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
        let row = JsonlRow::try_from(row)?;
        serde_json::to_writer(&mut writer, &row).map_err(|_| CliError::Io)?;
        writer.write_all(b"\n").map_err(|_| CliError::Io)?;
    }
    writer.flush().map_err(|_| CliError::Io)
}

fn normalize_kebab_variant(value: &str) -> String {
    value.replace('-', "_")
}

fn parse_json_opt<T: DeserializeOwned>(value: Option<String>) -> Result<Option<T>, CliError> {
    value
        .map(|json| serde_json::from_str(&json).map_err(|_| CliError::Pipeline))
        .transpose()
}

fn ambiguity_display(value: &Option<String>) -> Result<String, CliError> {
    let Some(json) = value else {
        return Ok(String::new());
    };
    let record: AmbiguityRecord = serde_json::from_str(json).map_err(|_| CliError::Pipeline)?;
    let losing = record
        .losing_candidates
        .iter()
        .map(|candidate| {
            format!(
                "{}:{}",
                candidate.class.to_canonical_str(),
                candidate.recognizer_id
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let reason = serde_json::to_string(&record.reason).map_err(|_| CliError::Pipeline)?;
    Ok(format!(
        "class={} reason={} losing=[{}]",
        record.ambiguity_class.to_canonical_str(),
        reason.trim_matches('"'),
        losing
    ))
}

#[derive(Serialize)]
struct JsonlRow {
    source: String,
    recognizer_id: Option<String>,
    recognizer_version_id: Option<String>,
    class: String,
    action: String,
    field_name: Option<String>,
    document_kind: String,
    conflict_loser: bool,
    decided_by: String,
    created_at: Option<i64>,
    session_id: Option<String>,
    snapshot_scheme: String,
    snapshot_alg: String,
    snapshot_key_version: Option<i64>,
    validator_fail_reason: Option<ValidatorFailReason>,
    ambiguity_record: Option<AmbiguityRecord>,
    collision_family: Option<String>,
    collision_variant: Option<String>,
    fallback_triggered: Option<String>,
}

impl TryFrom<AuditLogRow> for JsonlRow {
    type Error = CliError;

    fn try_from(row: AuditLogRow) -> Result<Self, Self::Error> {
        Ok(Self {
            source: row.source,
            recognizer_id: row.recognizer_id,
            recognizer_version_id: row.recognizer_version_id,
            class: row.class,
            action: row.action,
            field_name: row.field_name,
            document_kind: row.document_kind,
            conflict_loser: row.conflict_loser,
            decided_by: row.decided_by,
            created_at: row.created_at,
            session_id: row.session_id,
            snapshot_scheme: row.snapshot_scheme,
            snapshot_alg: row.snapshot_alg,
            snapshot_key_version: row.snapshot_key_version,
            validator_fail_reason: parse_json_opt(row.validator_fail_reason)?,
            ambiguity_record: parse_json_opt(row.ambiguity_record)?,
            collision_family: row.collision_family,
            collision_variant: row.collision_variant,
            fallback_triggered: row.fallback_triggered,
        })
    }
}
