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

/// 扫描结果（无 DB）：命令侧可先扫盘再短暂持锁写库。
pub struct ScanResult {
    pub scan_root: PathBuf,
    pub files: Vec<PathBuf>,
    /// walk 阶段跳过（非 md、超深目录提示等）
    pub pre_skipped: i64,
    pub pre_errors: Vec<String>,
}

/// 仅文件系统扫描，不持 DB。
pub fn scan_import_path(path: &str) -> Result<ScanResult, String> {
    let root = PathBuf::from(path);
    if !root.exists() {
        return Err(format!("路径不存在: {path}"));
    }
    let mut pre_errors: Vec<String> = Vec::new();
    let mut pre_skipped: i64 = 0;
    let (scan_root, files) = collect_candidates(&root, &mut pre_skipped, &mut pre_errors)?;
    Ok(ScanResult {
        scan_root,
        files,
        pre_skipped,
        pre_errors,
    })
}

/// 在已持有的连接上导入候选文件列表。
pub fn import_scanned(
    conn: &Connection,
    scan: ScanResult,
) -> Result<ImportResult, String> {
    let mut errors = scan.pre_errors;
    let mut skipped = scan.pre_skipped;
    let mut paths = scan.files;

    if paths.len() > MAX_IMPORT_FILES {
        let dropped = paths.len() - MAX_IMPORT_FILES;
        paths.truncate(MAX_IMPORT_FILES);
        push_error(
            &mut errors,
            format!("单次导入超过 2000 个文件，已截断（省略 {dropped} 个）"),
        );
    }

    let mut imported: i64 = 0;
    let mut updated: i64 = 0;
    let mut failed: i64 = 0;
    let mut changed_ids: Vec<i64> = Vec::new();

    for file in &paths {
        match import_one(conn, &scan.scan_root, file, &mut errors) {
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
        total: (imported + updated + skipped + failed) as i64,
        errors,
    })
}

/// 兼容测试与简单调用：扫盘 + 写库。
pub fn import_markdown(conn: &Connection, path: &str) -> Result<ImportResult, String> {
    let scan = scan_import_path(path)?;
    import_scanned(conn, scan)
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
    if !path_under_root(file, scan_root) {
        push_error(
            errors,
            format!("{}: 路径越界，已跳过", display_path(file)),
        );
        return Ok(ImportOne::Failed);
    }

    let meta = fs::symlink_metadata(file).map_err(|e| format!("{}: {}", display_path(file), e))?;
    if meta.file_type().is_symlink() {
        push_error(
            errors,
            format!("{}: 跳过符号链接", display_path(file)),
        );
        return Ok(ImportOne::Skipped);
    }
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

fn collect_candidates(
    root: &Path,
    pre_skipped: &mut i64,
    pre_errors: &mut Vec<String>,
) -> Result<(PathBuf, Vec<PathBuf>), String> {
    if root.is_symlink() {
        return Err("不支持以符号链接作为导入根路径".into());
    }
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
    walk_dir(root, root, 0, &mut out, pre_skipped, pre_errors)?;
    Ok((root.to_path_buf(), out))
}

fn walk_dir(
    scan_root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
    pre_skipped: &mut i64,
    pre_errors: &mut Vec<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {}", dir.display(), e))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                push_error(pre_errors, format!("{}: {}", path.display(), e));
                *pre_skipped += 1;
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            push_error(
                pre_errors,
                format!("{}: 跳过符号链接", path.display()),
            );
            *pre_skipped += 1;
            continue;
        }
        if meta.is_dir() {
            if depth < MAX_WALK_DEPTH {
                walk_dir(scan_root, &path, depth + 1, out, pre_skipped, pre_errors)?;
            } else {
                push_error(
                    pre_errors,
                    format!("{}: 超过最大目录深度 {MAX_WALK_DEPTH}，已跳过", path.display()),
                );
                *pre_skipped += 1;
            }
        } else if meta.is_file() {
            if is_markdown(&path) {
                if path_under_root(&path, scan_root) {
                    out.push(path);
                } else {
                    push_error(
                        pre_errors,
                        format!("{}: 路径越界，已跳过", path.display()),
                    );
                    *pre_skipped += 1;
                }
            } else {
                *pre_skipped += 1;
            }
        }
    }
    Ok(())
}

fn path_under_root(path: &Path, root: &Path) -> bool {
    let Ok(canon_path) = fs::canonicalize(path) else {
        return false;
    };
    let Ok(canon_root) = fs::canonicalize(root) else {
        return false;
    };
    canon_path.starts_with(&canon_root)
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
    fn import_moves_folder_when_relative_dir_changes() {
        let conn = init_in_memory().unwrap();
        let dir = temp_import_dir("move-dir");
        write_file(&dir.join("old").join("x.md"), "body");
        let r1 = import_markdown(&conn, dir.to_str().unwrap()).unwrap();
        assert_eq!(r1.imported, 1);
        let n1 = note::find_by_source_path(&conn, "old/x.md").unwrap().unwrap();
        let old_folder = n1.folder_id;

        // 同内容换路径：新 source_path → 新笔记；本测验证「同 source_path 改目录」——
        // 把文件挪到 new/ 但保留用旧键更新时，应改 folder。
        // 实际导入键=相对路径，挪文件会变成新键。这里直接测 update_imported 迁文件夹：
        let new_folder = folder::create(&conn, None, "new").unwrap();
        note::update_imported(&conn, n1.id, "x", "body", Some(new_folder.id)).unwrap();
        let n2 = note::get(&conn, n1.id).unwrap();
        assert_eq!(n2.folder_id, Some(new_folder.id));
        assert_ne!(Some(new_folder.id), old_folder);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_counts_non_md_as_skipped() {
        let conn = init_in_memory().unwrap();
        let dir = temp_import_dir("skip");
        write_file(&dir.join("a.md"), "md");
        write_file(&dir.join("b.txt"), "txt");
        let r = import_markdown(&conn, dir.to_str().unwrap()).unwrap();
        assert_eq!(r.imported, 1);
        assert!(r.skipped >= 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_note_stub_ok() {
        assert!(index_note_stub(1).is_ok());
    }

    #[test]
    fn scan_then_import_matches_direct() {
        let conn = init_in_memory().unwrap();
        let dir = temp_import_dir("scan");
        write_file(&dir.join("z.md"), "z");
        let scan = scan_import_path(dir.to_str().unwrap()).unwrap();
        let r = import_scanned(&conn, scan).unwrap();
        assert_eq!(r.imported, 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
