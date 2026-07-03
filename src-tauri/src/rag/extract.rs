//! Document text extraction. Each supported format resolves to plain UTF-8
//! text; everything downstream (chunking, embedding) is format-agnostic.

use std::path::Path;

use crate::error::{AthanorError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    Text,
    Pdf,
    Docx,
    Unsupported,
}

pub fn kind(path: &Path) -> DocKind {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "txt" | "md" | "markdown" | "rst" | "org" | "csv" | "json" | "yaml" | "yml" | "toml"
        | "xml" | "html" | "htm" | "tex" | "log" | "rs" | "py" | "js" | "ts" | "tsx" | "jsx"
        | "go" | "java" | "c" | "h" | "cpp" | "hpp" | "cs" | "rb" | "php" | "swift" | "kt"
        | "sh" | "ps1" | "sql" | "css" | "scss" | "vue" | "svelte" => DocKind::Text,
        "pdf" => DocKind::Pdf,
        "docx" => DocKind::Docx,
        _ => DocKind::Unsupported,
    }
}

/// Extract text, dispatching on extension. Returns an error the UI can show
/// verbatim (encrypted PDF, corrupt file, unsupported type).
pub fn extract(path: &Path) -> Result<String> {
    let text = match kind(path) {
        DocKind::Text => extract_text(path)?,
        DocKind::Pdf => extract_pdf(path)?,
        DocKind::Docx => extract_docx(path)?,
        DocKind::Unsupported => {
            // Last resort: try UTF-8. Fails cleanly on binary files.
            extract_text(path).map_err(|_| {
                AthanorError::Rag(format!(
                    "unsupported file type: {}",
                    path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default()
                ))
            })?
        }
    };
    let cleaned = normalize(&text);
    if cleaned.trim().is_empty() {
        return Err(AthanorError::Rag(
            "no extractable text — the file may be scanned images or empty".into(),
        ));
    }
    Ok(cleaned)
}

fn extract_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    // Tolerate a UTF-8 BOM and invalid sequences rather than failing.
    let start = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    Ok(String::from_utf8_lossy(&bytes[start..]).into_owned())
}

fn extract_pdf(path: &Path) -> Result<String> {
    pdf_extract::extract_text(path)
        .map_err(|e| AthanorError::Rag(format!("could not read PDF: {e}")))
}

/// DOCX is a zip; the body text lives in word/document.xml inside <w:t> runs,
/// with <w:p> as paragraph and <w:br>/<w:tab> as breaks. Parsed with a
/// streaming XML reader — no heavy office dependency.
fn extract_docx(path: &Path) -> Result<String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AthanorError::Rag(format!("not a valid .docx: {e}")))?;
    let mut xml = String::new();
    {
        use std::io::Read;
        let mut doc = zip
            .by_name("word/document.xml")
            .map_err(|_| AthanorError::Rag("`.docx` has no document body".into()))?;
        doc.read_to_string(&mut xml)?;
    }

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(false);
    let mut out = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"t" => in_text = true,
            Ok(Event::End(e)) if e.local_name().as_ref() == b"t" => in_text = false,
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"p" => out.push('\n'),
                b"br" => out.push('\n'),
                b"tab" => out.push('\t'),
                _ => {}
            },
            Ok(Event::End(e)) if e.local_name().as_ref() == b"p" => out.push('\n'),
            Ok(Event::Text(t)) if in_text => {
                out.push_str(&t.unescape().unwrap_or_default());
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AthanorError::Rag(format!("malformed .docx XML: {e}"))),
            _ => {}
        }
    }
    Ok(out)
}

/// Collapse runs of blank lines and trailing whitespace so chunk boundaries
/// land on real paragraphs, not PDF layout noise.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0;
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_detection() {
        assert_eq!(kind(Path::new("a.md")), DocKind::Text);
        assert_eq!(kind(Path::new("a.rs")), DocKind::Text);
        assert_eq!(kind(Path::new("a.PDF")), DocKind::Pdf);
        assert_eq!(kind(Path::new("a.docx")), DocKind::Docx);
        assert_eq!(kind(Path::new("a.png")), DocKind::Unsupported);
    }

    #[test]
    fn normalize_collapses_blank_runs() {
        // Multiple blank lines collapse to a single paragraph separator.
        assert_eq!(normalize("a\n\n\n\nb\n"), "a\n\nb\n");
        assert_eq!(normalize("a\nb"), "a\nb\n");
    }
}
