use std::path::PathBuf;

use async_trait::async_trait;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::adapter::{AdapterError, LogAdapter};

pub struct LaravelLogAdapter {
    path: PathBuf,
}

impl LaravelLogAdapter {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    async fn read_all(&self) -> Result<Vec<String>, AdapterError> {
        let file = File::open(&self.path)
            .await
            .map_err(|err| AdapterError::Connection(format!("{}: {err}", self.path.display())))?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut out = Vec::new();

        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|err| AdapterError::Query(err.to_string()))?
        {
            out.push(line);
        }

        Ok(out)
    }
}

#[async_trait]
impl LogAdapter for LaravelLogAdapter {
    async fn tail(&self, limit: usize) -> Result<Vec<String>, AdapterError> {
        let lines = self.read_all().await?;
        let start = lines.len().saturating_sub(limit);
        Ok(lines.into_iter().skip(start).collect())
    }

    async fn search(
        &self,
        pattern: &str,
        level: Option<&str>,
        limit: usize,
    ) -> Result<Vec<String>, AdapterError> {
        let lines = self.read_all().await?;
        let needle = pattern.to_ascii_lowercase();
        let level = level.map(|value| value.to_ascii_lowercase());

        Ok(lines
            .into_iter()
            .filter(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains(&needle)
                    && level
                        .as_ref()
                        .map(|value| lower.contains(&format!(".{}:", value)))
                        .unwrap_or(true)
            })
            .take(limit)
            .collect())
    }

    async fn context(&self, request_id: &str) -> Result<Vec<String>, AdapterError> {
        let lines = self.read_all().await?;
        let needle = format!("request_id={request_id}");
        Ok(lines
            .into_iter()
            .filter(|line| line.contains(&needle))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture() -> LaravelLogAdapter {
        LaravelLogAdapter::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("laravel-sample.log"),
        )
    }

    #[tokio::test]
    async fn search_filters_by_pattern_and_level() {
        let adapter = fixture();
        let hits = adapter
            .search("Integrity", Some("ERROR"), 10)
            .await
            .expect("search");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].contains("Duplicate entry"));
    }

    #[tokio::test]
    async fn tail_returns_last_lines() {
        let adapter = fixture();
        let tail = adapter.tail(2).await.expect("tail");
        assert_eq!(tail.len(), 2);
        assert!(tail[1].contains("request_id=req_2"));
    }

    #[tokio::test]
    async fn context_groups_by_request_id() {
        let adapter = fixture();
        let context = adapter.context("req_1").await.expect("context");
        assert_eq!(context.len(), 4);
        assert!(context.iter().all(|line| line.contains("req_1")));
    }
}
