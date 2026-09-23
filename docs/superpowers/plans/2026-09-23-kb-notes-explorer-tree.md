# Notes Explorer Tree UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace 速记 left pane with a single explorer-style tree (no「全部笔记」/「未分类」, no secondary note list); inline folder create/rename with sort; drag reorder; create-note can resolve folder by name.

**Architecture:** Keep existing `note_folders` + `notes.folder_id`. Add `reorder_note_folders` and optional `sort_order` on create/rename. Rebuild `NoteFolderTree` as the sole left navigator (folders + notes as rows). `NoteList` becomes a thin shell (header + tree + import); `NoteEditor` stays right pane.

**Tech Stack:** Tauri 2, Rusqlite, Vue 3, TypeScript, lucide-vue-next (`Check`, existing folder icons)

## Global Constraints

- Branch: `feature/kb-notes-structure` only (never product commits on `master`)
- Spec: `docs/superpowers/specs/2026-09-23-kb-notes-explorer-tree-design.md`
- Do **not** change import / promote-delete / `source_path` / index stub
- Do **not** restore HTML5 note→folder drag
- Pointer drag for folder reorder (same pattern as todo row drag); avoid HTML5 DnD for folders if it conflicts with Tauri `dragDropEnabled` — prefer pointer implementation like `useTodoDrag`
- Copy: user-facing「未分类」→「顶级」where it means `folder_id IS NULL`
- Windows Rust tests: `npm run tauri:test` (or embed manifest then `cargo test --lib -- <filter>`)
- Before starting UI tasks: commit or stash any unfinished review-fix WIP on the branch so the tree work has a clean baseline
- No new npm/Cargo dependencies

## File map

| Path | Responsibility |
|------|----------------|
| `src-tauri/src/repo/folder.rs` | `create` accept/set `sort_order`; `set_sort`; `reorder(ids)`; tests |
| `src-tauri/src/commands.rs` | `reorder_note_folders`; extend create/rename args if needed |
| `src-tauri/src/lib.rs` | Register `reorder_note_folders` |
| `src/api/tauri.ts` | `reorderNoteFolders`; create/rename optional `sortOrder` |
| `src/stores/workbench.ts` | `reorderFolders`; createFolder/renameFolder pass sort; resolve folder by name helper |
| `src/composables/useNoteFolders.ts` | Keep children map; add root-folder sort helpers if useful |
| `src/components/NoteFolderTree.vue` | Explorer tree: folders + nested notes + top-level notes; inline edit; pointer reorder |
| `src/components/NoteList.vue` | Header (import/create) + tags + `NoteFolderTree` only; emit select/create/delete/import |
| `src/components/NoteEditor.vue` | Path empty for top-level; no「未分类」label |
| `src/index/index.vue` | Create-note flow with optional folder name; empty editor copy |

---

### Task 1: Folder sort_order write API (Rust)

**Files:**
- Modify: `src-tauri/src/repo/folder.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/repo/folder.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: existing `create` / `rename` / `list`
- Produces:
  - `pub fn create(conn, parent_id, name, sort_order: Option<i64>) -> Result<NoteFolder>` — if `None`, use `max(sort_order)+1` among siblings (root: `parent_id IS NULL`)
  - `pub fn reorder(conn, ids: &[i64]) -> Result<()>` — set `sort_order = index+1` for each id in order; all must be same parent (this UX only reorders **root** folders: reject if any `parent_id` is Some)
  - Command: `reorder_note_folders(ids: Vec<i64>) -> Result<()>`
  - Keep `create_note_folder(parent_id, name, sort_order: Option<i64>)` — serde Option defaults None

- [ ] **Step 1: Write failing tests in `folder.rs`**

```rust
#[test]
fn create_assigns_incrementing_sort_among_siblings() {
    let conn = init_in_memory().unwrap();
    let a = create(&conn, None, "A", None).unwrap();
    let b = create(&conn, None, "B", None).unwrap();
    assert_eq!(a.sort_order, 1);
    assert_eq!(b.sort_order, 2);
}

#[test]
fn reorder_root_folders_rewrites_sort_order() {
    let conn = init_in_memory().unwrap();
    let a = create(&conn, None, "A", None).unwrap();
    let b = create(&conn, None, "B", None).unwrap();
    let c = create(&conn, None, "C", None).unwrap();
    reorder(&conn, &[c.id, a.id, b.id]).unwrap();
    let list = list(&conn).unwrap();
    let by_id: std::collections::HashMap<_, _> = list.iter().map(|f| (f.id, f.sort_order)).collect();
    assert_eq!(by_id[&c.id], 1);
    assert_eq!(by_id[&a.id], 2);
    assert_eq!(by_id[&b.id], 3);
}

#[test]
fn reorder_rejects_non_root_mix() {
    let conn = init_in_memory().unwrap();
    let root = create(&conn, None, "R", None).unwrap();
    let child = create(&conn, Some(root.id), "C", None).unwrap();
    let err = reorder(&conn, &[root.id, child.id]).unwrap_err();
    assert!(err.to_string().contains("INVALID") || err.to_string().contains("根"));
}
```

- [ ] **Step 2: Run tests — expect FAIL (signatures / fn missing)**

```powershell
cd src-tauri
# After mt.exe embed if needed on Windows:
cargo test --lib -- repo::folder::tests::create_assigns -- --nocapture
```

Expected: compile error or test fail on missing `reorder` / wrong `create` arity.

- [ ] **Step 3: Implement `next_sort`, update `create`, add `reorder`**

```rust
fn next_sibling_sort(conn: &Connection, parent_id: Option<i64>) -> Result<i64> {
    let max: Option<i64> = match parent_id {
        Some(pid) => conn.query_row(
            "SELECT MAX(sort_order) FROM note_folders WHERE parent_id = ?1",
            params![pid],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT MAX(sort_order) FROM note_folders WHERE parent_id IS NULL",
            [],
            |r| r.get(0),
        )?,
    };
    Ok(max.unwrap_or(0) + 1)
}

pub fn create(
    conn: &Connection,
    parent_id: Option<i64>,
    name: &str,
    sort_order: Option<i64>,
) -> Result<NoteFolder> {
    // ... existing validation ...
    let sort = sort_order.unwrap_or(next_sibling_sort(conn, parent_id)?);
    conn.execute(
        "INSERT INTO note_folders (parent_id, name, sort_order, created_at, updated_at) VALUES (?1,?2,?3,?4,?4)",
        params![parent_id, name.trim(), sort, now()],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn reorder(conn: &Connection, ids: &[i64]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    for &id in ids {
        let f = get(conn, id)?;
        if f.parent_id.is_some() {
            return Err(rusqlite::Error::InvalidParameterName(
                "INVALID_ARGUMENT: 本版本仅支持根级文件夹排序".into(),
            ));
        }
    }
    for (i, &id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE note_folders SET sort_order = ?1, updated_at = ?2 WHERE id = ?3",
            params![(i as i64) + 1, now(), id],
        )?;
    }
    Ok(())
}
```

Update all in-crate `create(&conn, …)` call sites to pass `None` for sort (knowledge import `find_or_create_path`, tests).

- [ ] **Step 4: Wire command + lib**

```rust
#[tauri::command]
pub fn reorder_note_folders(state: State<'_, DbState>, ids: Vec<i64>) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    folder::reorder(&conn, &ids).map_err(err_str)?;
    log::info!("文件夹重排: {:?}", ids);
    Ok(())
}

#[tauri::command]
pub fn create_note_folder(
    state: State<'_, DbState>,
    parent_id: Option<i64>,
    name: String,
    sort_order: Option<i64>,
) -> Result<NoteFolder, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let folder = folder::create(&conn, parent_id, &name, sort_order).map_err(err_str)?;
    Ok(folder)
}
```

Register `reorder_note_folders` in `lib.rs` next to other folder commands.

- [ ] **Step 5: Run tests — expect PASS**

```powershell
cargo test --lib -- repo::folder::tests -- --test-threads=1
```

Expected: all folder tests ok (including old promote/cycle + new three).

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/repo/folder.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(kb): folder sort_order on create and root reorder command"
```

---

### Task 2: Frontend API + store for reorder / sort

**Files:**
- Modify: `src/api/tauri.ts`
- Modify: `src/stores/workbench.ts`

**Interfaces:**
- Consumes: Task 1 commands
- Produces:
  - `tauriApi.reorderNoteFolders(ids: number[]): Promise<void>`
  - `tauriApi.createNoteFolder(parentId, name, sortOrder?: number | null)`
  - `store.reorderFolders(ids: number[])`
  - `store.resolveOrCreateRootFolder(name: string): Promise<number>` — trim; find root folder by name; else create at root with default sort

- [ ] **Step 1: Update `tauri.ts`**

```ts
createNoteFolder: (parentId: number | null, name: string, sortOrder?: number | null) =>
  invoke<NoteFolder>('create_note_folder', {
    parentId,
    name,
    sortOrder: sortOrder ?? null,
  }),
reorderNoteFolders: (ids: number[]) =>
  invoke<void>('reorder_note_folders', { ids }),
```

- [ ] **Step 2: Store helpers**

```ts
async function reorderFolders(ids: number[]) {
  if (!isTauri()) {
    // preview: permute local sort_order
    ids.forEach((id, i) => {
      const f = state.folders.find((x) => x.id === id)
      if (f) f.sort_order = i + 1
    })
    return
  }
  await tauriApi.reorderNoteFolders(ids)
  await refreshFolders()
}

async function resolveOrCreateRootFolder(name: string): Promise<number> {
  const trimmed = name.trim()
  if (!trimmed) throw new Error('文件夹名称不能为空')
  const existing = state.folders.find((f) => f.parent_id == null && f.name === trimmed)
  if (existing) return existing.id
  const folder = await createFolder(null, trimmed) // uses default sort
  return folder.id
}
```

Export both from store return object. Update `createFolder` to pass optional `sortOrder` through to API.

- [ ] **Step 3: Typecheck**

```powershell
npx vue-tsc --noEmit
```

Expected: exit 0 (or only pre-existing unrelated errors).

- [ ] **Step 4: Commit**

```powershell
git add src/api/tauri.ts src/stores/workbench.ts
git commit -m "feat(kb): frontend reorder folders and resolve folder by name"
```

---

### Task 3: Explorer `NoteFolderTree` (single tree, inline edit, pointer reorder)

**Files:**
- Modify: `src/components/NoteFolderTree.vue` (major rewrite)
- Modify: `src/composables/useNoteFolders.ts` if needed

**Interfaces:**
- Consumes: `store.state.folders`, `props.notes`, `store.createFolder` / `renameFolder` / `reorderFolders` / `deleteFolder` / `moveFolder`
- Produces emits:
  - `select-note(id: number)`
  - `select-folder(id: number | null)` — null when clearing
  - (keep delete/move confirm internal)

**Behavior (must match spec):**
1. No「全部笔记」/「未分类」rows
2. Rows: root folders by `sort_order` → if expanded, direct notes (`folder_id === f.id`) indented → then all notes with `folder_id == null`
3. Click folder chevron or name: toggle expand; do **not** emit note list elsewhere
4. Double-click folder name: inline edit row (name + number sort + Check)
5. FolderPlus: insert create edit row at **top** of tree body; default sort = root folder count + 1; focus input
6. Pointer-drag folder rows among **root** folders only → `reorderFolders(newIdOrder)`
7. Click note row → `emit('select-note', id)`
8. Right-click folder: existing menu (create child / rename / move / delete) but rename uses inline, not `prompt`
9. Right-click note: emit or handle move via parent — prefer emit `note-context` or keep ContextMenu in tree with「移到顶级」

- [ ] **Step 1: Replace props/emits**

```ts
const props = defineProps<{
  notes: readonly Note[]
  activeNoteId: number | null
  selectedFolderId?: number | null
}>()
const emit = defineEmits<{
  (e: 'select-note', id: number): void
  (e: 'select-folder', id: number | null): void
}>()
```

Remove `modelValue` / `FolderFilter` / `expandForFolderId` — expand parent chain via `watch(() => props.activeNoteId)`.

- [ ] **Step 2: Build `treeRows` computed**

Pseudo:

```ts
type Row =
  | { kind: 'folder'; folder: NoteFolder; depth: number; hasChildren: boolean }
  | { kind: 'note'; note: Note; depth: number }

const treeRows = computed(() => {
  const rows: Row[] = []
  const roots = [...(childrenOf.value.get(null) ?? [])]
  for (const f of roots) {
    rows.push({ kind: 'folder', folder: f, depth: 0, hasChildren: true /* notes or child folders */ })
    if (expanded.value.has(f.id)) {
      // optional: nested child folders (keep recurse for multi-level)
      // then direct notes:
      for (const n of props.notes.filter((x) => x.folder_id === f.id)
        .sort((a, b) => /* updated_at desc */)) {
        rows.push({ kind: 'note', note: n, depth: 1 })
      }
    }
  }
  for (const n of props.notes.filter((x) => x.folder_id == null)
    .sort(/* updated_at desc */)) {
    rows.push({ kind: 'note', note: n, depth: 0 })
  }
  return rows
})
```

Include nested folders recursively under expanded parents (existing product supports multi-level); **root reorder only** for drag.

- [ ] **Step 3: Inline editor component (in-file)**

Template fragment when `editing`:

```vue
<div class="edit-row">
  <input ref="editInput" v-model="editName" @keydown.enter="commitEdit" @keydown.escape="cancelEdit" />
  <div class="sort-box">
    <input type="number" v-model.number="editSort" min="1" />
    <button type="button" class="sort-step" @mousedown.prevent="bumpSort(1)">▲</button>
    <!-- or single button split hit-test like demo -->
  </div>
  <button type="button" class="btn-ok" @click="commitEdit"><Check :size="14" /></button>
</div>
```

`commitEdit`: create → `store.createFolder(null, name, editSort)`; rename → `renameFolder` + if sort changed call reorder or a `set_sort` path — simplest: after rename, if root, build full root id list with this folder moved to `editSort` position then `reorderFolders`.

- [ ] **Step 4: Pointer reorder (root only)**

Follow `composables/useTodoDrag.ts` pattern: pointerdown on folder row (not on chevron), threshold, show drop indicator, on up compute new root id order → `store.reorderFolders(ids)`.

Do **not** use HTML5 `draggable` if it fights native file drop.

- [ ] **Step 5: Manual smoke in `pnpm run tauri:dev` / browser preview**

Checklist: no virtual nodes; expand shows notes under folder only in tree; top notes at end; FolderPlus inline; dblclick rename; drag reorder.

- [ ] **Step 6: Commit**

```powershell
git add src/components/NoteFolderTree.vue src/composables/useNoteFolders.ts
git commit -m "feat(kb): explorer folder tree with inline edit and reorder"
```

---

### Task 4: Slim `NoteList` + wire `index.vue` create-with-folder-name

**Files:**
- Modify: `src/components/NoteList.vue`
- Modify: `src/index/index.vue`
- Modify: `src/components/NoteEditor.vue` (copy only)

**Interfaces:**
- Consumes: Task 3 tree emits
- Produces: same parent events `@select` `@create` `@delete` `@import-request` but create payload becomes `{ folderId: number | null, folderName?: string }` **or** keep `folderId` and handle name dialog in NoteList before emit

**Recommended create UX (minimal):**

When user clicks「＋」/「新建笔记」:

1. If a folder is selected in tree → `emit('create', selectedFolderId)`
2. Else show a small inline bar under header (not modal): title optional skip; **文件夹** text input (placeholder「留空=顶级」) + 确认
3. On confirm: if name empty → `emit('create', null)`; else parent calls `resolveOrCreateRootFolder` then `addNote(title, id)`

Simpler alternative matching spec: always `emit('create', folderId | null)` from selected folder, and add optional folder-name field only when no folder selected — implement in NoteList then:

```ts
async function confirmCreate() {
  const name = folderNameInput.value.trim()
  if (!name) {
    emit('create', selectedFolderId.value)
    return
  }
  try {
    const id = await store.resolveOrCreateRootFolder(name)
    emit('create', id)
  } catch (e) {
    showToast(String(e))
  }
}
```

And change `onCreateNote` in index to only `addNote('无标题笔记', folderId)` (already supports folderId).

- [ ] **Step 1: Strip `NoteList` of `nl-list` / `sortedNotes` / folder filter state**

Template left column = header + optional tag filter + `<NoteFolderTree ... @select-note="emit('select', id)" />` filling remaining height. Empty editor state stays in `NoteEditor` / index, not under tree.

- [ ] **Step 2: Note context menu「移到顶级」**

Replace label `移到未分类` → `移到顶级` (`setNoteFolder(id, null)`).

- [ ] **Step 3: `NoteEditor` path**

Keep empty string when `folder_id == null` (already). Grep for `未分类` in note UI and replace user-visible strings.

- [ ] **Step 4: Delete-folder confirm copy**

「将变为未分类」→「将变为顶级笔记」in `NoteFolderTree` confirm message.

- [ ] **Step 5: `vue-tsc` + visual check**

```powershell
npx vue-tsc --noEmit
npm run build
```

Expected: build ok.

- [ ] **Step 6: Commit**

```powershell
git add src/components/NoteList.vue src/index/index.vue src/components/NoteEditor.vue src/components/NoteFolderTree.vue
git commit -m "feat(kb): single-pane notes explorer layout and create-into-folder-by-name"
```

---

### Task 5: Acceptance pass

**Files:** none (manual + optional small test)

- [ ] **Step 1: Run folder unit tests again**

```powershell
# Windows: use scripts/cargo-test.ps1 or mt-embed workflow
cargo test --lib -- repo::folder::tests -- --test-threads=1
```

Expected: PASS

- [ ] **Step 2: GUI checklist (from spec §5)**

- [ ] No「全部笔记」/「未分类」on tree  
- [ ] Expand folder → notes only indented in tree; no lower duplicate list  
- [ ] Top-level notes after all folders  
- [ ] FolderPlus → top inline → √ creates; duplicate name rejected  
- [ ] Double-click rename; Esc cancels  
- [ ] Drag reorder persists after refreshFolders  
- [ ] New note default top-level; with folder name creates/joins folder  
- [ ] Tags filter still works if present  

- [ ] **Step 3: Commit any copy/fix leftovers**

```powershell
git commit -m "fix(kb): explorer tree acceptance polish"
```

---

## Spec coverage (self-review)

| Spec item | Task |
|-----------|------|
| Remove 全部/未分类 | T3 |
| Remove secondary list | T4 |
| Explorer order folders→notes→toplevel | T3 |
| Click expand / dblclick rename | T3 |
| FolderPlus inline + sort default n+1 + √ | T3 |
| Drag reorder | T1+T2+T3 |
| Create note folder-by-name | T2+T4 |
| Copy 未分类→顶级 | T4 |
| No import/delete/DnD note changes | Global constraints |

## Placeholder scan

No TBD steps; commands and signatures named above.

## Type consistency

- `create(..., sort_order: Option<i64>)` / frontend `sortOrder`
- `reorder_note_folders(ids)` / `reorderNoteFolders(ids)`
- `resolveOrCreateRootFolder(name) -> number`
- Tree emits `select-note` / `select-folder` only
