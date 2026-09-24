# Notes Tree Inline Rename Note Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users rename notes inline in the 速记 folder tree (double-click + context menu), title-only, without wiping note content.

**Architecture:** Add `note::rename` (UPDATE title + updated_at only) → `rename_note` command → `tauriApi.renameNote` → `store.renameNote` (normalize empty →「无标题笔记」). Extend `NoteFolderTree` `EditState` with `rename-note` (name input, no sort). Sync `NoteEditor.localTitle` when `props.note.title` changes and the title input is not focused.

**Tech Stack:** Rust (`rusqlite`), Tauri 2 commands, Vue 3 + existing store pattern

## Global Constraints

- Branch: `feature/kb-notes-structure` only
- Spec: `docs/superpowers/specs/2026-09-24-kb-notes-tree-rename-note-design.md`
- Empty title →「无标题笔记」(same string as `NoteEditor.normalizeTitle`)
- Must **not** call `update_note` / `saveNote` for tree rename (content wipe risk after `list_meta`)
- No sort stepper on note rename UI
- No F2 shortcut; no extension bridge capability in this plan
- Windows tests: `npm run tauri:test -- --lib note::` (or filter `rename_note`); never hang `cargo test` without the wrapper
- No new dependencies

## File map

| Path | Responsibility |
|------|----------------|
| `src-tauri/src/repo/note.rs` | `rename` + unit tests |
| `src-tauri/src/commands.rs` | `rename_note` command |
| `src-tauri/src/lib.rs` | Register command next to `update_note` |
| `src/api/tauri.ts` | `renameNote` invoke wrapper |
| `src/stores/workbench.ts` | `renameNote` store method + export |
| `src/components/NoteFolderTree.vue` | Inline edit UI + dblclick + context menu |
| `src/components/NoteEditor.vue` | Watch `props.note.title` → sync `localTitle` when unfocused |

---

### Task 1: Backend `note::rename` + command + API wrapper

**Files:**
- Modify: `src-tauri/src/repo/note.rs` (add `rename` after `update`, add tests near `update_note_title_and_content`)
- Modify: `src-tauri/src/commands.rs` (add `rename_note` after `update_note`)
- Modify: `src-tauri/src/lib.rs` (register `commands::rename_note` after `commands::update_note`)
- Modify: `src/api/tauri.ts` (add `renameNote` next to `updateNote`)

**Interfaces:**
- Consumes: existing `get`, `update`, `NOT_FOUND` error style
- Produces:
  - `pub fn rename(conn: &Connection, id: i64, title: &str) -> Result<Note>`
  - `#[tauri::command] pub fn rename_note(state, id: i64, title: String) -> Result<Note, String>`
  - `tauriApi.renameNote: (id: number, title: string) => invoke<Note>('rename_note', { id, title })`

- [ ] **Step 1: Write the failing tests**

In `src-tauri/src/repo/note.rs` `mod tests`, add:

```rust
#[test]
fn rename_note_changes_title_keeps_content() {
    let conn = init_in_memory().unwrap();
    let n = create(&conn, "T").unwrap();
    update(&conn, n.id, "T", "正文保留").unwrap();
    let renamed = rename(&conn, n.id, "新标题").unwrap();
    assert_eq!(renamed.title, "新标题");
    assert_eq!(renamed.content, "正文保留");
    let again = get(&conn, n.id).unwrap();
    assert_eq!(again.content, "正文保留");
}

#[test]
fn rename_note_empty_becomes_default_title() {
    let conn = init_in_memory().unwrap();
    let n = create(&conn, "T").unwrap();
    update(&conn, n.id, "T", "x").unwrap();
    let renamed = rename(&conn, n.id, "   ").unwrap();
    assert_eq!(renamed.title, "无标题笔记");
    assert_eq!(renamed.content, "x");
}

#[test]
fn rename_note_missing_is_not_found() {
    let conn = init_in_memory().unwrap();
    let err = rename(&conn, 999_999, "x").unwrap_err();
    assert!(
        err.to_string().contains("NOT_FOUND"),
        "expected NOT_FOUND, got: {err}"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

```powershell
npm run tauri:test -- --lib note::tests::rename_note_changes_title_keeps_content
```

Expected: compile fail or link fail because `rename` is undefined / tests don't compile.

- [ ] **Step 3: Implement `rename`**

Insert after `update` in `note.rs`:

```rust
/// 只改标题，不改 content（树内联重命名；避免 list_meta 空正文经 update 误存）。
pub fn rename(conn: &Connection, id: i64, title: &str) -> Result<Note> {
    let title = {
        let t = title.trim();
        if t.is_empty() {
            "无标题笔记"
        } else {
            t
        }
    };
    let affected = conn.execute(
        "UPDATE notes SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, now(), id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    get(conn, id)
}
```

- [ ] **Step 4: Wire command + registration + TS wrapper**

In `commands.rs` after `update_note`:

```rust
#[tauri::command]
pub fn rename_note(
    state: State<'_, DbState>,
    id: i64,
    title: String,
) -> Result<Note, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let note = note::rename(&conn, id, &title).map_err(err_str)?;
    log::info!("重命名笔记: id={} -> {}", id, note.title);
    drop(conn);
    crate::kb_hooks::on_notes_changed(&[id]);
    Ok(note)
}
```

In `lib.rs` `invoke_handler`, immediately after `commands::update_note,` add:

```rust
commands::rename_note,
```

In `src/api/tauri.ts` next to `updateNote`:

```ts
renameNote: (id: number, title: string) =>
  invoke<Note>('rename_note', { id, title }),
```

- [ ] **Step 5: Run tests to verify they pass**

```powershell
npm run tauri:test -- --lib note::tests::rename_note
```

Expected: the three new tests PASS.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/repo/note.rs src-tauri/src/commands.rs src-tauri/src/lib.rs src/api/tauri.ts
git commit -m "feat(kb): rename_note title-only command"
```

---

### Task 2: Store `renameNote`

**Files:**
- Modify: `src/stores/workbench.ts` (add `renameNote` near `saveNote` / `renameFolder`; export in return object)

**Interfaces:**
- Consumes: `tauriApi.renameNote`, `isTauri()`, `state.notes`
- Produces: `async function renameNote(id: number, title: string): Promise<Note>` — normalizes empty/whitespace to「无标题笔记」before invoke; merges returned note into `state.notes` while **preserving existing `content`** if the API return has empty content (defensive; `get` after rename still returns full content, but keep merge safe)

- [ ] **Step 1: Add store method**

Near `saveNote`, add:

```ts
function normalizeNoteTitle(title: string): string {
  const t = title.trim()
  return t ? t : '无标题笔记'
}

async function renameNote(id: number, title: string) {
  const nextTitle = normalizeNoteTitle(title)
  if (!isTauri()) {
    const i = state.notes.findIndex((x) => x.id === id)
    if (i < 0) throw new Error('笔记不存在')
    const prev = state.notes[i]!
    const n: Note = {
      ...prev,
      title: nextTitle,
      updated_at: new Date().toISOString(),
    }
    state.notes[i] = n
    return n
  }
  const n = await tauriApi.renameNote(id, nextTitle)
  const i = state.notes.findIndex((x) => x.id === id)
  if (i >= 0) {
    const prev = state.notes[i]!
    // 防 list_meta / 空 content 回写冲掉本地已有正文
    state.notes[i] = {
      ...n,
      content: n.content || prev.content,
    }
  }
  return n
}
```

Export `renameNote` in the store return object next to `saveNote` / `renameFolder`.

- [ ] **Step 2: Typecheck (optional quick)**

```powershell
npx vue-tsc --noEmit -p tsconfig.json 2>&1 | Select-String -Pattern "renameNote|workbench"
```

Expected: no errors mentioning `renameNote` / `workbench`.

- [ ] **Step 3: Commit**

```powershell
git add src/stores/workbench.ts
git commit -m "feat(kb): store renameNote for title-only rename"
```

---

### Task 3: `NoteFolderTree` inline rename UI

**Files:**
- Modify: `src/components/NoteFolderTree.vue`

**Interfaces:**
- Consumes: `store.renameNote` from Task 2; existing `editing` / `editName` / `commitEdit` / `cancelEdit` / `focusEditInput`
- Produces: `EditState` includes `{ mode: 'rename-note'; note: Note }`; dblclick + context「重命名」

- [ ] **Step 1: Extend edit state + helpers**

Change:

```ts
type EditState =
  | { mode: 'create'; parentId: number | null }
  | { mode: 'rename'; folder: NoteFolder }
  | { mode: 'rename-note'; note: Note }
```

Add:

```ts
function startRenameNote(note: Note) {
  editing.value = { mode: 'rename-note', note }
  editName.value = note.title
  void focusEditInput()
}

function onNoteDblClick(note: Note, e: MouseEvent) {
  e.preventDefault()
  e.stopPropagation()
  emit('select-note', note.id)
  if (note.folder_id != null) emit('select-folder', note.folder_id)
  else emit('select-folder', null)
  startRenameNote(note)
}

function showRenameNoteEdit(noteId: number): boolean {
  return editing.value?.mode === 'rename-note' && editing.value.note.id === noteId
}
```

- [ ] **Step 2: Extend `commitEdit`**

Replace the body of `commitEdit` so note rename is handled (keep create/folder rename as today). Concrete shape:

```ts
async function commitEdit() {
  const state = editing.value
  if (!state) return

  if (state.mode === 'rename-note') {
    const raw = editName.value.trim()
    const name = raw || '无标题笔记'
    if (name === state.note.title) {
      cancelEdit()
      return
    }
    try {
      await store.renameNote(state.note.id, name)
      cancelEdit()
    } catch (err) {
      const msg = String(err)
      showToast(msg)
      if (msg.includes('NOT_FOUND')) cancelEdit()
      // else stay in edit mode
    }
    return
  }

  const name = editName.value.trim()
  if (!name) {
    showToast('文件夹名称不能为空')
    return
  }
  try {
    if (state.mode === 'create') {
      const folder = await store.createFolder(state.parentId, name, editSort.value)
      await applySiblingSort(state.parentId, folder.id, editSort.value)
      emit('select-folder', folder.id)
    } else {
      const folder = state.folder
      if (name !== folder.name) {
        await store.renameFolder(folder.id, name)
      }
      if (editSort.value !== folder.sort_order) {
        await applySiblingSort(folder.parent_id, folder.id, editSort.value)
      }
    }
    cancelEdit()
  } catch (err) {
    showToast(String(err))
  }
}
```

- [ ] **Step 3: Context menu**

In `onNoteContext`, put rename first:

```ts
const items: ContextMenuItem[] = [
  { label: '重命名', onClick: () => startRenameNote(note) },
  {
    label: '移到顶级',
    onClick: () => void moveNoteToFolder(note.id, null),
  },
  // ... existing move items unchanged ...
]
```

- [ ] **Step 4: Template — note row edit + dblclick**

Where `row.kind === 'note'` currently renders a single `.tree-row`, wrap like folders:

```vue
<div
  v-else-if="row.kind === 'note' && showRenameNoteEdit(row.note.id)"
  class="edit-row"
  :style="{ paddingLeft: `${8 + row.depth * 12}px` }"
  data-no-drag
>
  <input
    :ref="bindEditInput"
    v-model="editName"
    type="text"
    class="edit-name"
    maxlength="80"
    placeholder="笔记标题"
    @keydown.enter.prevent="commitEdit"
    @keydown.escape.prevent="cancelEdit"
  />
  <button type="button" class="btn-ok" title="保存" @click="commitEdit">
    <Check :size="14" :stroke-width="2.5" />
  </button>
</div>

<div
  v-else-if="row.kind === 'note'"
  class="tree-row tree-row--note"
  ...existing attrs...
  @dblclick="onNoteDblClick(row.note, $event)"
>
  ...
</div>
```

**Do not** render `.sort-box` on the note rename row.

- [ ] **Step 5: Manual smoke (dev server if already running)**

速记视图：双击笔记 → 无序号框 → 改名保存；右键「重命名」；清空标题 →「无标题笔记」；拖拽松手不误进重命名。

- [ ] **Step 6: Commit**

```powershell
git add src/components/NoteFolderTree.vue
git commit -m "feat(kb): inline rename notes in folder tree"
```

---

### Task 4: `NoteEditor` title sync from store

**Files:**
- Modify: `src/components/NoteEditor.vue`

**Interfaces:**
- Consumes: `props.note?.title`, `localTitle`, `.ed-title-input`
- Produces: when external title changes and title input is not focused, `localTitle` updates; content/`dirty` unchanged

- [ ] **Step 1: Add title watch**

After `syncLocal` / near other watches on `props.note`, add:

```ts
watch(
  () => props.note?.title,
  (title) => {
    if (title == null) return
    if (title === localTitle.value) return
    const active = document.activeElement
    const titleInput = active instanceof HTMLInputElement && active.classList.contains('ed-title-input')
    if (titleInput) return
    localTitle.value = title
  },
)
```

Do **not** set `dirty` from this watch. Do **not** call `scheduleSave`.

- [ ] **Step 2: Manual check**

Open a note → focus body (not title) → tree-rename that note → editor title updates. Focus title input and type → tree rename (via another path) should not clobber mid-edit (skip if hard to trigger; at least unfocused path works).

- [ ] **Step 3: Commit**

```powershell
git add src/components/NoteEditor.vue
git commit -m "fix(kb): sync editor title when note renamed from tree"
```

---

## Spec coverage

| Spec item | Task |
|-----------|------|
| Double-click inline rename (no sort) | T3 |
| Context menu「重命名」 | T3 |
| Empty →「无标题笔记」 | T1 (Rust) + T2 (store) + T3 commit |
| Title-only API / no content wipe | T1 |
| Editor title sync when unfocused | T4 |
| Unchanged title = no API | T3 `commitEdit` |
| Error: toast; NOT_FOUND cancels edit | T3 |
| No F2 / no update_note semantic change | Global |

## Placeholder scan

None — all steps have concrete code/commands.

## Type consistency

- Command name: `rename_note` / `tauriApi.renameNote` / `store.renameNote`
- Edit mode: `'rename-note'` (string literal) throughout Task 3
- Default title string: `无标题笔记` identical in Rust, store, and UI commit path
