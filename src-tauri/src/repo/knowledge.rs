use crate::models::KbChunkHit;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

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


/// 中文停用词（混合检索关键词提取）
const STOP_WORDS: &[&str] = &[
    "的", "了", "吗", "呢", "是", "我", "你", "他", "她", "它", "这", "那", "有", "在", "和",
    "与", "就", "都", "也", "很", "着", "过", "把", "被", "让", "给", "从", "到", "对", "为",
    "以", "而", "或", "但", "如果", "因为", "所以", "什么", "怎么", "怎样", "如何",
    "一个", "没有", "可以", "不是", "这个", "那个", "我们", "你们", "他们", "它们", "啊", "吧",
];

/// 文件夹路径：根→叶用 ` / ` 拼接；未分类 / 缺失 →「未分类」
pub fn folder_path(conn: &Connection, folder_id: Option<i64>) -> String {
    let Some(mut id) = folder_id else {
        return "未分类".into();
    };
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(id) {
            break;
        }
        match conn.query_row(
            "SELECT parent_id, name FROM note_folders WHERE id = ?1",
            params![id],
            |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, String>(1)?)),
        ) {
            Ok((parent, name)) => {
                names.push(name);
                match parent {
                    Some(p) => id = p,
                    None => break,
                }
            }
            Err(_) => {
                names.clear();
                break;
            }
        }
    }
    if names.is_empty() {
        return "未分类".into();
    }
    names.reverse();
    names.join(" / ")
}

/// 关键词提取：去标点/停用词；中文 2-gram + 整词；英文按空白分词。
pub fn extract_keywords(query: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let flush_buf = |buf: &mut String, tokens: &mut Vec<String>| {
        if buf.is_empty() {
            return;
        }
        let word = std::mem::take(buf);
        let lower = word.to_lowercase();
        if is_stop_word(&lower) {
            return;
        }
        // 英文/数字词
        if word.chars().all(|c| c.is_ascii_alphanumeric()) {
            if word.chars().count() >= 2 {
                tokens.push(lower);
            }
            return;
        }
        // 中文：整词（≥2 字）+ 2-gram
        let chars: Vec<char> = word.chars().collect();
        if chars.len() >= 2 {
            tokens.push(word.clone());
        }
        if chars.len() >= 2 {
            for i in 0..chars.len().saturating_sub(1) {
                let gram: String = chars[i..i + 2].iter().collect();
                if !is_stop_word(&gram) {
                    tokens.push(gram);
                }
            }
        }
    };

    for ch in query.chars() {
        if ch.is_whitespace() || is_punct(ch) {
            flush_buf(&mut buf, &mut tokens);
        } else {
            // 中英切换时切开
            let prev_ascii = buf
                .chars()
                .last()
                .map(|c| c.is_ascii_alphanumeric())
                .unwrap_or(false);
            let cur_ascii = ch.is_ascii_alphanumeric();
            if !buf.is_empty() && prev_ascii != cur_ascii {
                flush_buf(&mut buf, &mut tokens);
            }
            buf.push(ch);
        }
    }
    flush_buf(&mut buf, &mut tokens);

    // 去重保序
    let mut seen = HashSet::new();
    tokens
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation()
        || matches!(
            c,
            '，' | '。'
                | '！'
                | '？'
                | '；'
                | '：'
                | '、'
                | '（'
                | '）'
                | '【'
                | '】'
                | '《'
                | '》'
                | '\u{201c}'
                | '\u{201d}'
                | '\u{2018}'
                | '\u{2019}'
                | '·'
                | '…'
        )
}

fn is_stop_word(w: &str) -> bool {
    STOP_WORDS.contains(&w)
}

fn count_keyword_hits(haystack: &str, keywords: &[String]) -> i64 {
    if keywords.is_empty() || haystack.is_empty() {
        return 0;
    }
    let lower = haystack.to_lowercase();
    keywords
        .iter()
        .filter(|k| {
            if k.chars().all(|c| c.is_ascii()) {
                lower.contains(&k.to_lowercase())
            } else {
                haystack.contains(k.as_str())
            }
        })
        .count() as i64
}

struct ChunkRow {
    chunk_id: i64,
    note_id: i64,
    note_title: String,
    folder_id: Option<i64>,
    heading: String,
    content: String,
    embedding: Option<Vec<u8>>,
}

fn load_chunks_with_notes(conn: &Connection, only_embedded: bool) -> rusqlite::Result<Vec<ChunkRow>> {
    let sql = if only_embedded {
        "SELECT c.id, c.note_id, n.title, n.folder_id, c.heading, c.content, c.embedding
         FROM kb_chunks c
         JOIN notes n ON n.id = c.note_id
         WHERE c.embedding IS NOT NULL"
    } else {
        "SELECT c.id, c.note_id, n.title, n.folder_id, c.heading, c.content, c.embedding
         FROM kb_chunks c
         JOIN notes n ON n.id = c.note_id"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(ChunkRow {
            chunk_id: r.get(0)?,
            note_id: r.get(1)?,
            note_title: r.get(2)?,
            folder_id: r.get(3)?,
            heading: r.get(4)?,
            content: r.get(5)?,
            embedding: r.get(6)?,
        })
    })?;
    rows.collect()
}

fn path_cache_get(
    cache: &mut HashMap<Option<i64>, String>,
    conn: &Connection,
    folder_id: Option<i64>,
) -> String {
    if let Some(p) = cache.get(&folder_id) {
        return p.clone();
    }
    let p = folder_path(conn, folder_id);
    cache.insert(folder_id, p.clone());
    p
}

fn to_hit(
    row: &ChunkRow,
    folder_path: String,
    score: f64,
    vector_score: f64,
    keyword_hits: i64,
) -> KbChunkHit {
    KbChunkHit {
        chunk_id: row.chunk_id,
        note_id: row.note_id,
        note_title: row.note_title.clone(),
        folder_path,
        heading: row.heading.clone(),
        content: row.content.clone(),
        score,
        vector_score,
        keyword_hits,
    }
}

/// 混合检索：向量余弦 + 关键词 2-gram 加分；向量不足时 LIKE 降级补足。
pub fn hybrid_search(
    conn: &Connection,
    query_vec: &[f32],
    query_text: &str,
    top_k: i64,
) -> Result<Vec<KbChunkHit>, String> {
    let top_k = top_k.clamp(3, 10) as usize;
    let keywords = extract_keywords(query_text);
    let mut path_cache: HashMap<Option<i64>, String> = HashMap::new();

    let embedded = load_chunks_with_notes(conn, true).map_err(|e| e.to_string())?;

    // 1) 向量检索：仅按 vector_score 排序，取 top_k*3 候选（§5.6.2）
    let candidate_n = top_k.saturating_mul(3).max(top_k);
    let mut scored: Vec<(f64, f64, i64, ChunkRow)> = Vec::new();

    if !query_vec.is_empty() {
        let mut by_vector: Vec<(f64, ChunkRow)> = Vec::new();
        for row in embedded {
            let Some(blob) = row.embedding.as_deref() else {
                continue;
            };
            let Ok(vec) = blob_to_embedding(blob) else {
                continue;
            };
            if vec.is_empty() {
                continue;
            }
            let vector_score = cosine(query_vec, &vec);
            by_vector.push((vector_score, row));
        }
        by_vector.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        if by_vector.len() > candidate_n {
            by_vector.truncate(candidate_n);
        }

        // 2) 对候选施加关键词加分，再按混合分排序
        for (vector_score, row) in by_vector {
            let hay = format!("{} {} {}", row.note_title, row.heading, row.content);
            let keyword_hits = count_keyword_hits(&hay, &keywords);
            let score = vector_score * 0.7 + (keyword_hits.min(10) as f64) * 0.05;
            scored.push((score, vector_score, keyword_hits, row));
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    }

    // 3) 按混合分取 top_k
    let mut results: Vec<KbChunkHit> = Vec::new();
    let mut seen_ids: HashSet<i64> = HashSet::new();
    for (score, vector_score, keyword_hits, row) in scored.into_iter().take(top_k) {
        seen_ids.insert(row.chunk_id);
        let fp = path_cache_get(&mut path_cache, conn, row.folder_id);
        results.push(to_hit(&row, fp, score, vector_score, keyword_hits));
    }

    // 4) 向量未就绪 / 命中不足：关键词 LIKE 补足
    if results.len() < top_k {
        let all = load_chunks_with_notes(conn, false).map_err(|e| e.to_string())?;
        let mut like_hits: Vec<(i64, ChunkRow)> = Vec::new();
        for row in all {
            if seen_ids.contains(&row.chunk_id) {
                continue;
            }
            let hay = format!("{} {} {}", row.note_title, row.heading, row.content);
            let hits = count_keyword_hits(&hay, &keywords);
            if hits > 0 {
                like_hits.push((hits, row));
            } else if keywords.is_empty() {
                // 无关键词时用原始 query 子串兜底
                let q = query_text.trim();
                if !q.is_empty() && hay.contains(q) {
                    like_hits.push((1, row));
                }
            }
        }
        like_hits.sort_by(|a, b| b.0.cmp(&a.0));
        for (hits, row) in like_hits {
            if results.len() >= top_k {
                break;
            }
            seen_ids.insert(row.chunk_id);
            let fp = path_cache_get(&mut path_cache, conn, row.folder_id);
            let score = (hits.min(10) as f64) * 0.05;
            results.push(to_hit(&row, fp, score, 0.0, hits));
        }
    }

    Ok(results)
}

// ---------- kb_chunks / kb_meta CRUD ----------

pub fn clear_all_chunks(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM kb_chunks", [])?;
    Ok(())
}

pub fn delete_chunks_for_note(conn: &Connection, note_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM kb_chunks WHERE note_id = ?1", params![note_id])?;
    Ok(())
}

pub fn insert_chunk(
    conn: &Connection,
    note_id: i64,
    idx: i64,
    heading: &str,
    content: &str,
    dim: i64,
    model: &str,
    embedding: Option<&[u8]>,
    embed_error: Option<&str>,
) -> rusqlite::Result<()> {
    let token_count = content.chars().count() as i64;
    conn.execute(
        "INSERT INTO kb_chunks (
            note_id, chunk_index, heading, content, token_count,
            dim, model, embedding, embed_error
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            note_id,
            idx,
            heading,
            content,
            token_count,
            dim,
            model,
            embedding,
            embed_error
        ],
    )?;
    Ok(())
}

/// 事务内替换某笔记的全部片段。
pub fn replace_note_chunks(
    conn: &Connection,
    note_id: i64,
    rows: &[(
        i64,          // chunk_index
        &str,         // heading
        &str,         // content
        i64,          // dim
        &str,         // model
        Option<&[u8]>, // embedding
        Option<&str>, // embed_error
    )],
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    delete_chunks_for_note(&tx, note_id)?;
    for (idx, heading, content, dim, model, embedding, embed_error) in rows {
        insert_chunk(
            &tx,
            note_id,
            *idx,
            heading,
            content,
            *dim,
            model,
            *embedding,
            *embed_error,
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// (status, progress, model, indexed_notes, chunk_count, last_indexed_at, error)
pub fn get_meta(
    conn: &Connection,
) -> rusqlite::Result<(String, i64, String, i64, i64, Option<String>, Option<String>)> {
    conn.query_row(
        "SELECT status, progress, model, indexed_notes, chunk_count, last_indexed_at, error
         FROM kb_meta WHERE id = 1",
        [],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        },
    )
}

pub fn set_meta_indexing(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE kb_meta SET status = 'indexing', progress = 0, error = NULL WHERE id = 1",
        [],
    )?;
    Ok(())
}

pub fn set_meta_done(
    conn: &Connection,
    model: &str,
    indexed_notes: i64,
    chunk_count: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE kb_meta SET
            status = 'done',
            progress = 100,
            error = NULL,
            model = ?1,
            indexed_notes = ?2,
            chunk_count = ?3,
            last_indexed_at = strftime('%Y-%m-%d %H:%M:%f','now')
         WHERE id = 1",
        params![model, indexed_notes, chunk_count],
    )?;
    Ok(())
}

pub fn set_meta_error(conn: &Connection, err: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE kb_meta SET status = 'error', error = ?1 WHERE id = 1",
        params![err],
    )?;
    Ok(())
}

pub fn set_meta_progress(conn: &Connection, progress: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE kb_meta SET progress = ?1 WHERE id = 1",
        params![progress],
    )?;
    Ok(())
}

/// 若仍停在 `indexing`，落成 `error`（重建中途 `?` 失败 / Drop 兜底用）。
/// 返回是否实际写入了 error。
pub fn clear_stuck_indexing(conn: &Connection, err: &str) -> rusqlite::Result<bool> {
    let (status, _, _, _, _, _, _) = get_meta(conn)?;
    if status == "indexing" {
        set_meta_error(conn, err)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// 全量重建收尾：有片段但无一成功嵌入 → error；否则 done。
/// `Err(String)` 表示应向上返回的业务错误（meta 已写好）。
pub fn finalize_rebuild_meta(conn: &Connection, model: &str) -> Result<(), String> {
    let indexed = count_indexed_notes(conn).map_err(|e| e.to_string())?;
    let chunks = count_chunks(conn).map_err(|e| e.to_string())?;
    if chunks > 0 && indexed == 0 {
        let msg = "嵌入全部失败，请检查 Ollama / 嵌入配置";
        set_meta_error(conn, msg).map_err(|e| e.to_string())?;
        let _ = conn.execute(
            "UPDATE kb_meta SET indexed_notes = ?1, chunk_count = ?2, model = ?3 WHERE id = 1",
            params![indexed, chunks, model],
        );
        return Err(msg.into());
    }
    set_meta_done(conn, model, indexed, chunks).map_err(|e| e.to_string())?;
    Ok(())
}

/// 增量索引后刷新计数；若当前非 indexing 则置为 done。
pub fn refresh_meta_counts(conn: &Connection, model: &str) -> rusqlite::Result<()> {
    let indexed = count_indexed_notes(conn)?;
    let chunks = count_chunks(conn)?;
    let (status, _, _, _, _, _, _) = get_meta(conn)?;
    if status == "indexing" {
        conn.execute(
            "UPDATE kb_meta SET indexed_notes = ?1, chunk_count = ?2 WHERE id = 1",
            params![indexed, chunks],
        )?;
    } else {
        set_meta_done(conn, model, indexed, chunks)?;
    }
    Ok(())
}

pub fn count_chunks(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM kb_chunks", [], |r| r.get(0))
}

pub fn count_indexed_notes(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(DISTINCT note_id) FROM kb_chunks WHERE embedding IS NOT NULL",
        [],
        |r| r.get(0),
    )
}

pub fn count_notes(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))
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

    #[test]
    fn replace_chunks_roundtrip_without_embed() {
        let conn = crate::db::init_in_memory().unwrap();
        let n = crate::repo::note::create(&conn, "t").unwrap();
        delete_chunks_for_note(&conn, n.id).unwrap();
        insert_chunk(
            &conn,
            n.id,
            0,
            "H",
            "body",
            0,
            "m",
            None,
            Some("no embed"),
        )
        .unwrap();
        assert_eq!(count_chunks(&conn).unwrap(), 1);

        replace_note_chunks(
            &conn,
            n.id,
            &[(1, "H2", "body2", 0, "m", None, Some("still no"))],
        )
        .unwrap();
        assert_eq!(count_chunks(&conn).unwrap(), 1);
        let idx: i64 = conn
            .query_row(
                "SELECT chunk_index FROM kb_chunks WHERE note_id = ?1",
                params![n.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1);
    }

    #[test]
    fn clear_stuck_indexing_only_when_indexing() {
        let conn = crate::db::init_in_memory().unwrap();
        set_meta_indexing(&conn).unwrap();
        assert!(clear_stuck_indexing(&conn, "索引失败（任务中断）").unwrap());
        let (status, _, _, _, _, _, err) = get_meta(&conn).unwrap();
        assert_eq!(status, "error");
        assert_eq!(err.as_deref(), Some("索引失败（任务中断）"));

        // 已是 error 时不再覆盖
        assert!(!clear_stuck_indexing(&conn, "另一条").unwrap());
        let (_, _, _, _, _, _, err2) = get_meta(&conn).unwrap();
        assert_eq!(err2.as_deref(), Some("索引失败（任务中断）"));
    }

    #[test]
    fn finalize_rebuild_all_embed_failed_sets_error() {
        let conn = crate::db::init_in_memory().unwrap();
        let n = crate::repo::note::create(&conn, "t").unwrap();
        insert_chunk(&conn, n.id, 0, "", "body", 0, "m", None, Some("boom")).unwrap();
        let err = finalize_rebuild_meta(&conn, "m").unwrap_err();
        assert!(err.contains("嵌入全部失败"));
        let (status, _, _, _, _, _, e) = get_meta(&conn).unwrap();
        assert_eq!(status, "error");
        assert_eq!(e.as_deref(), Some("嵌入全部失败，请检查 Ollama / 嵌入配置"));
    }

    #[test]
    fn finalize_rebuild_empty_kb_is_done() {
        let conn = crate::db::init_in_memory().unwrap();
        finalize_rebuild_meta(&conn, "m").unwrap();
        let (status, progress, _, indexed, chunks, _, err) = get_meta(&conn).unwrap();
        assert_eq!(status, "done");
        assert_eq!(progress, 100);
        assert_eq!(indexed, 0);
        assert_eq!(chunks, 0);
        assert!(err.is_none());
    }

    #[test]
    fn live_counts_survive_stale_meta_after_note_delete() {
        let conn = crate::db::init_in_memory().unwrap();
        let n = crate::repo::note::create(&conn, "t").unwrap();
        let blob = embedding_to_blob(&[1.0f32, 0.0]);
        insert_chunk(&conn, n.id, 0, "", "body", 2, "m", Some(&blob), None).unwrap();
        set_meta_done(&conn, "m", 1, 1).unwrap();

        crate::repo::note::delete(&conn, n.id).unwrap();
        // CASCADE 清了 chunks，但 meta 列仍是旧值
        let (_, _, _, meta_indexed, meta_chunks, _, _) = get_meta(&conn).unwrap();
        assert_eq!(meta_indexed, 1);
        assert_eq!(meta_chunks, 1);
        assert_eq!(count_chunks(&conn).unwrap(), 0);
        assert_eq!(count_indexed_notes(&conn).unwrap(), 0);
        assert_eq!(count_notes(&conn).unwrap(), 0);
    }

    #[test]
    fn extract_keywords_chinese_bigrams() {
        let kws = extract_keywords("架构设计");
        assert!(kws.iter().any(|k| k == "架构"));
        assert!(kws.iter().any(|k| k == "构设"));
        assert!(kws.iter().any(|k| k == "设计"));
        assert!(kws.iter().any(|k| k == "架构设计"));
    }

    #[test]
    fn extract_keywords_drops_stop_words() {
        let kws = extract_keywords("这是什么架构");
        assert!(!kws.iter().any(|k| *k == "这" || *k == "是" || *k == "什么"));
        assert!(kws.iter().any(|k| k.contains("架构")));
    }

    #[test]
    fn folder_path_uncategorized_and_nested() {
        let conn = crate::db::init_in_memory().unwrap();
        assert_eq!(folder_path(&conn, None), "未分类");
        let root = crate::repo::folder::create(&conn, None, "工作", None).unwrap();
        let child = crate::repo::folder::create(&conn, Some(root.id), "项目A", None).unwrap();
        assert_eq!(folder_path(&conn, Some(child.id)), "工作 / 项目A");
    }

    #[test]
    fn hybrid_search_keyword_fallback_without_vectors() {
        let conn = crate::db::init_in_memory().unwrap();
        let n = crate::repo::note::create(&conn, "架构笔记").unwrap();
        insert_chunk(
            &conn,
            n.id,
            0,
            "设计",
            "这里讨论系统架构设计原则",
            0,
            "m",
            None,
            Some("no embed"),
        )
        .unwrap();
        let hits = hybrid_search(&conn, &[], "架构设计", 3).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].note_id, n.id);
        assert!(hits[0].keyword_hits > 0);
        assert_eq!(hits[0].folder_path, "未分类");
    }

    #[test]
    fn hybrid_search_vector_ranks_similar_higher() {
        let conn = crate::db::init_in_memory().unwrap();
        let n1 = crate::repo::note::create(&conn, "向量相关").unwrap();
        let n2 = crate::repo::note::create(&conn, "无关").unwrap();
        let close = embedding_to_blob(&[1.0f32, 0.0, 0.0]);
        let far = embedding_to_blob(&[0.0f32, 1.0, 0.0]);
        insert_chunk(&conn, n1.id, 0, "", "close", 3, "m", Some(&close), None).unwrap();
        insert_chunk(&conn, n2.id, 0, "", "far", 3, "m", Some(&far), None).unwrap();
        let hits = hybrid_search(&conn, &[1.0, 0.0, 0.0], "xyz", 3).unwrap();
        assert!(hits.len() >= 2);
        assert_eq!(hits[0].note_id, n1.id);
        assert!(hits[0].vector_score > hits[1].vector_score);
    }

    #[test]
    fn hybrid_search_clamps_top_k() {
        let conn = crate::db::init_in_memory().unwrap();
        for i in 0..12 {
            let n = crate::repo::note::create(&conn, &format!("n{i}")).unwrap();
            insert_chunk(
                &conn,
                n.id,
                0,
                "",
                &format!("内容架构{i}"),
                0,
                "m",
                None,
                Some("e"),
            )
            .unwrap();
        }
        let hits = hybrid_search(&conn, &[], "架构", 100).unwrap();
        assert!(hits.len() <= 10);
        let hits2 = hybrid_search(&conn, &[], "架构", 1).unwrap();
        assert!(hits2.len() <= 3);
    }

    /// §5.6.2：候选池先按 vector_score 截断，再关键词加分。
    /// top_k=3 → 候选 9；第 10 名高关键词低向量不得挤掉第 9 名高向量低关键词。
    #[test]
    fn hybrid_search_candidate_cut_is_vector_only() {
        let conn = crate::db::init_in_memory().unwrap();
        let query = [1.0f32, 0.0, 0.0];
        // 9 个高向量、正文无关键词（query 用「架构设计」）
        for i in 0..9 {
            let n = crate::repo::note::create(&conn, &format!("hv{i}")).unwrap();
            // 余弦随 i 略降，但仍远高于低向量块
            let v = embedding_to_blob(&[1.0 - (i as f32) * 0.01, 0.0, 0.0]);
            insert_chunk(
                &conn,
                n.id,
                0,
                "",
                &format!("semantic body {i}"),
                3,
                "m",
                Some(&v),
                None,
            )
            .unwrap();
        }
        // 第 10 名：低向量但关键词极强
        let n_kw = crate::repo::note::create(&conn, "架构设计笔记").unwrap();
        let far = embedding_to_blob(&[0.0f32, 1.0, 0.0]);
        insert_chunk(
            &conn,
            n_kw.id,
            0,
            "架构",
            "架构设计 架构设计 架构设计 架构设计",
            3,
            "m",
            Some(&far),
            None,
        )
        .unwrap();

        let hits = hybrid_search(&conn, &query, "架构设计", 3).unwrap();
        assert_eq!(hits.len(), 3);
        // 高向量块应进最终 top_3；低向量高关键词块不得因关键词挤进候选后上位
        assert!(
            hits.iter().all(|h| h.note_id != n_kw.id),
            "low-vector high-keyword chunk must stay outside vector candidate cut; got: {:?}",
            hits.iter().map(|h| (h.note_id, h.vector_score, h.keyword_hits)).collect::<Vec<_>>()
        );
        assert!(hits.iter().all(|h| h.vector_score > 0.9));
    }

}
