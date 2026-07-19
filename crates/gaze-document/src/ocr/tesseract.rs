//! Tesseract subprocess OCR adapter.
//!
//! Invokes the `tesseract` CLI directly via [`std::process::Command`].
//! Subprocess (rather than FFI) is intentional — adopters never need a
//! native build toolchain or `libtesseract` headers.
//!
//! ## Fail-closed contract
//!
//! * Missing binary → [`DocumentError::TesseractNotFound`] with per-OS install
//!   instructions in the message payload.
//! * Non-zero exit → [`DocumentError::TesseractFailed`] carrying captured
//!   stderr (truncated).
//!
//! ## Confidence
//!
//! The `tsv` output mode emits per-word confidence in column 11. We compute
//! a mean across words whose confidence is `>= 0`. Tesseract uses `-1` for
//! structural (block / paragraph / line) rows that carry no confidence.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use super::{BBox, ImageInput, OcrBackend, OcrError, OcrHints, OcrResult, OcrSpan};
use crate::DocumentError;

const STDERR_TRUNCATE_BYTES: usize = 4096;

/// Tesseract subprocess OCR backend.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct TesseractBackend {
    /// Tesseract language code (default `eng`).
    pub lang: String,
    /// Optional explicit binary path. `None` → look up `tesseract` on `PATH`.
    pub binary: Option<std::path::PathBuf>,
}

impl TesseractBackend {
    /// Builds the adapter with `eng` language and `tesseract` on `PATH`.
    pub fn new() -> Self {
        Self {
            lang: "eng".to_string(),
            binary: None,
        }
    }

    /// Builds the adapter with an explicit language code.
    pub fn with_lang(lang: impl Into<String>) -> Self {
        Self {
            lang: lang.into(),
            binary: None,
        }
    }

    /// Runs OCR on a file already on disk.
    ///
    /// Calls `tesseract <path> stdout -l <lang> tsv`, parses the TSV stream
    /// for text + per-word confidence in a single subprocess invocation.
    pub fn extract_from_file(&self, path: &Path) -> Result<OcrResult, DocumentError> {
        self.extract_from_file_with_lang(path, &self.lang)
    }

    fn extract_from_file_with_lang(
        &self,
        path: &Path,
        lang: &str,
    ) -> Result<OcrResult, DocumentError> {
        let tsv = self.run_tesseract_tsv(path, lang)?;
        Ok(parse_tsv_result(&tsv, lang))
    }

    fn run_tesseract_tsv(&self, path: &Path, lang: &str) -> Result<String, DocumentError> {
        let binary: &std::ffi::OsStr = self
            .binary
            .as_deref()
            .map(AsRef::as_ref)
            .unwrap_or_else(|| "tesseract".as_ref());

        #[cfg(target_os = "macos")]
        let canonical_path = path.canonicalize()?;
        #[cfg(target_os = "macos")]
        let path = canonical_path.as_path();

        let output = Command::new(binary)
            .arg(path)
            .arg("stdout")
            .arg("-l")
            .arg(lang)
            .arg("tsv")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => DocumentError::TesseractNotFound(install_hint()),
                _ => DocumentError::Io(err),
            })?;

        if !output.status.success() {
            let stderr = truncate_stderr(&output.stderr);
            return Err(DocumentError::TesseractFailed {
                status: output.status.code().unwrap_or(-1),
                stderr,
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Runs OCR on an in-memory image payload.
    ///
    /// Writes `bytes` to a tempfile (preserving any input extension via the
    /// caller's choice of `extension`) so Tesseract auto-detects the image
    /// format from the suffix. Cleaned up on drop.
    pub fn extract_from_bytes(
        &self,
        bytes: &[u8],
        extension: &str,
    ) -> Result<OcrResult, DocumentError> {
        let suffix = format!(".{extension}");
        let mut file = tempfile::Builder::new()
            .prefix("gaze-document-ocr-")
            .suffix(suffix.as_str())
            .tempfile()?;
        file.write_all(bytes)?;
        file.flush()?;
        let path = file.path().to_path_buf();
        self.extract_from_file(&path)
    }
}

impl Default for TesseractBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl OcrBackend for TesseractBackend {
    fn name(&self) -> &str {
        "tesseract"
    }

    fn recognize(&self, image: ImageInput, hints: OcrHints) -> Result<Vec<OcrSpan>, OcrError> {
        let suffix = format!(".{}", image.format.extension());
        let mut file = tempfile::Builder::new()
            .prefix("gaze-document-ocr-")
            .suffix(suffix.as_str())
            .tempfile()
            .map_err(|err| OcrError::Internal(err.to_string()))?;
        file.write_all(&image.bytes)
            .map_err(|err| OcrError::Internal(err.to_string()))?;
        file.flush()
            .map_err(|err| OcrError::Internal(err.to_string()))?;
        let tsv = self
            .run_tesseract_tsv(file.path(), hints.primary_language())
            .map_err(document_error_to_ocr_error)?;
        Ok(parse_tsv_spans(&tsv))
    }
}

fn parse_tsv_result(tsv: &str, lang: &str) -> OcrResult {
    parse_tsv(tsv, lang)
}

fn parse_tsv_spans(tsv: &str) -> Vec<OcrSpan> {
    let mut spans = Vec::new();

    for (idx, line) in tsv.lines().enumerate() {
        if idx == 0 || line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 {
            continue;
        }
        let level: u32 = cols[0].parse().unwrap_or(0);
        if level != 5 {
            continue;
        }
        let word = cols[11];
        if word.is_empty() {
            continue;
        }
        let confidence = cols[10]
            .parse::<f32>()
            .ok()
            .filter(|conf| *conf >= 0.0)
            .map(|conf| (conf / 100.0).clamp(0.0, 1.0));
        spans.push(OcrSpan {
            text: word.to_string(),
            bbox: BBox {
                x: cols[6].parse().unwrap_or(0),
                y: cols[7].parse().unwrap_or(0),
                w: cols[8].parse().unwrap_or(0),
                h: cols[9].parse().unwrap_or(0),
            },
            confidence,
        });
    }

    spans
}

fn document_error_to_ocr_error(err: DocumentError) -> OcrError {
    match err {
        DocumentError::TesseractNotFound(hint) => OcrError::InitFailed(hint),
        DocumentError::TesseractFailed { status, stderr } => {
            OcrError::RecognizeFailed(format!("status {status}: {stderr}"))
        }
        DocumentError::Io(err) => OcrError::Internal(err.to_string()),
        other => OcrError::Internal(other.to_string()),
    }
}

fn parse_tsv(tsv: &str, lang: &str) -> OcrResult {
    let mut text = String::new();
    let mut current_line: Option<(u64, u64, u64)> = None;
    let mut current_text = String::new();
    let mut conf_sum: f64 = 0.0;
    let mut conf_count: usize = 0;

    for (idx, line) in tsv.lines().enumerate() {
        if idx == 0 || line.is_empty() {
            // Skip header row + blank trailers.
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 {
            continue;
        }
        // Tesseract TSV columns: level, page_num, block_num, par_num,
        // line_num, word_num, left, top, width, height, conf, text
        let level: u32 = cols[0].parse().unwrap_or(0);
        if level != 5 {
            continue; // word rows only
        }
        let block_num: u64 = cols[2].parse().unwrap_or(0);
        let par_num: u64 = cols[3].parse().unwrap_or(0);
        let line_num: u64 = cols[4].parse().unwrap_or(0);
        let conf: f32 = cols[10].parse().unwrap_or(-1.0);
        let word = cols[11];
        if word.is_empty() {
            continue;
        }

        let line_key = (block_num, par_num, line_num);
        if current_line != Some(line_key) {
            if !current_text.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&current_text);
                current_text.clear();
            }
            current_line = Some(line_key);
        }
        if !current_text.is_empty() {
            current_text.push(' ');
        }
        current_text.push_str(word);

        if conf >= 0.0 {
            conf_sum += conf as f64;
            conf_count += 1;
        }
    }
    if !current_text.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&current_text);
    }

    let mean_confidence = if conf_count == 0 {
        None
    } else {
        Some((conf_sum / conf_count as f64) as f32)
    };
    OcrResult {
        text,
        mean_confidence,
        word_count: conf_count,
        lang: lang.to_string(),
    }
}

fn truncate_stderr(bytes: &[u8]) -> String {
    if bytes.len() <= STDERR_TRUNCATE_BYTES {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut out = String::from_utf8_lossy(&bytes[..STDERR_TRUNCATE_BYTES]).into_owned();
    out.push_str("\n…(truncated)");
    out
}

fn install_hint() -> String {
    if cfg!(target_os = "macos") {
        "Install via `brew install tesseract` (or `port install tesseract`).".to_string()
    } else if cfg!(target_os = "linux") {
        "Install via `apt-get install tesseract-ocr` (Debian/Ubuntu), \
         `dnf install tesseract` (Fedora), or `pacman -S tesseract` (Arch)."
            .to_string()
    } else if cfg!(target_os = "windows") {
        "Install via `winget install --id UB-Mannheim.TesseractOCR` or download the \
         UB-Mannheim build and add it to PATH."
            .to_string()
    } else {
        "Install Tesseract from https://github.com/tesseract-ocr/tesseract and ensure it is on PATH.".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tsv_groups_words_into_lines() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
1\t1\t0\t0\t0\t0\t0\t0\t100\t100\t-1\t\n\
5\t1\t1\t1\t1\t1\t0\t0\t40\t10\t91\tBill\n\
5\t1\t1\t1\t1\t2\t40\t0\t30\t10\t93\tto:\n\
5\t1\t1\t1\t2\t1\t0\t20\t60\t10\t87\tJane\n\
5\t1\t1\t1\t2\t2\t60\t20\t60\t10\t89\tDoe\n";
        let result = parse_tsv(tsv, "eng");
        assert_eq!(result.text, "Bill to:\nJane Doe");
        assert_eq!(result.word_count, 4);
        let conf = result.mean_confidence.expect("expected mean confidence");
        assert!((conf - 90.0).abs() < 1.0);
    }

    #[test]
    fn parse_tsv_empty_yields_empty_text() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n";
        let result = parse_tsv(tsv, "eng");
        assert!(result.text.is_empty());
        assert_eq!(result.word_count, 0);
        assert!(result.mean_confidence.is_none());
    }

    #[test]
    fn install_hint_is_non_empty() {
        assert!(!install_hint().is_empty());
    }
}
