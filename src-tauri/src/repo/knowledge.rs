const TARGET_CHUNK_LEN: usize = 500;
const MAX_CHUNK_LEN: usize = 1000;
const MAX_HEADING_LEVELS: usize = 4;

pub struct ChunkText {
    pub heading: String,
    pub content: String,
}

struct RawSegment {
    heading: String,
    text: String,
    is_code: bool,
}

pub fn chunk_markdown(md: &str) -> Vec<ChunkText> {
    if md.trim().is_empty() {
        return Vec::new();
    }

    let segments = parse_segments(md);
    let mut chunks = Vec::new();
    let mut buf = String::new();
    let mut buf_heading = String::new();
    let mut buf_is_code = false;

    for seg in segments {
        let seg_text = if seg.is_code {
            seg.text.clone()
        } else {
            strip_markdown(&seg.text)
        };
        if seg_text.trim().is_empty() {
            continue;
        }

        if seg.is_code {
            flush_buffer(&mut buf, &mut buf_heading, buf_is_code, &mut chunks);
            for piece in split_long_text(&seg_text, MAX_CHUNK_LEN) {
                if !piece.trim().is_empty() {
                    chunks.push(ChunkText {
                        heading: seg.heading.clone(),
                        content: piece,
                    });
                }
            }
            continue;
        }

        for piece in split_long_text(&seg_text, MAX_CHUNK_LEN) {
            if piece.trim().is_empty() {
                continue;
            }
            let piece_len = piece.chars().count();
            let buf_len = buf.chars().count();
            let same_heading = buf.is_empty() || buf_heading == seg.heading;

            if !buf.is_empty()
                && (!same_heading || buf_len + 1 + piece_len > MAX_CHUNK_LEN
                    || buf_len >= TARGET_CHUNK_LEN)
            {
                flush_buffer(&mut buf, &mut buf_heading, buf_is_code, &mut chunks);
            }

            if buf.is_empty() {
                buf_heading = seg.heading.clone();
                buf_is_code = false;
            }

            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(&piece);
        }
    }

    flush_buffer(&mut buf, &mut buf_heading, buf_is_code, &mut chunks);
    chunks
}

fn flush_buffer(buf: &mut String, heading: &mut String, _is_code: bool, chunks: &mut Vec<ChunkText>) {
    let text = buf.trim();
    if text.is_empty() {
        buf.clear();
        return;
    }
    chunks.push(ChunkText {
        heading: std::mem::take(heading),
        content: text.to_string(),
    });
    buf.clear();
}

fn parse_segments(md: &str) -> Vec<RawSegment> {
    let mut headings: Vec<String> = Vec::new();
    let mut segments = Vec::new();
    let mut para_lines: Vec<String> = Vec::new();
    let mut in_code = false;
    let mut code_lines: Vec<String> = Vec::new();

    let flush_paragraph = |headings: &[String], lines: &mut Vec<String>, segments: &mut Vec<RawSegment>| {
        if lines.is_empty() {
            return;
        }
        let text = lines.join("\n");
        lines.clear();
        if text.trim().is_empty() {
            return;
        }
        segments.push(RawSegment {
            heading: heading_chain(headings),
            text,
            is_code: false,
        });
    };

    for line in md.lines() {
        if in_code {
            if line.trim_start().starts_with("```") {
                code_lines.push(line.to_string());
                let text = code_lines.join("\n");
                code_lines.clear();
                in_code = false;
                if !text.trim().is_empty() {
                    segments.push(RawSegment {
                        heading: heading_chain(&headings),
                        text,
                        is_code: true,
                    });
                }
            } else {
                code_lines.push(line.to_string());
            }
            continue;
        }

        if line.trim_start().starts_with("```") {
            flush_paragraph(&headings, &mut para_lines, &mut segments);
            in_code = true;
            code_lines.push(line.to_string());
            continue;
        }

        if let Some((level, title)) = parse_heading(line) {
            flush_paragraph(&headings, &mut para_lines, &mut segments);
            update_headings(&mut headings, level, title);
            continue;
        }

        if line.trim().is_empty() {
            flush_paragraph(&headings, &mut para_lines, &mut segments);
        } else {
            para_lines.push(line.to_string());
        }
    }

    if in_code {
        let text = code_lines.join("\n");
        if !text.trim().is_empty() {
            segments.push(RawSegment {
                heading: heading_chain(&headings),
                text,
                is_code: true,
            });
        }
    } else {
        flush_paragraph(&headings, &mut para_lines, &mut segments);
    }

    segments
}

fn parse_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = trimmed[hashes..].trim_start();
    if rest.is_empty() || !rest.starts_with(|c: char| c != '#') {
        return None;
    }
    let title = rest.trim_end_matches('#').trim().to_string();
    if title.is_empty() {
        return None;
    }
    Some((hashes, title))
}

fn update_headings(headings: &mut Vec<String>, level: usize, title: String) {
    if level <= 1 {
        headings.clear();
    } else {
        headings.truncate(level - 1);
    }
    headings.push(title);
    if headings.len() > MAX_HEADING_LEVELS {
        let drop = headings.len() - MAX_HEADING_LEVELS;
        headings.drain(0..drop);
    }
}

fn heading_chain(headings: &[String]) -> String {
    let start = headings.len().saturating_sub(MAX_HEADING_LEVELS);
    headings[start..].join(" / ")
}

fn split_long_text(text: &str, max_len: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let mut end = (start + max_len).min(chars.len());
        if end < chars.len() {
            let window_start = end.saturating_sub(80).max(start + 1);
            if let Some(rel) = chars[window_start..end]
                .iter()
                .rposition(|c| c.is_whitespace() || *c == '，' || *c == '。' || *c == '；')
            {
                end = window_start + rel + 1;
            }
        }
        if end <= start {
            end = (start + max_len).min(chars.len());
        }
        out.push(chars[start..end].iter().collect());
        start = end;
    }
    out
}

fn strip_markdown(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&strip_markdown_line(line));
    }
    out
}

fn strip_markdown_line(line: &str) -> String {
    let mut s = line.trim().to_string();
    if let Some(rest) = s.strip_prefix('>') {
        s = rest.trim_start().to_string();
    }
    if let Some(rest) = s.strip_prefix("- ") {
        s = rest.to_string();
    } else if let Some(rest) = s.strip_prefix("* ") {
        s = rest.to_string();
    } else if let Some(rest) = s.strip_prefix("+ ") {
        s = rest.to_string();
    } else if let Some(pos) = s.find(". ") {
        if pos <= 3 && s[..pos].chars().all(|c| c.is_ascii_digit()) {
            s = s[pos + 2..].to_string();
        }
    }

    s = strip_inline_links(&s);
    s = strip_emphasis(&s);
    s = s.replace('`', "");
    s.trim().to_string()
}

fn strip_inline_links(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            if let Some((alt, next)) = parse_link_text(&s[i + 2..]) {
                out.push_str(&alt);
                i += 2 + next;
                continue;
            }
        }
        if bytes[i] == b'[' {
            if let Some((label, next)) = parse_link_text(&s[i + 1..]) {
                out.push_str(&label);
                i += 1 + next;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn parse_link_text(s: &str) -> Option<(String, usize)> {
    let mut depth = 0usize;
    for (idx, ch) in s.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' if depth == 0 => {
                let label = s[..idx].to_string();
                let rest = &s[idx + 1..];
                if rest.starts_with('(') {
                    if let Some(close) = rest.find(')') {
                        return Some((label, idx + 1 + close + 1));
                    }
                }
                return Some((label, idx + 1));
            }
            ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

fn strip_emphasis(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'*' {
            if let Some(end) = s[i + 2..].find("**") {
                out.push_str(&s[i + 2..i + 2 + end]);
                i += 4 + end;
                continue;
            }
        }
        if i + 1 < bytes.len() && bytes[i] == b'~' && bytes[i + 1] == b'~' {
            if let Some(end) = s[i + 2..].find("~~") {
                out.push_str(&s[i + 2..i + 2 + end]);
                i += 4 + end;
                continue;
            }
        }
        if bytes[i] == b'*' || bytes[i] == b'_' {
            if let Some(end) = s[i + 1..].find(bytes[i] as char) {
                out.push_str(&s[i + 1..i + 1 + end]);
                i += 2 + end;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

pub fn embedding_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

pub fn blob_to_embedding(b: &[u8]) -> Result<Vec<f32>, String> {
    if b.len() % 4 != 0 {
        return Err("embedding BLOB 长度非法".into());
    }
    let mut out = Vec::with_capacity(b.len() / 4);
    for chunk in b.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}

pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    let mut dot = 0f64;
    let mut na = 0f64;
    let mut nb = 0f64;
    for i in 0..n {
        let x = a[i] as f64;
        let y = b[i] as f64;
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_markdown_builds_heading_chain() {
        let md = "# A\n\npara1\n\n## B\n\npara2\n";
        let chunks = chunk_markdown(md);
        assert!(chunks.iter().any(|c| c.heading.contains("A") && c.content.contains("para1")));
        assert!(chunks.iter().any(|c| c.heading.contains("B") && c.content.contains("para2")));
    }

    #[test]
    fn chunk_markdown_skips_empty() {
        assert!(chunk_markdown("").is_empty());
        assert!(chunk_markdown("   \n\n").is_empty());
    }

    #[test]
    fn embedding_blob_roundtrip() {
        let v = vec![1.0f32, -2.5, 0.0];
        let b = embedding_to_blob(&v);
        let back = blob_to_embedding(&b).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn cosine_identical_is_one() {
        let a = vec![1.0f32, 0.0, 0.0];
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn chunk_markdown_keeps_code_block_intact() {
        let md = "# Title\n\n```rust\nfn main() {}\n```\n";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("```"));
        assert!(chunks[0].content.contains("fn main()"));
    }

    #[test]
    fn chunk_markdown_strips_inline_markdown() {
        let md = "Hello **bold** and [link](http://x.com)\n";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("bold"));
        assert!(!chunks[0].content.contains("**"));
        assert!(chunks[0].content.contains("link"));
        assert!(!chunks[0].content.contains("http://"));
    }

    #[test]
    fn chunk_markdown_splits_long_paragraph() {
        let para: String = "字".repeat(1200);
        let md = format!("# H\n\n{para}\n");
        let chunks = chunk_markdown(&md);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|c| c.content.chars().count() <= MAX_CHUNK_LEN));
    }

    #[test]
    fn chunk_markdown_caps_heading_chain_at_four() {
        let md = "# L1\n## L2\n### L3\n#### L4\n##### L5\n\nbody\n";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks.len(), 1);
        let parts: Vec<_> = chunks[0].heading.split(" / ").collect();
        assert_eq!(parts.len(), 4);
        assert!(chunks[0].heading.contains("L5"));
        assert!(!chunks[0].heading.contains("L1"));
    }

    #[test]
    fn blob_to_embedding_rejects_bad_length() {
        assert!(blob_to_embedding(&[0u8, 1, 2]).is_err());
    }
}
