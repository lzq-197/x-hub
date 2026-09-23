# Knowledge-Base Notes Structure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver multi-level note folders + Markdown batch import in 速记 (PRD M2+M4), with index stub hooks and promote-on-delete semantics.

**Architecture:** SQLite `note_folders` + `notes.folder_id`/`source_path`; Rust `repo/folder` + extended `repo/note` + `knowledge` import/stub; Vue NoteList tree + ImportDialog; no KnowledgeView/RAG tables.

**Tech Stack:** Tauri 2, Rusqlite, Vue 3, TypeScript, `@tauri-apps/plugin-dialog`

## Global Constraints

- Work only on branch `feature/kb-notes-structure` (from latest `develop`); never commit product code to `master`
- Do **not** create `kb_chunks` / `kb_meta` / `embed.rs` / `KnowledgeView` / sidebar `kb` nav
- Delete folder = promote child folders to parent + direct notes → `folder_id=NULL`; do **not** cascade-delete notes
- `kb_index_note` is a no-op stub (no network, no DB writes beyond existing note ops)
- No new Cargo/npm dependencies
- Errors: `Result<T, String>` with readable Chinese messages
- Windows Rust tests: `npm run tauri:test` (not bare `cargo test`)
- Spec: `docs/superpowers/specs/2026-09-23-kb-notes-structure-design.md`
- Follow existing patterns in `repo/note.rs`, `db.rs` migrate, `commands.rs`, `tauri.ts`

## File map

| Path | Responsibility |
|------|----------------|
| `src-tauri/src/db.rs` | `note_folders` table + `notes.folder_id`/`source_path` columns |
| `src-tauri/src/models.rs` | `NoteFolder`, `ImportResult`; extend `Note` |
| `src-tauri/src/repo/folder.rs` | Folder CRUD, promote-delete, find_or_create path |
| `src-tauri/src/repo/note.rs` | SELECT/INSERT include folder fields; set_folder; by source_path |
| `src-tauri/src/kb_hooks.rs` | `on_notes_changed` → stub index |
| `src-tauri/src/knowledge.rs` | `import_markdown` + `kb_index_note` command helpers |
| `src-tauri/src/commands.rs` | Tauri command wrappers |
| `src-tauri/src/lib.rs` | `mod` + `invoke_handler` |
| `src/api/tauri.ts` | Types + invoke wrappers |
| `src/stores/workbench.ts` | `folders` state + methods |
| `src/components/NoteList.vue` | Tree + filtered list |
| `src/components/ImportDialog.vue` | Import UI |
| `src/index/index.vue` | Wire import + folder open (no kb nav) |
| `src/components/NoteEditor.vue` | Optional folder path label |

---

### Task 1: Create feature branch from develop

**Files:** none (git only)

**Interfaces:**
- Consumes: `origin/develop` tip (includes structure design spec)
- Produces: local+remote `feature/kb-notes-structure` based on current develop

- [ ] **Step 1: Sync develop and create/update feature branch**

```powershell
git fetch origin
git checkout develop
git pull origin develop
# If remote feature exists, track it then merge develop; else create fresh
git checkout -B feature/kb-notes-structure origin/feature/kb-notes-structure 2>$null
if ($LASTEXITCODE -ne 0) { git checkout -b feature/kb-notes-structure }
git merge develop -m "merge develop into feature/kb-notes-structure"
git push -u origin feature/kb-notes-structure
```

Expected: current branch `feature/kb-notes-structure`; contains `docs/superpowers/specs/2026-09-23-kb-notes-structure-design.md`.

- [ ] **Step 2: No code commit**

---

### Task 2: Schema migrate + models

**Files:**
- Modify: `src-tauri/src/models.rs` (`Note` ~L27–33)
- Modify: `src-tauri/src/db.rs` (`migrate` before final `Ok(())`)
- Test: add test in `db.rs` `#[cfg(test)]` or extend existing migrate tests if present; otherwise test via `init_in_memory` in folder tests (Task 3). For this task add a small test module in `db.rs` if none exists for column presence.

**Interfaces:**
- Produces:
  - `Note { ..., folder_id: Option<i64>, source_path: Option<String> }` with `#[serde(default)]` on new fields
  - `NoteFolder { id, parent_id: Option<i64>, name, sort_order, notes_count, subtree_notes, created_at, updated_at }`
  - `ImportResult { imported, updated, skipped, failed, total, errors: Vec<String> }`
  - Tables/columns as in spec §3

- [ ] **Step 1: Extend `Note` and add structs in `models.rs`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub folder_id: Option<i64>,
    #[serde(default)]
    pub source_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteFolder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub sort_order: i64,
    #[serde(default)]
    pub notes_count: i64,
    #[serde(default)]
    pub subtree_notes: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub imported: i64,
    pub updated: i64,
    pub skipped: i64,
    pub failed: i64,
    pub total: i64,
    pub errors: Vec<String>,
}
```

- [ ] **Step 2: In `migrate()`, append `CREATE TABLE IF NOT EXISTS note_folders (...)` inside or after the main `execute_batch`, and PRAGMA+ALTER for notes columns**

Follow existing todos pattern in `db.rs` (~L358+):

```rust
conn.execute_batch(
    r#"
    CREATE TABLE IF NOT EXISTS note_folders (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      parent_id INTEGER REFERENCES note_folders(id) ON DELETE CASCADE,
      name TEXT NOT NULL,
      sort_order INTEGER NOT NULL DEFAULT 0,
      created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
      updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
    );
    "#,
)?;

let note_cols: Vec<String> = conn
    .prepare("PRAGMA table_info(notes)")?
    .query_map([], |row| row.get(1))?
    .collect::<rusqlite::Result<Vec<_>>>()?;
if !note_cols.iter().any(|c| c == "folder_id") {
    conn.execute(
        "ALTER TABLE notes ADD COLUMN folder_id INTEGER REFERENCES note_folders(id) ON DELETE SET NULL",
        [],
    )?;
}
if !note_cols.iter().any(|c| c == "source_path") {
    conn.execute("ALTER TABLE notes ADD COLUMN source_path TEXT", [])?;
}
```

Do **not** create `kb_chunks` / `kb_meta`.

- [ ] **Step 3: Fix `repo/note.rs` `row_to_note` and all SELECT lists to include `folder_id`, `source_path` (default NULL)**

Update every `SELECT id, title, content, created_at, updated_at` to:

```sql
SELECT id, title, content, folder_id, source_path, created_at, updated_at
```

For `list_meta`, use empty content but still select folder fields:

```sql
SELECT id, title, '', folder_id, source_path, created_at, updated_at FROM notes ...
```

```rust
pub fn row_to_note(row: &rusqlite::Row) -> Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        folder_id: row.get(3)?,
        source_path: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}
```

- [ ] **Step 4: Run note unit tests**

```powershell
npm run tauri:test -- --lib repo::note::tests -- --nocapture
```

Expected: existing note tests PASS (folder_id/source_path None).

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/models.rs src-tauri/src/db.rs src-tauri/src/repo/note.rs
git commit -m "feat(kb): add note_folders schema and Note folder fields"
```

---

### Task 3: `repo/folder.rs` — CRUD + promote delete + anti-cycle

**Files:**
- Create: `src-tauri/src/repo/folder.rs`
- Modify: `src-tauri/src/repo/mod.rs` — `pub mod folder;`

**Interfaces:**
- Produces (exact names for later tasks):
  - `pub fn create(conn, parent_id: Option<i64>, name: &str) -> Result<NoteFolder>`
  - `pub fn rename(conn, id: i64, name: &str) -> Result<NoteFolder>`
  - `pub fn list(conn) -> Result<Vec<NoteFolder>>` (with counts)
  - `pub fn get(conn, id: i64) -> Result<NoteFolder>`
  - `pub fn move_folder(conn, id: i64, new_parent_id: Option<i64>) -> Result<NoteFolder>`
  - `pub fn delete_promote(conn, id: i64) -> Result<(i64 /*direct notes cleared*/, i64 /*child folders promoted*/)>`
  - `pub fn collect_descendant_ids(conn, id: i64) -> Result<Vec<i64>>` (includes self)
  - `pub fn find_or_create_path(conn, parent_id: Option<i64>, segments: &[&str]) -> Result<i64>` (for import)
  - `pub fn depth_of(conn, id: i64) -> Result<i64>` — reject create/move if depth > 8

- [ ] **Step 1: Write failing tests in `folder.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;
    use crate::repo::note;

    #[test]
    fn delete_promotes_children_and_unclassifies_direct_notes() {
        let conn = init_in_memory().unwrap();
        let root = create(&conn, None, "工作").unwrap();
        let child = create(&conn, Some(root.id), "项目").unwrap();
        let n = note::create(&conn, "直属").unwrap();
        note::set_folder(&conn, n.id, Some(root.id)).unwrap();
        let n2 = note::create(&conn, "子内").unwrap();
        note::set_folder(&conn, n2.id, Some(child.id)).unwrap();

        let (cleared, promoted) = delete_promote(&conn, root.id).unwrap();
        assert_eq!(cleared, 1);
        assert_eq!(promoted, 1);
        assert!(get(&conn, root.id).is_err());
        let child2 = get(&conn, child.id).unwrap();
        assert_eq!(child2.parent_id, None);
        assert_eq!(note::get(&conn, n.id).unwrap().folder_id, None);
        assert_eq!(note::get(&conn, n2.id).unwrap().folder_id, Some(child.id));
    }

    #[test]
    fn move_rejects_cycle() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, None, "A").unwrap();
        let b = create(&conn, Some(a.id), "B").unwrap();
        let err = move_folder(&conn, a.id, Some(b.id)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("环") || msg.contains("子孙") || msg.contains("INVALID"));
    }
}
```

Note: `note::set_folder` is added in Task 4 — if compiling Task 3 alone, either implement a minimal `set_folder` in note.rs in this task’s Step 3 before tests, or keep tests behind implementing set_folder first. **Order within this task: implement `set_folder` stub in note.rs first (Task 4 Step partial), then folder tests.**

Simpler order: implement `note::set_folder` in Task 3 Step 2a before folder tests.

- [ ] **Step 2: Add `note::set_folder` minimal**

In `repo/note.rs`:

```rust
pub fn set_folder(conn: &Connection, note_id: i64, folder_id: Option<i64>) -> Result<Note> {
    let affected = conn.execute(
        "UPDATE notes SET folder_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![folder_id, now(), note_id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {note_id} 不存在"
        )));
    }
    get(conn, note_id)
}
```

- [ ] **Step 3: Run tests — expect FAIL (folder module missing)**

```powershell
npm run tauri:test -- --lib repo::folder::tests -- --nocapture
```

Expected: compile fail or test fail until implementation.

- [ ] **Step 4: Implement `folder.rs`**

Key `delete_promote`:

```rust
pub fn delete_promote(conn: &Connection, id: i64) -> Result<(i64, i64)> {
    let folder = get(conn, id)?; // must exist
    let parent = folder.parent_id;
    let promoted: i64 = conn.execute(
        "UPDATE note_folders SET parent_id = ?1, updated_at = ?2 WHERE parent_id = ?3",
        params![parent, now(), id],
    )? as i64;
    let cleared: i64 = conn.execute(
        "UPDATE notes SET folder_id = NULL, updated_at = ?1 WHERE folder_id = ?2",
        params![now(), id],
    )? as i64;
    conn.execute("DELETE FROM note_folders WHERE id = ?1", params![id])?;
    Ok((cleared, promoted))
}
```

`move_folder`: if `new_parent_id` is Some and in `collect_descendant_ids(id)`, return `InvalidParameterName("不能将文件夹移动到自身或其子文件夹下".into())`.

`list`: query folders then attach counts (direct `COUNT` on notes; subtree via descendants — OK to compute in Rust for small trees).

`find_or_create_path`: for each segment under current parent, `SELECT id FROM note_folders WHERE parent_id IS ? AND name = ?` (handle NULL parent with `parent_id IS NULL`), else create.

- [ ] **Step 5: Run tests — expect PASS**

```powershell
npm run tauri:test -- --lib repo::folder::tests -- --nocapture
```

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/repo/folder.rs src-tauri/src/repo/mod.rs src-tauri/src/repo/note.rs
git commit -m "feat(kb): folder repo with promote-on-delete and anti-cycle move"
```

---

### Task 4: Note source_path helpers + import prep

**Files:**
- Modify: `src-tauri/src/repo/note.rs`

**Interfaces:**
- Produces:
  - `pub fn find_by_source_path(conn, path: &str) -> Result<Option<Note>>`
  - `pub fn create_imported(conn, title, content, folder_id, source_path) -> Result<Note>`
  - `pub fn update_imported(conn, id, title, content, folder_id) -> Result<Note>`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn import_key_updates_same_source_path() {
    let conn = init_in_memory().unwrap();
    let a = create_imported(&conn, "a", "v1", None, "docs/a.md").unwrap();
    let b = update_imported(&conn, a.id, "a", "v2", None).unwrap();
    assert_eq!(b.content, "v2");
    assert!(find_by_source_path(&conn, "docs/a.md").unwrap().unwrap().id == a.id);
}
```

- [ ] **Step 2: Run — FAIL then implement**

```rust
pub fn find_by_source_path(conn: &Connection, path: &str) -> Result<Option<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, folder_id, source_path, created_at, updated_at FROM notes WHERE source_path = ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query(params![path])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_note(row)?)),
        None => Ok(None),
    }
}

pub fn create_imported(
    conn: &Connection,
    title: &str,
    content: &str,
    folder_id: Option<i64>,
    source_path: &str,
) -> Result<Note> {
    let ts = now();
    conn.execute(
        "INSERT INTO notes (title, content, folder_id, source_path, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?5)",
        params![title, content, folder_id, source_path, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn update_imported(
    conn: &Connection,
    id: i64,
    title: &str,
    content: &str,
    folder_id: Option<i64>,
) -> Result<Note> {
    let affected = conn.execute(
        "UPDATE notes SET title=?1, content=?2, folder_id=?3, updated_at=?4 WHERE id=?5",
        params![title, content, folder_id, now(), id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    get(conn, id)
}
```

- [ ] **Step 3: Run tests PASS + commit**

```powershell
npm run tauri:test -- --lib repo::note::tests -- --nocapture
git add src-tauri/src/repo/note.rs
git commit -m "feat(kb): note helpers for import source_path upsert"
```

---

### Task 5: `kb_hooks` stub + `knowledge` import + commands registration

**Files:**
- Create: `src-tauri/src/kb_hooks.rs`
- Create: `src-tauri/src/knowledge.rs`
- Modify: `src-tauri/src/lib.rs` — `mod kb_hooks; mod knowledge;`
- Modify: `src-tauri/src/commands.rs` — folder + import + stub commands; hook create/update note
- Modify: `src-tauri/src/lib.rs` invoke_handler list

**Interfaces:**
- Produces Tauri commands (snake_case invoke names):
  - `list_note_folders` → `Vec<NoteFolder>`
  - `create_note_folder(parentId: Option<i64>, name: String)`
  - `rename_note_folder(id, name)`
  - `move_note_folder(id, newParentId: Option<i64>)`
  - `delete_note_folder(id, moveTo: Option<i64>)` — if `moveTo.is_some()` return `Err("本版本不支持迁移删除".into())`; else `folder::delete_promote`
  - `set_note_folder(noteId, folderId: Option<i64>)`
  - `import_markdown(path: String)` → `ImportResult` (progress Channel optional; may omit Channel in v1 and keep signature without it if simpler — prefer without Channel first: `import_markdown(path: String) -> Result<ImportResult, String>`)
  - `kb_index_note(noteId: i64) -> Result<(), String>` always Ok

- [ ] **Step 1: Implement stub**

```rust
// kb_hooks.rs
pub fn on_notes_changed(_note_ids: &[i64]) {
    // RAG 段替换：真正的 kb_index_note / 队列
    let _ = crate::knowledge::index_note_stub;
}

// knowledge.rs
pub fn index_note_stub(_note_id: i64) -> Result<(), String> {
    Ok(())
}
```

Call `kb_hooks::on_notes_changed(&[id])` at end of `create_note` / `update_note` after success (ignore errors — wrap in `let _ =`).

- [ ] **Step 2: Implement `import_markdown` in `knowledge.rs`**

Algorithm (sync is OK; use `std::fs`):

1. Resolve `path` as file or directory
2. Collect candidate paths (depth ≤ 8, skip `.*` names, only `.md`/`.markdown`)
3. If count > 2000, truncate to 2000 and push error `"单次导入超过 2000 个文件，已截断"`
4. For each file: if size > 2MB → failed++; else read UTF-8 (lossy OK); compute relative path with `/`; folder segments = parent dirs; `find_or_create_path`; title = file stem; upsert by `source_path`
5. After loop, `kb_hooks::on_notes_changed(&changed_ids)`
6. Return `ImportResult`

- [ ] **Step 3: Unit test import with tempdir**

```rust
#[test]
fn import_creates_folder_and_note() {
    let conn = init_in_memory().unwrap();
    let dir = tempfile::tempdir().unwrap(); // ONLY if tempfile already in Cargo.toml
}
```

If `tempfile` is **not** in Cargo.toml, do **not** add dependency — write files under `std::env::temp_dir().join(format!("xhub-kb-{}", std::process::id()))` and clean up manually.

- [ ] **Step 4: Wire commands + register in `lib.rs` next to note commands**

Use camelCase args in `#[tauri::command]` as existing commands do (`parentId`, etc.).

- [ ] **Step 5: Run**

```powershell
npm run tauri:test -- --lib -- --nocapture
```

Expected: PASS (or fix failures).

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/kb_hooks.rs src-tauri/src/knowledge.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(kb): folder/import commands and index stub hooks"
```

---

### Task 6: Frontend API + store

**Files:**
- Modify: `src/api/tauri.ts` — `Note` interface + folder/import/kbIndexNote
- Modify: `src/stores/workbench.ts` — `folders`, refresh/CRUD/import methods

**Interfaces:**
- Produces TypeScript:

```ts
export interface Note {
  id: number
  title: string
  content: string
  folder_id?: number | null
  source_path?: string | null
  created_at: string
  updated_at: string
}
export interface NoteFolder {
  id: number
  parent_id: number | null
  name: string
  sort_order: number
  notes_count: number
  subtree_notes: number
  created_at: string
  updated_at: string
}
export interface ImportResult {
  imported: number
  updated: number
  skipped: number
  failed: number
  total: number
  errors: string[]
}
// tauriApi.listNoteFolders, createNoteFolder, renameNoteFolder, deleteNoteFolder(id, moveTo: null),
// moveNoteFolder, setNoteFolder, importMarkdown, kbIndexNote
```

Store methods: `refreshFolders`, `createFolder`, `renameFolder`, `deleteFolder`, `moveFolder`, `setNoteFolder`, `importMarkdown` (calls API then `refreshNotes`+`refreshFolders`).

Load folders in existing bootstrap that loads notes (find `getInitialData` / `loadAll` path and also `refreshFolders` there — if InitialData not extended, separate `listNoteFolders` call on app load is fine).

- [ ] **Step 1: Add API wrappers**
- [ ] **Step 2: Add store state + methods**
- [ ] **Step 3: Typecheck**

```powershell
npm run build
```

Expected: vue-tsc + vite build succeed (or only `npx vue-tsc --noEmit` if build too heavy — prefer `npm run build` per project norms).

- [ ] **Step 4: Commit**

```powershell
git add src/api/tauri.ts src/stores/workbench.ts
git commit -m "feat(kb): frontend API and store for folders and import"
```

---

### Task 7: NoteList folder tree UI

**Files:**
- Modify: `src/components/NoteList.vue`
- Modify: `src/index/index.vue` as needed for new emits (`import`, folder events) and ConfirmDialog host if confirm lives in parent

**Interfaces:**
- Extends emits: keep `select`/`create`/`delete`; add `import-request`; folder mutations via store directly inside NoteList is OK (like tags today use store)

UI requirements:
- Header: title + Import button + Create note
- Left or top tree: 「全部笔记」「未分类」+ recursive folders with expand; badge = notes_count or subtree_notes
- Selecting node filters list:
  - 全部: all notes
  - 未分类: `!folder_id`
  - folder id: `folder_id === id` (direct only for list; tree badge may show subtree — show `notes_count` for clarity)
- Context menu / buttons: 新建子文件夹、重命名、删除（ConfirmDialog copy from spec §7）、移动（simple prompt or nested picker — minimum: rename/create/delete in v1; move via optional parent select using `AppSelect` if quick)
- Drag note onto folder: `store.setNoteFolder(noteId, folderId)`
- Keep existing tag filter

Delete confirm message exactly:

> 将删除文件夹「{name}」。其下子文件夹会移到上一级；该文件夹内的 {n} 篇笔记将变为未分类。笔记不会被删除。

Use `notes_count` for `{n}`.

- [ ] **Step 1: Implement tree + filter in NoteList**
- [ ] **Step 2: Wire ConfirmDialog for folder delete**
- [ ] **Step 3: Visual check in browser `npm run build` types OK**
- [ ] **Step 4: Commit**

```powershell
git add src/components/NoteList.vue src/index/index.vue
git commit -m "feat(kb): NoteList folder tree with promote-delete confirm"
```

---

### Task 8: ImportDialog

**Files:**
- Create: `src/components/ImportDialog.vue`
- Modify: `src/components/NoteList.vue` or `index.vue` to open it

**Interfaces:**
- Props: `visible: boolean`
- Emits: `close`, `done(result: ImportResult)`
- On confirm: `open({ directory: true, multiple: false })` from `@tauri-apps/plugin-dialog` (also allow file filter for `.md` — if dialog cannot mix, prefer directory; single file: `open({ multiple: false, filters: [{ name: 'Markdown', extensions: ['md', 'markdown'] }] })` as secondary button 「导入文件」)

Flow: pick path → `store.importMarkdown(path)` → show summary → Toast via inject `showToast` → emit done → parent refreshes.

Guard `isTauri()`.

- [ ] **Step 1: Implement dialog**
- [ ] **Step 2: Wire from NoteList 导入 button**
- [ ] **Step 3: Commit**

```powershell
git add src/components/ImportDialog.vue src/components/NoteList.vue src/index/index.vue
git commit -m "feat(kb): Markdown import dialog for notes"
```

---

### Task 9: Optional editor folder label + open-note expand

**Files:**
- Modify: `src/components/NoteEditor.vue` — show folder path string if `folder_id` set (resolve from `store.state.folders`)
- Modify: `src/index/index.vue` / NoteList — when selecting note from search, set active folder filter to note’s folder or 「未分类」

- [ ] **Step 1: Small UI polish**
- [ ] **Step 2: Commit**

```powershell
git add src/components/NoteEditor.vue src/components/NoteList.vue src/index/index.vue
git commit -m "feat(kb): show note folder path and sync list filter on open"
```

---

### Task 10: Verification gate

**Files:** none

- [ ] **Step 1: Full Rust tests**

```powershell
npm run tauri:test
```

Expected: PASS

- [ ] **Step 2: Frontend build**

```powershell
npm run build
```

Expected: PASS

- [ ] **Step 3: Manual checklist (document results in commit message or leave for human)**

- [ ] Create nested folders, rename, move (no cycle)
- [ ] Delete folder: children promote; notes uncategorized; notes still openable
- [ ] Import a small md tree twice (second = updates)
- [ ] Tags + global search still work
- [ ] No sidebar 「知识库」; no Ollama required

- [ ] **Step 4: Push feature branch**

```powershell
git push origin feature/kb-notes-structure
```

---

## Spec coverage

| Spec item | Task |
|-----------|------|
| Schema note_folders + columns | Task 2 |
| Promote-on-delete | Task 3 |
| Anti-cycle move | Task 3 |
| Import + source_path | Tasks 4–5, 8 |
| Index stub hooks | Task 5 |
| No kb_chunks / KnowledgeView | Global + Tasks 2,7 |
| NoteList tree UI | Task 7 |
| Import dialog | Task 8 |
| Frontend API/store | Task 6 |
| Branch feature/kb-notes-structure | Task 1 |
| Acceptance / tests | Tasks 3–5, 10 |

## Out of scope (next plan)

- RAG, embeddings, KnowledgeView, kb_chunks, real `kb_index_note`
