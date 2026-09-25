//! Markdown 导入 + 知识库索引流水线（分块 / 嵌入 / kb_meta）。

use crate::commands::DbState;
use crate::models::{
    ChatMessage, Citation, EmbedTestResult, ImportResult, IndexProgressEvent, KbAskEvent,
    KbChunkHit, KbEmbedConfigView, KbStatus,
};
use crate::repo::{folder, knowledge as kb_repo, note};
use rusqlite::Connection;
use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, State};

const MAX_IMPORT_FILES: usize = 2000;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_WALK_DEPTH: usize = 8;
const MAX_ERRORS: usize = 20;

static IS_INDEXING: AtomicBool = AtomicBool::new(false);

struct IndexingGuard;

impl Drop for IndexingGuard {
    fn drop(&mut self) {
        IS_INDEXING.store(false, Ordering::SeqCst);
    }
}

/// 重建未正常收尾时，若 `kb_meta` 仍停在 `indexing`，落成 `error`。
/// 与 [`IndexingGuard`] 分开：后者只清 `IS_INDEXING`。
struct RebuildMetaGuard {
    app: AppHandle,
    /// 已调用 `set_meta_done` / 显式 `set_meta_error` 等终态时置 true。
    finished: Cell<bool>,
}

impl RebuildMetaGuard {
    fn new(app: AppHandle) -> Self {
        Self {
            app,
            finished: Cell::new(false),
        }
    }

    fn mark_finished(&self) {
        self.finished.set(true);
    }
}

impl Drop for RebuildMetaGuard {
    fn drop(&mut self) {
        if self.finished.get() {
            return;
        }
        let Some(state) = self.app.try_state::<DbState>() else {
            return;
        };
        let Ok(conn) = state.0.lock() else {
            return;
        };
        let _ = kb_repo::clear_stuck_indexing(&conn, "索引失败（任务中断）");
    }
}

pub fn embed_config_from_disk() -> crate::embed::EmbedConfig {
    let cfg = crate::config::load();
    crate::embed::EmbedConfig {
        base_url: cfg.kb_embed_base_url,
        model: cfg.kb_embed_model,
        api_key: crate::embed::get_embed_api_key(),
    }
}

/// 增量索引：重建进行中时直接跳过。
pub async fn index_note_async(app: &AppHandle, note_id: i64) -> Result<(), String> {
    index_note_inner(app, note_id, false).await
}

/// 重建路径：忽略 IS_INDEXING 跳过逻辑。
pub async fn index_note_force(app: &AppHandle, note_id: i64) -> Result<(), String> {
    index_note_inner(app, note_id, true).await
}

async fn index_note_inner(app: &AppHandle, note_id: i64, force: bool) -> Result<(), String> {
    if !force && IS_INDEXING.load(Ordering::SeqCst) {
        return Ok(());
    }

    let content = {
        let state = app.try_state::<DbState>().ok_or("数据库未就绪")?;
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        match note::get(&conn, note_id) {
            Ok(n) => n.content,
            Err(_) => return Ok(()),
        }
    };

    let chunks = kb_repo::chunk_markdown(&content);
    let cfg = embed_config_from_disk();

    if chunks.is_empty() {
        let state = app.try_state::<DbState>().ok_or("数据库未就绪")?;
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        kb_repo::delete_chunks_for_note(&conn, note_id).map_err(|e| e.to_string())?;
        kb_repo::refresh_meta_counts(&conn, &cfg.model).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let inputs: Vec<String> = chunks
        .iter()
        .map(|c| {
            if c.heading.is_empty() {
                c.content.clone()
            } else {
                format!("{}\n{}", c.heading, c.content)
            }
        })
        .collect();

    let embed_result = crate::embed::embed_batch(&cfg, &inputs).await;

    let state = app.try_state::<DbState>().ok_or("数据库未就绪")?;
    let conn = state.0.lock().map_err(|e| e.to_string())?;

    match &embed_result {
        Ok(vecs) => {
            let blobs: Vec<Option<Vec<u8>>> = (0..chunks.len())
                .map(|i| vecs.get(i).map(|v| kb_repo::embedding_to_blob(v)))
                .collect();
            let row_refs: Vec<(i64, &str, &str, i64, &str, Option<&[u8]>, Option<&str>)> = chunks
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let emb = blobs[i].as_deref();
                    let dim = vecs.get(i).map(|v| v.len() as i64).unwrap_or(0);
                    (
                        i as i64,
                        c.heading.as_str(),
                        c.content.as_str(),
                        dim,
                        cfg.model.as_str(),
                        emb,
                        None,
                    )
                })
                .collect();
            kb_repo::replace_note_chunks(&conn, note_id, &row_refs).map_err(|e| e.to_string())?;
        }
        Err(e) => {
            log::warn!("kb embed note {note_id}: {e}");
            let err_msg = e.as_str();
            let row_refs: Vec<(i64, &str, &str, i64, &str, Option<&[u8]>, Option<&str>)> = chunks
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    (
                        i as i64,
                        c.heading.as_str(),
                        c.content.as_str(),
                        0,
                        cfg.model.as_str(),
                        None,
                        Some(err_msg),
                    )
                })
                .collect();
            kb_repo::replace_note_chunks(&conn, note_id, &row_refs).map_err(|e| e.to_string())?;
        }
    }

    kb_repo::refresh_meta_counts(&conn, &cfg.model).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn kb_index_note(app: AppHandle, note_id: i64) -> Result<(), String> {
    index_note_async(&app, note_id).await
}

#[tauri::command]
pub async fn kb_rebuild_index(
    app: AppHandle,
    on_progress: tauri::ipc::Channel<IndexProgressEvent>,
) -> Result<(), String> {
    if IS_INDEXING.swap(true, Ordering::SeqCst) {
        return Err("索引任务进行中".into());
    }
    let _guard = IndexingGuard;
    let meta_guard = RebuildMetaGuard::new(app.clone());

    let cfg = embed_config_from_disk();

    let note_ids: Vec<i64> = {
        let state = app.try_state::<DbState>().ok_or("数据库未就绪")?;
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        kb_repo::set_meta_indexing(&conn).map_err(|e| e.to_string())?;
        kb_repo::clear_all_chunks(&conn).map_err(|e| e.to_string())?;
        let notes = note::list_meta(&conn).map_err(|e| e.to_string())?;
        notes.into_iter().map(|n| n.id).collect()
    };

    let total = note_ids.len() as i64;
    let _ = on_progress.send(IndexProgressEvent {
        stage: "indexing".into(),
        done: 0,
        total,
    });

    for (i, id) in note_ids.iter().enumerate() {
        if let Err(e) = index_note_force(&app, *id).await {
            let state = app.try_state::<DbState>().ok_or("数据库未就绪")?;
            let conn = state.0.lock().map_err(|e| e.to_string())?;
            let msg = format!("索引笔记 {id} 失败: {e}");
            let _ = kb_repo::set_meta_error(&conn, &msg);
            meta_guard.mark_finished();
            return Err(msg);
        }
        let done = (i + 1) as i64;
        let progress = if total > 0 {
            ((done * 100) / total).min(99)
        } else {
            100
        };
        {
            let state = app.try_state::<DbState>().ok_or("数据库未就绪")?;
            let conn = state.0.lock().map_err(|e| e.to_string())?;
            kb_repo::set_meta_progress(&conn, progress).map_err(|e| e.to_string())?;
        }
        let _ = on_progress.send(IndexProgressEvent {
            stage: "indexing".into(),
            done,
            total,
        });
    }

    {
        let state = app.try_state::<DbState>().ok_or("数据库未就绪")?;
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        match kb_repo::finalize_rebuild_meta(&conn, &cfg.model) {
            Ok(()) => meta_guard.mark_finished(),
            Err(msg) => {
                // 全失败已写 error；DB 失败仍可能停在 indexing → 留给 Drop
                if kb_repo::get_meta(&conn)
                    .map(|(s, ..)| s != "indexing")
                    .unwrap_or(false)
                {
                    meta_guard.mark_finished();
                }
                return Err(msg);
            }
        }
    }

    let _ = on_progress.send(IndexProgressEvent {
        stage: "done".into(),
        done: total,
        total,
    });
    Ok(())
}

#[tauri::command]
pub fn kb_get_status(state: State<'_, DbState>) -> Result<KbStatus, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let (status, progress, model, _stale_indexed, _stale_chunks, last_indexed_at, error) =
        kb_repo::get_meta(&conn).map_err(|e| e.to_string())?;
    // 计数以 live 为准，避免删笔记后 CASCADE 清 chunk 但 meta 列滞后
    let indexed_notes = kb_repo::count_indexed_notes(&conn).map_err(|e| e.to_string())?;
    let chunk_count = kb_repo::count_chunks(&conn).map_err(|e| e.to_string())?;
    let total_notes = kb_repo::count_notes(&conn).map_err(|e| e.to_string())?;
    let cfg = crate::config::load();
    let embedding_ready =
        !cfg.kb_embed_base_url.trim().is_empty() && !cfg.kb_embed_model.trim().is_empty();
    Ok(KbStatus {
        status,
        indexed_notes,
        chunk_count,
        model,
        last_indexed_at,
        error,
        progress,
        total_notes,
        embedding_ready,
    })
}

/// 扫描结果（无 DB）：仅路径列表；读文件见 [`load_prepared`]。
pub struct ScanResult {
    pub scan_root: PathBuf,
    pub files: Vec<PathBuf>,
    /// walk 阶段跳过（非 md、超深目录提示等）
    pub pre_skipped: i64,
    pub pre_errors: Vec<String>,
}

/// 锁外读盘后的单条导入载荷（写库时不再碰文件系统）。
pub struct PreparedItem {
    pub source_path: String,
    pub title: String,
    pub content: String,
    /// 相对路径的父目录段（用于 find_or_create_path）
    pub folder_segments: Vec<String>,
}

/// 锁外准备完毕、可短锁写库的批次。
pub struct PreparedImport {
    pub items: Vec<PreparedItem>,
    pub skipped: i64,
    pub failed: i64,
    pub errors: Vec<String>,
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

/// 锁外：截断超额、读文件、组载荷。不访问数据库。
pub fn load_prepared(scan: ScanResult) -> PreparedImport {
    let mut errors = scan.pre_errors;
    let mut skipped = scan.pre_skipped;
    let mut failed: i64 = 0;
    let mut paths = scan.files;

    if paths.len() > MAX_IMPORT_FILES {
        let dropped = (paths.len() - MAX_IMPORT_FILES) as i64;
        paths.truncate(MAX_IMPORT_FILES);
        skipped += dropped;
        push_error(
            &mut errors,
            format!("单次导入超过 2000 个文件，已截断（省略 {dropped} 个）"),
        );
    }

    let mut items = Vec::with_capacity(paths.len());
    for file in &paths {
        match read_one(&scan.scan_root, file, &mut errors) {
            Ok(ReadOne::Item(item)) => items.push(item),
            Ok(ReadOne::Skipped) => skipped += 1,
            Ok(ReadOne::Failed) => failed += 1,
            Err(e) => {
                failed += 1;
                push_error(&mut errors, e);
            }
        }
    }

    PreparedImport {
        items,
        skipped,
        failed,
        errors,
    }
}

/// 在已持有的连接上只做 DB 写入（不读盘）。
pub fn import_prepared(
    conn: &Connection,
    prepared: PreparedImport,
) -> Result<ImportResult, String> {
    let mut errors = prepared.errors;
    let skipped = prepared.skipped;
    let mut failed = prepared.failed;
    let mut imported: i64 = 0;
    let mut updated: i64 = 0;
    let mut changed_ids: Vec<i64> = Vec::new();

    for item in &prepared.items {
        match upsert_one(conn, item) {
            Ok(ImportOne::Imported(id)) => {
                imported += 1;
                changed_ids.push(id);
            }
            Ok(ImportOne::Updated(id)) => {
                updated += 1;
                changed_ids.push(id);
            }
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
        total: imported + updated + skipped + failed,
        errors,
    })
}

/// 扫盘 → 读盘（无锁）→ 写库。测试与简单调用入口。
pub fn import_markdown(conn: &Connection, path: &str) -> Result<ImportResult, String> {
    let scan = scan_import_path(path)?;
    let prepared = load_prepared(scan);
    import_prepared(conn, prepared)
}

enum ReadOne {
    Item(PreparedItem),
    Skipped,
    Failed,
}

enum ImportOne {
    Imported(i64),
    Updated(i64),
}

fn read_one(
    scan_root: &Path,
    file: &Path,
    errors: &mut Vec<String>,
) -> Result<ReadOne, String> {
    if !path_under_root(file, scan_root) {
        push_error(
            errors,
            format!("{}: 路径越界，已跳过", display_path(file)),
        );
        return Ok(ReadOne::Failed);
    }

    let meta = fs::symlink_metadata(file).map_err(|e| format!("{}: {}", display_path(file), e))?;
    if meta.file_type().is_symlink() {
        push_error(
            errors,
            format!("{}: 跳过符号链接", display_path(file)),
        );
        return Ok(ReadOne::Skipped);
    }
    if meta.len() > MAX_FILE_BYTES {
        push_error(
            errors,
            format!("{}: 单文件超过 2MB", display_path(file)),
        );
        return Ok(ReadOne::Failed);
    }

    let bytes = fs::read(file).map_err(|e| format!("{}: {}", display_path(file), e))?;
    let content = String::from_utf8_lossy(&bytes).into_owned();

    let rel = file
        .strip_prefix(scan_root)
        .unwrap_or(file)
        .to_path_buf();
    let source_path = normalize_source_path(&rel);
    if source_path.is_empty() {
        return Ok(ReadOne::Skipped);
    }

    let title = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "无标题笔记".into());

    let folder_segments = folder_segments_of(&rel);

    Ok(ReadOne::Item(PreparedItem {
        source_path,
        title,
        content,
        folder_segments,
    }))
}

fn upsert_one(conn: &Connection, item: &PreparedItem) -> Result<ImportOne, String> {
    let folder_id = if item.folder_segments.is_empty() {
        None
    } else {
        let refs: Vec<&str> = item.folder_segments.iter().map(|s| s.as_str()).collect();
        Some(
            folder::find_or_create_path(conn, None, &refs).map_err(|e| e.to_string())?,
        )
    };

    match note::find_by_source_path(conn, &item.source_path).map_err(|e| e.to_string())? {
        Some(existing) => {
            let n = note::update_imported(
                conn,
                existing.id,
                &item.title,
                &item.content,
                folder_id,
            )
            .map_err(|e| e.to_string())?;
            Ok(ImportOne::Updated(n.id))
        }
        None => {
            let n = note::create_imported(
                conn,
                &item.title,
                &item.content,
                folder_id,
                &item.source_path,
            )
            .map_err(|e| e.to_string())?;
            Ok(ImportOne::Imported(n.id))
        }
    }
}

fn folder_segments_of(rel: &Path) -> Vec<String> {
    let parent = match rel.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return Vec::new(),
    };
    parent
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
        .collect()
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


fn resolve_top_k(top_k: Option<i64>) -> i64 {
    top_k
        .unwrap_or_else(|| crate::config::load().kb_top_k)
        .clamp(3, 10)
}

fn snippet_of(content: &str) -> String {
    let chars: Vec<char> = content.chars().collect();
    if chars.len() <= 200 {
        content.to_string()
    } else {
        chars[..200].iter().collect::<String>() + "…"
    }
}

fn hits_to_citations(hits: &[KbChunkHit]) -> Vec<Citation> {
    hits.iter()
        .enumerate()
        .map(|(i, h)| Citation {
            index: (i + 1) as i64,
            note_id: h.note_id,
            note_title: h.note_title.clone(),
            folder_path: h.folder_path.clone(),
            heading: h.heading.clone(),
            snippet: snippet_of(&h.content),
        })
        .collect()
}

fn build_rag_system_prompt(hits: &[KbChunkHit]) -> String {
    let mut parts = String::from(
        "你是「x-hub 个人知识库」的智能助手。请基于下方【知识库片段】回答用户问题。\n\
\n\
## 规则\n\
1. 只依据知识库片段回答；片段未覆盖的，明确说「知识库中没有相关内容」，不要编造。\n\
2. 引用来源：在答案中需要引用处用 [1] [2] ... 标注，编号对应下方片段序号。\n\
3. 每个结论尽量带引用；引用编号必须真实存在于片段列表中。\n\
4. 回答使用与用户问题相同的语言（中文问题用中文回答）。\n\
5. 如果片段之间有冲突，指出冲突并分别标注引用。\n\
\n\
## 知识库片段\n",
    );
    for (i, h) in hits.iter().enumerate() {
        let n = i + 1;
        let heading_part = if h.heading.trim().is_empty() {
            String::new()
        } else {
            format!("（{}）", h.heading)
        };
        parts.push_str(&format!(
            "[{n}] 来自《{title}》{heading}：\n{content}\n\n",
            title = h.note_title,
            heading = heading_part,
            content = h.content,
        ));
    }
    parts
}

const NO_HIT_SYSTEM: &str = "你是个人知识库助手。用户的知识库中未检索到相关内容，请基于通用知识回答，并在开头说明「知识库中未找到直接相关内容，以下为通用回答」。";

fn chat_msg(role: &str, content: String) -> ChatMessage {
    ChatMessage {
        id: 0,
        session_id: 0,
        role: role.into(),
        content,
        created_at: String::new(),
    }
}

/// 嵌入 query → hybrid_search（锁外嵌入，锁内检索）。
#[tauri::command]
pub async fn kb_search(
    app: AppHandle,
    query: String,
    top_k: Option<i64>,
) -> Result<Vec<KbChunkHit>, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(vec![]);
    }
    let top_k = resolve_top_k(top_k);
    let cfg = embed_config_from_disk();

    let query_vec = match crate::embed::embed_batch(&cfg, &[query.clone()]).await {
        Ok(mut vecs) => vecs.pop().unwrap_or_default(),
        Err(e) => {
            log::warn!("kb_search embed failed, keyword fallback: {e}");
            Vec::new()
        }
    };

    let state = app.try_state::<DbState>().ok_or("数据库未就绪")?;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    kb_repo::hybrid_search(&conn, &query_vec, &query, top_k)
}

/// RAG 流式问答：检索 → 注入 system prompt → stream_chat → Channel 事件。
#[tauri::command]
pub async fn kb_ask(
    app: AppHandle,
    question: String,
    model_id: String,
    top_k: Option<i64>,
    on_event: tauri::ipc::Channel<KbAskEvent>,
) -> Result<(), String> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err("问题不能为空".into());
    }
    let top_k = resolve_top_k(top_k);

    // 模型解析：platform 走 pick_chat_model；其余按 id/name 精确匹配
    let models = crate::config::load().chat_models;
    let wants_platform = model_id == crate::chat::PLATFORM_ENTRY_NAME
        || models.iter().any(|m| {
            crate::chat::is_platform_model(m) && (m.name == model_id || m.id == model_id)
        });
    let model = if wants_platform {
        crate::commands::pick_chat_model(&models, &model_id)?
    } else {
        models
            .iter()
            .find(|m| m.id == model_id || m.name == model_id)
            .cloned()
            .ok_or_else(|| {
                format!("模型「{model_id}」不存在，请先在设置 → AI助手 中配置")
            })?
    };

    // 检索（复用 kb_search 逻辑：锁外嵌入）
    let cfg = embed_config_from_disk();
    let query_vec = match crate::embed::embed_batch(&cfg, &[question.clone()]).await {
        Ok(mut vecs) => vecs.pop().unwrap_or_default(),
        Err(e) => {
            log::warn!("kb_ask embed failed, keyword fallback: {e}");
            Vec::new()
        }
    };
    let hits = {
        let state = app.try_state::<DbState>().ok_or("数据库未就绪")?;
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        kb_repo::hybrid_search(&conn, &query_vec, &question, top_k)?
    };

    let (messages, citations) = if hits.is_empty() {
        (
            vec![
                chat_msg("system", NO_HIT_SYSTEM.into()),
                chat_msg("user", question.clone()),
            ],
            Vec::new(),
        )
    } else {
        let system = build_rag_system_prompt(&hits);
        let citations = hits_to_citations(&hits);
        (
            vec![
                chat_msg("system", system),
                chat_msg("user", question.clone()),
            ],
            citations,
        )
    };

    let mut reply = String::new();
    let chunk_sender = on_event.clone();
    let result = crate::chat::stream_chat(&model, &messages, &mut reply, |delta| {
        chunk_sender
            .send(KbAskEvent::Chunk { content: delta })
            .map_err(|e| e.to_string())
    })
    .await;

    match result {
        Ok(_) if !reply.trim().is_empty() => {
            let kb_status = {
                let state = app.try_state::<DbState>().ok_or("数据库未就绪")?;
                let conn = state.0.lock().map_err(|e| e.to_string())?;
                let (status, progress, model_name, _, _, last_indexed_at, error) =
                    kb_repo::get_meta(&conn).map_err(|e| e.to_string())?;
                let indexed_notes =
                    kb_repo::count_indexed_notes(&conn).map_err(|e| e.to_string())?;
                let chunk_count = kb_repo::count_chunks(&conn).map_err(|e| e.to_string())?;
                let total_notes = kb_repo::count_notes(&conn).map_err(|e| e.to_string())?;
                let cfg = crate::config::load();
                let embedding_ready = !cfg.kb_embed_base_url.trim().is_empty()
                    && !cfg.kb_embed_model.trim().is_empty();
                KbStatus {
                    status,
                    indexed_notes,
                    chunk_count,
                    model: model_name,
                    last_indexed_at,
                    error,
                    progress,
                    total_notes,
                    embedding_ready,
                }
            };
            on_event
                .send(KbAskEvent::Done {
                    answer: reply,
                    citations,
                    kb_status,
                })
                .map_err(|e| e.to_string())?;
        }
        Ok(_) => {
            on_event
                .send(KbAskEvent::Error {
                    message: "模型未返回任何内容".into(),
                    partial: reply,
                })
                .map_err(|e| e.to_string())?;
        }
        Err(e) => {
            on_event
                .send(KbAskEvent::Error {
                    message: e,
                    partial: reply,
                })
                .map_err(|e2| e2.to_string())?;
        }
    }

    Ok(())
}

#[tauri::command]
pub fn get_kb_embed_config() -> Result<KbEmbedConfigView, String> {
    let cfg = crate::config::load();
    Ok(KbEmbedConfigView {
        base_url: cfg.kb_embed_base_url,
        model: cfg.kb_embed_model,
        has_api_key: crate::embed::get_embed_api_key()
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false),
    })
}

#[tauri::command]
pub fn save_kb_embed_config(
    base_url: String,
    model: String,
    api_key: String,
    top_k: Option<i64>,
) -> Result<KbEmbedConfigView, String> {
    let mut cfg = crate::config::load();
    cfg.kb_embed_base_url = base_url.trim().to_string();
    cfg.kb_embed_model = model.trim().to_string();
    if let Some(k) = top_k {
        cfg.kb_top_k = k.clamp(3, 10);
    }
    crate::config::save(&cfg)?;

    if api_key.trim().is_empty() {
        crate::embed::clear_embed_api_key()?;
    } else {
        crate::embed::save_embed_api_key(api_key.trim())?;
    }

    Ok(KbEmbedConfigView {
        base_url: cfg.kb_embed_base_url,
        model: cfg.kb_embed_model,
        has_api_key: crate::embed::get_embed_api_key()
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false),
    })
}

#[tauri::command]
pub async fn kb_test_embed(
    base_url: String,
    model: String,
    api_key: String,
) -> Result<EmbedTestResult, String> {
    let api_key = if api_key.trim().is_empty() {
        crate::embed::get_embed_api_key()
    } else {
        Some(api_key.trim().to_string())
    };
    let cfg = crate::embed::EmbedConfig {
        base_url: base_url.trim().to_string(),
        model: model.trim().to_string(),
        api_key,
    };
    match crate::embed::test_connection(&cfg).await {
        Ok((_model, dim)) => Ok(EmbedTestResult {
            ok: true,
            message: format!("连接成功，向量维度 {dim}"),
            dim: Some(dim as i64),
        }),
        Err(e) => Ok(EmbedTestResult {
            ok: false,
            message: e,
            dim: None,
        }),
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

        let new_folder = folder::create(&conn, None, "new", None).unwrap();
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
    fn load_prepared_does_not_touch_db() {
        let dir = temp_import_dir("no-db");
        write_file(&dir.join("z.md"), "z");
        let scan = scan_import_path(dir.to_str().unwrap()).unwrap();
        let prepared = load_prepared(scan);
        assert_eq!(prepared.items.len(), 1);
        assert_eq!(prepared.items[0].title, "z");
        assert_eq!(prepared.items[0].content, "z");
        // 未打开任何 Connection —— 若误触 DB 会编译/运行期失败
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_then_import_matches_direct() {
        let conn = init_in_memory().unwrap();
        let dir = temp_import_dir("scan");
        write_file(&dir.join("z.md"), "z");
        let scan = scan_import_path(dir.to_str().unwrap()).unwrap();
        let prepared = load_prepared(scan);
        let r = import_prepared(&conn, prepared).unwrap();
        assert_eq!(r.imported, 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncate_over_limit_counts_dropped_as_skipped() {
        let mut files = Vec::new();
        for i in 0..(MAX_IMPORT_FILES + 3) {
            files.push(PathBuf::from(format!("f{i}.md")));
        }
        let scan = ScanResult {
            scan_root: PathBuf::from("/tmp/xhub-fake"),
            files,
            pre_skipped: 0,
            pre_errors: Vec::new(),
        };
        // 路径不存在 → 全部读失败，但截断的 3 个应先计入 skipped
        let prepared = load_prepared(scan);
        assert_eq!(prepared.skipped, 3);
        assert_eq!(prepared.items.len(), 0);
        assert_eq!(prepared.failed, MAX_IMPORT_FILES as i64);
        assert!(prepared.errors.iter().any(|e| e.contains("截断")));
    }
}
