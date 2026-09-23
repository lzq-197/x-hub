//! Markdown 导入 + 索引 stub（本段无 kb_chunks / 嵌入）。

use crate::models::ImportResult;
use crate::repo::{folder, note};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_IMPORT_FILES: usize = 2000;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_WALK_DEPTH: usize = 8;
const MAX_ERRORS: usize = 20;

/// 索引 stub：立即成功，不写库、不访问网络。
pub fn index_note_stub(_note_id: i64) -> Result<(), String> {
    Ok(())
}

/// 递归导入目录（或单文件）。部分失败不整单回滚。
pub fn import_markdown(conn: &Connection, path: &str) -> Result<ImportResult, String> {
    let root = PathBuf::from(path);
    if !root.exists() {
        return Err(format!("路径不存在: {path}"));
    }

    let (scan_root, candidates) = collect_candidates(&root)?;
    let mut errors: Vec<String> = Vec::new();
    let mut paths = candidates;

    if paths.len() > MAX_IMPORT_FILES {
        paths.truncate(MAX_IMPORT_FILES);
        push_error(
            &mut errors,
            "单次导入超过 2000 个文件，已截断".to_string(),
        );
    }

    let mut imported: i64 = 0;
    let mut updated: i64 = 0;
    let mut skipped: i64 = 0;
    let mut failed: i64 = 0;
    let mut changed_ids: Vec<i64> = Vec::new();

    for file in &paths {
        match import_one(conn, &scan_root, file, &mut errors) {
            Ok(ImportOne::Imported(id)) => {
                imported += 1;
                changed_ids.push(id);
            }
            Ok(ImportOne::Updated(id)) => {
                updated += 1;
                changed_ids.push(id);
            }
            Ok(ImportOne::Skipped) => skipped += 1,
            Ok(ImportOne::Failed) => failed += 1,
            Err(e) => {
                failed += 1;
                push_error(&mut errors, e);
            }
        }
    }

    crate::kb_hooks::on_notes_changed(&changed_ids);

    Ok(ImportResult {
        imported,
        updated,
        skipped,
        failed,
        total: paths.len() as i64,
        errors,
    })
}

enum ImportOne {
    Imported(i64),
    Updated(i64),
    Skipped,
    Failed,
}

fn import_one(
    conn: &Connection,
    scan_root: &Path,
    file: &Path,
    errors: &mut Vec<String>,
) -> Result<ImportOne, String> {
    let meta = fs::metadata(file).map_err(|e| format!("{}: {}", display_path(file), e))?;
    if meta.len() > MAX_FILE_BYTES {
        push_error(
            errors,
            format!("{}: 单文件超过 2MB", display_path(file)),
        );
        return Ok(ImportOne::Failed);
    }

    let bytes = fs::read(file).map_err(|e| format!("{}: {}", display_path(file), e))?;
    let content = String::from_utf8_lossy(&bytes).into_owned();

    let rel = file
        .strip_prefix(scan_root)
        .unwrap_or(file)
        .to_path_buf();
    let source_path = normalize_source_path(&rel);
    if source_path.is_empty() {
        return Ok(ImportOne::Skipped);
    }

    let title = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "无标题笔记".into());

    let folder_id = resolve_folder_id(conn, &rel)?;

    match note::find_by_source_path(conn, &source_path).map_err(|e| e.to_string())? {
        Some(existing) => {
            let n = note::update_imported(conn, existing.id, &title, &content, folder_id)
                .map_err(|e| e.to_string())?;
            Ok(ImportOne::Updated(n.id))
        }
        None => {
            let n = note::create_imported(conn, &title, &content, folder_id, &source_path)
                .map_err(|e| e.to_string())?;
            Ok(ImportOne::Imported(n.id))
        }
    }
}

fn resolve_folder_id(conn: &Connection, rel: &Path) -> Result<Option<i64>, String> {
    let parent = match rel.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return Ok(None),
    };
    let segments: Vec<String> = parent
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => {
                let name = s.to_string_lossy();
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            _ => None,
        })
        .collect();
    if segments.is_empty() {
        return Ok(None);
    }
    let refs: Vec<&str> = segments.iter().map(|s| s.as_str()).collect();
    folder::find_or_create_path(conn, None, &refs)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// 返回 (扫描根目录, 候选文件列表)。单文件时扫描根为其父目录。
fn collect_candidates(root: &Path) -> Result<(PathBuf, Vec<PathBuf>), String> {
    if root.is_file() {
        if !is_markdown(root) {
            return Err("仅支持导入 .md / .markdown 文件".into());
        }
        let parent = root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        return Ok((parent, vec![root.to_path_buf()]));
    }
    if !root.is_dir() {
        return Err(format!("无法识别的路径: {}", root.display()));
    }
    let mut out = Vec::new();
    walk_dir(root, 0, &mut out)?;
    Ok((root.to_path_buf(), out))
}

fn walk_dir(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {}", dir.display(), e))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let ft = entry
            .file_type()
            .map_err(|e| format!("{}: {}", path.display(), e))?;
        if ft.is_dir() {
            if depth < MAX_WALK_DEPTH {
                walk_dir(&path, depth + 1, out)?;
            }
        } else if ft.is_file() && is_markdown(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn is_markdown(path: &Path) -> bool {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("md") | Some("markdown") => true,
        _ => false,
    }
}

fn normalize_source_path(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

fn push_error(errors: &mut Vec<String>, msg: String) {
    if errors.len() < MAX_ERRORS {
        errors.push(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;
    use std::io::Write;

    fn temp_import_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "xhub-kb-{}-{}-{}",
            std::process::id(),
            nanos,
            tag
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn import_creates_folder_and_note() {
        let conn = init_in_memory().unwrap();
        let dir = temp_import_dir("folder");
        write_file(&dir.join("docs").join("hello.md"), "# Hello\n\nworld");

        let result = import_markdown(&conn, dir.to_str().unwrap()).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.updated, 0);
        assert_eq!(result.failed, 0);
        assert_eq!(result.total, 1);

        let folders = folder::list(&conn).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "docs");
        assert_eq!(folders[0].notes_count, 1);

        let n = note::find_by_source_path(&conn, "docs/hello.md")
            .unwrap()
            .expect("note by source_path");
        assert_eq!(n.title, "hello");
        assert!(n.content.contains("world"));
        assert_eq!(n.folder_id, Some(folders[0].id));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_same_source_path_updates_not_duplicates() {
        let conn = init_in_memory().unwrap();
        let dir = temp_import_dir("upsert");
        let file = dir.join("a.md");
        write_file(&file, "v1");

        let r1 = import_markdown(&conn, dir.to_str().unwrap()).unwrap();
        assert_eq!(r1.imported, 1);

        write_file(&file, "v2");
        let r2 = import_markdown(&conn, dir.to_str().unwrap()).unwrap();
        assert_eq!(r2.imported, 0);
        assert_eq!(r2.updated, 1);

        let notes = note::list(&conn).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].content, "v2");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_note_stub_ok() {
        assert!(index_note_stub(1).is_ok());
    }
}
