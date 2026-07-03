//! Paragraph-aware chunking with overlap.
//!
//! Targets ~1200 characters per chunk (~300 tokens) with ~180 characters of
//! overlap so a fact split across a boundary survives in at least one chunk.
//! Splits on paragraph breaks first, then hard-wraps any single paragraph
//! that exceeds the target.

const TARGET: usize = 1200;
const OVERLAP: usize = 180;
const MAX: usize = 1600;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub index: usize,
    pub text: String,
    /// Byte offset in the source document (for future highlight/preview).
    #[allow(dead_code)]
    pub start: usize,
}

/// Split a document into overlapping chunks. Deterministic and unit-tested.
pub fn chunk(text: &str) -> Vec<Chunk> {
    let paragraphs = split_paragraphs(text);
    let mut chunks = Vec::new();
    let mut buf = String::new();
    let mut buf_start = 0usize;

    let flush = |chunks: &mut Vec<Chunk>, buf: &mut String, start: usize| {
        let trimmed = buf.trim();
        if !trimmed.is_empty() {
            chunks.push(Chunk {
                index: chunks.len(),
                text: trimmed.to_string(),
                start,
            });
        }
        buf.clear();
    };

    for (para, offset) in paragraphs {
        if para.len() > MAX {
            // Oversized paragraph: flush current, then hard-wrap this one.
            flush(&mut chunks, &mut buf, buf_start);
            for piece in hard_wrap(&para) {
                chunks.push(Chunk {
                    index: chunks.len(),
                    text: piece,
                    start: offset,
                });
            }
            buf_start = offset + para.len();
            continue;
        }
        if buf.len() + para.len() + 2 > TARGET && !buf.is_empty() {
            flush(&mut chunks, &mut buf, buf_start);
            // Carry an overlap tail from the previous chunk for continuity.
            if let Some(prev) = chunks.last() {
                let tail = char_tail(&prev.text, OVERLAP);
                buf.push_str(&tail);
                buf.push_str("\n\n");
            }
            buf_start = offset;
        }
        if buf.is_empty() {
            buf_start = offset;
        }
        buf.push_str(&para);
        buf.push_str("\n\n");
    }
    flush(&mut chunks, &mut buf, buf_start);

    // Reindex so indices are contiguous after any hard-wrap interleaving.
    for (i, c) in chunks.iter_mut().enumerate() {
        c.index = i;
    }
    chunks
}

fn split_paragraphs(text: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut cur = String::new();
    let mut byte = 0;
    for line in text.split_inclusive('\n') {
        if line.trim().is_empty() {
            if !cur.trim().is_empty() {
                out.push((cur.trim().to_string(), start));
            }
            cur.clear();
            start = byte + line.len();
        } else {
            if cur.is_empty() {
                start = byte;
            }
            cur.push_str(line);
        }
        byte += line.len();
    }
    if !cur.trim().is_empty() {
        out.push((cur.trim().to_string(), start));
    }
    out
}

/// Hard-wrap a paragraph too big to be one chunk, on sentence-ish boundaries.
fn hard_wrap(para: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut cur = String::new();
    for sentence in para.split_inclusive(['.', '!', '?', '\n']) {
        if cur.len() + sentence.len() > TARGET && !cur.is_empty() {
            pieces.push(cur.trim().to_string());
            let tail = char_tail(&cur, OVERLAP);
            cur = tail;
        }
        cur.push_str(sentence);
        // A single sentence longer than MAX: split on char boundary.
        while cur.len() > MAX {
            let cut = floor_char_boundary(&cur, TARGET);
            pieces.push(cur[..cut].trim().to_string());
            cur = cur[cut.saturating_sub(OVERLAP)..].to_string();
        }
    }
    if !cur.trim().is_empty() {
        pieces.push(cur.trim().to_string());
    }
    pieces
}

fn char_tail(s: &str, want: usize) -> String {
    if s.len() <= want {
        return s.to_string();
    }
    let cut = ceil_char_boundary(s, s.len() - want);
    s[cut..].to_string()
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_chunk() {
        let c = chunk("Hello world.\n\nSecond paragraph.");
        assert_eq!(c.len(), 1);
        assert!(c[0].text.contains("Hello world"));
        assert!(c[0].text.contains("Second paragraph"));
    }

    #[test]
    fn long_text_splits_with_overlap() {
        let para = "Sentence number ".repeat(200); // ~3200 chars, one paragraph
        let text = format!("{para}\n\n{para}");
        let chunks = chunk(&text);
        assert!(chunks.len() >= 3, "expected multiple chunks, got {}", chunks.len());
        for c in &chunks {
            assert!(c.text.len() <= MAX + 50, "chunk too large: {}", c.text.len());
            assert!(!c.text.is_empty());
        }
        // Indices are contiguous.
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.index, i);
        }
    }

    #[test]
    fn unicode_is_not_split_mid_char() {
        let text = "café ".repeat(500);
        let chunks = chunk(&text);
        for c in &chunks {
            assert!(std::str::from_utf8(c.text.as_bytes()).is_ok());
        }
    }
}
