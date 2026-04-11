//! Laravel single-file log adapter. Reads `storage/logs/laravel.log` (or a
//! per-day file) and parses the conventional Laravel line format:
//!
//!   [YYYY-MM-DD HH:MM:SS] env.LEVEL: message
//!
//! Lines that don't match this pattern are kept as continuation of the
//! previous entry (common for multi-line stack traces).

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::adapter::{AdapterError, LogAdapter, LogLine};

static LINE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^\[(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\] (?P<env>[^.]+)\.(?P<level>[A-Z]+): (?P<msg>.*)$",
    )
    .expect("log line regex")
});

pub struct LaravelLogAdapter {
    path: PathBuf,
}

impl LaravelLogAdapter {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    async fn read_all(&self) -> Result<Vec<LogLine>, AdapterError> {
        let file = File::open(&self.path)
            .await
            .map_err(|e| AdapterError::Connection(format!("{}: {e}", self.path.display())))?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let mut out: Vec<LogLine> = Vec::new();
        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| AdapterError::Query(e.to_string()))?
        {
            if let Some(cap) = LINE_RE.captures(&line) {
                out.push(LogLine {
                    timestamp: cap["ts"].to_string(),
                    level: cap["level"].to_string(),
                    message: cap["msg"].to_string(),
                    raw: line,
                });
            } else if let Some(last) = out.last_mut() {
                // Continuation line: attach to previous.
                last.raw.push('\n');
                last.raw.push_str(&line);
                last.message.push('\n');
                last.message.push_str(&line);
            }
        }
        Ok(out)
    }
}

#[async_trait]
impl LogAdapter for LaravelLogAdapter {
    async fn search(
        &self,
        pattern: &str,
        level: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LogLine>, AdapterError> {
        let lines = self.read_all().await?;
        let needle = pattern.to_ascii_lowercase();
        let out: Vec<_> = lines
            .into_iter()
            .filter(|l| {
                level
                    .map(|lvl| l.level.eq_ignore_ascii_case(lvl))
                    .unwrap_or(true)
                    && l.message.to_ascii_lowercase().contains(&needle)
            })
            .take(limit)
            .collect();
        Ok(out)
    }

    async fn tail(&self, n: usize) -> Result<Vec<LogLine>, AdapterError> {
        let lines = self.read_all().await?;
        let start = lines.len().saturating_sub(n);
        Ok(lines.into_iter().skip(start).collect())
    }

    async fn context(&self, request_id: &str) -> Result<Vec<LogLine>, AdapterError> {
        let needle = format!("request_id={request_id}");
        let lines = self.read_all().await?;
        Ok(lines
            .into_iter()
            .filter(|l| l.raw.contains(&needle))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> LaravelLogAdapter {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/laravel-sample.log");
        LaravelLogAdapter::new(p)
    }

    #[tokio::test]
    async fn search_by_pattern_and_level() {
        let a = fixture();
        let hits = a.search("Integrity", Some("ERROR"), 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].message.contains("Duplicate entry"));
    }

    #[tokio::test]
    async fn tail_returns_last_n_lines() {
        let a = fixture();
        let tail = a.tail(2).await.unwrap();
        assert_eq!(tail.len(), 2);
        assert!(tail.last().unwrap().message.contains("request_id=req_2"));
    }

    #[tokio::test]
    async fn context_by_request_id_groups_lines() {
        let a = fixture();
        let ctx = a.context("req_1").await.unwrap();
        assert_eq!(ctx.len(), 4);
        assert!(ctx.iter().all(|l| l.raw.contains("req_1")));
    }
}
