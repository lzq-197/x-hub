# Notes Tree Focus + Nest-on-Drop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix notes-tree selection/create focus and sort steppers; add pointer drag so folders and notes nest into a target folder or reorder among siblings; persist isolated `sort_order` for notes vs folders.

**Architecture:** Extend existing rusqlite folder/note repos and the pointer-drag path in `NoteFolderTree.vue` (no HTML5 DnD). Folders and notes keep separate `sort_order` namespaces; tree always renders child folders then notes. Selection state stays in `NoteList` (`selectedFolderId`); FolderPlus and blank-click drive create parent.

**Tech Stack:** Tauri 2, Rusqlite, Vue 3, TypeScript, lucide-vue-next

## Global Constraints

- Branch: `feature/kb-notes-structure` only
- Spec: `docs/superpowers/specs/2026-09-24-kb-notes-tree-focus-dnd-design.md`
- Pointer drag only — never HTML5 DnD (Tauri `dragDropEnabled`)
- Do not change import / promote-delete / `source_path` / `list_meta` content wipe
- Windows Rust tests: embed manifest then run (`scripts/cargo-test.ps1` or equivalent); filter `repo::folder::tests` / `repo::note::tests`
- No new npm/Cargo dependencies
- User-facing copy: keep「顶级」; no「未分类」

## File map

| Path | Responsibility |
|------|----------------|
| `src-tauri/src/db.rs` | Migrate `notes.sort_order`; backfill per folder |
| `src-tauri/src/models.rs` | `Note.sort_order` |
| `src-tauri/src/repo/note.rs` | Assign sort on create/set_folder; `reorder`; list still OK globally |
| `src-tauri/src/repo/folder.rs` | Same-parent `reorder`; after `move_folder` append folder sort |
| `src-tauri/src/commands.rs` | `reorder_notes`; keep `reorder_note_folders` |
| `src-tauri/src/lib.rs` | Register `reorder_notes` |
| `src/api/tauri.ts` | `Note.sort_order`; `reorderNotes` |
| `src/stores/workbench.ts` | `reorderNotes`; create/move refresh |
| `src/components/NoteFolderTree.vue` | Blank clear; FolderPlus parent; ▲▼; full pointer DnD |
| `src/components/NoteList.vue` | Wire blank clear if needed; create still uses `selectedFolderId` |
| Optional: `src/utils/noteTreeHit.ts` | Pure hit-test helpers (row mid vs gap) for unit-testability |

---

### Task 1: Notes `sort_order` column + repo

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/repo/note.rs`
- Test: `src-tauri/src/repo/note.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: existing `create_with_folder` / `set_folder` / `list`
- Produces:
  - `Note { …, sort_order: i64 }`
  - `note::reorder(conn, ids: &[i64]) -> Result<()>` — all ids same `folder_id` (incl. all NULL)
  - create / set_folder assign `max(sort)+1` in target group

- [ ] **Step 1: Write failing tests**

In `repo/note.rs` tests:

```rust
#[test]
fn create_assigns_incrementing_sort_in_folder() {
    let conn = init_in_memory().unwrap();
    let f = folder::create(&conn, None, "W", None).unwrap();
    let a = create_with_folder(&conn, "a", Some(f.id)).unwrap();
    let b = create_with_folder(&conn, "b", Some(f.id)).unwrap();
    assert_eq!(a.sort_order, 1);
    assert_eq!(b.sort_order, 2);
}

#[test]
fn reorder_notes_same_folder_rewrites_sort() {
    let conn = init_in_memory().unwrap();
    let f = folder::create(&conn, None, "W", None).unwrap();
    let a = create_with_folder(&conn, "a", Some(f.id)).unwrap();
    let b = create_with_folder(&conn, "b", Some(f.id)).unwrap();
    reorder(&conn, &[b.id, a.id]).unwrap();
    assert_eq!(get(&conn, b.id).unwrap().sort_order, 1);
    assert_eq!(get(&conn, a.id).unwrap().sort_order, 2);
}

#[test]
fn reorder_notes_rejects_mixed_folders() {
    let conn = init_in_memory().unwrap();
    let f1 = folder::create(&conn, None, "A", None).unwrap();
    let f2 = folder::create(&conn, None, "B", None).unwrap();
    let a = create_with_folder(&conn, "a", Some(f1.id)).unwrap();
    let b = create_with_folder(&conn, "b", Some(f2.id)).unwrap();
    let err = reorder(&conn, &[a.id, b.id]).unwrap_err();
    assert!(err.to_string().contains("INVALID") || err.to_string().contains("同"));
}
```

- [ ] **Step 2: Run tests — expect FAIL**

```powershell
# from repo root; embed manifest then:
cargo test --manifest-path src-tauri/Cargo.toml --lib -- repo::note::tests::create_assigns -- --test-threads=1
```

Expected: compile error or test fail (`sort_order` missing / `reorder` missing).

- [ ] **Step 3: Migrate + model**

`db.rs` (after existing notes folder columns block): if `notes` lacks `sort_order`, `ALTER TABLE notes ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0`, then backfill:

```sql
-- per distinct folder_id (use IFNULL(folder_id,-1)), order by updated_at DESC, id DESC → assign 1..n
```

Implement backfill in Rust loops (clearer than pure SQL window on SQLite version variance).

`models.rs`: add `#[serde(default)] pub sort_order: i64` on `Note`.

- [ ] **Step 4: Implement note repo**

- Extend all `SELECT` / `row_to_note` with `sort_order`.
- `next_note_sort(conn, folder_id)` → `MAX+1`.
- `create_with_folder` / `create_imported` INSERT includes `sort_order`.
- `set_folder`: update `folder_id` **and** set `sort_order = next` in new group.
- `reorder(ids)`: empty OK; load each note; require identical `folder_id`; write `sort_order = i+1`.

Keep global `list` / `list_meta` order as `updated_at DESC` for overview/search; **tree UI** sorts by `sort_order` client-side (Task 5).

- [ ] **Step 5: Run tests — expect PASS**

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/db.rs src-tauri/src/models.rs src-tauri/src/repo/note.rs
git commit -m "feat(kb): notes.sort_order with same-folder reorder"
```

---

### Task 2: Folder reorder any same-parent + move appends sort

**Files:**
- Modify: `src-tauri/src/repo/folder.rs`
- Test: same file tests

**Interfaces:**
- Consumes: Task 1 unchanged
- Produces:
  - `folder::reorder(conn, ids)` — all same `parent_id` (root or nested); error message no longer「仅支持根级」
  - `move_folder` after successful parent change sets `sort_order = next_sibling_sort` under new parent

- [ ] **Step 1: Replace/extend failing tests**

```rust
#[test]
fn reorder_nested_siblings() {
    let conn = init_in_memory().unwrap();
    let root = create(&conn, None, "R", None).unwrap();
    let a = create(&conn, Some(root.id), "A", None).unwrap();
    let b = create(&conn, Some(root.id), "B", None).unwrap();
    reorder(&conn, &[b.id, a.id]).unwrap();
    assert_eq!(get(&conn, b.id).unwrap().sort_order, 1);
    assert_eq!(get(&conn, a.id).unwrap().sort_order, 2);
}

#[test]
fn reorder_rejects_mixed_parents() {
    let conn = init_in_memory().unwrap();
    let r1 = create(&conn, None, "R1", None).unwrap();
    let r2 = create(&conn, None, "R2", None).unwrap();
    let c = create(&conn, Some(r1.id), "C", None).unwrap();
    let err = reorder(&conn, &[r2.id, c.id]).unwrap_err();
    assert!(err.to_string().contains("INVALID") || err.to_string().contains("同"));
}

#[test]
fn move_folder_appends_sort_under_new_parent() {
    let conn = init_in_memory().unwrap();
    let b = create(&conn, None, "B", None).unwrap();
    let _x = create(&conn, Some(b.id), "X", None).unwrap();
    let a = create(&conn, None, "A", None).unwrap();
    move_folder(&conn, a.id, Some(b.id)).unwrap();
    let kids = list(&conn).unwrap().into_iter().filter(|f| f.parent_id == Some(b.id)).collect::<Vec<_>>();
    // X sort 1, A should be 2
    assert_eq!(get(&conn, a.id).unwrap().sort_order, 2);
}
```

Rename/remove old `reorder_rejects_non_root_mix` to match new semantics.

- [ ] **Step 2: Run — expect FAIL** on nested reorder / move append

- [ ] **Step 3: Implement**

```rust
// reorder: compare parent_id of first folder; all must match (Option equality)
// error: "INVALID_ARGUMENT: 只能重排同一父级下的文件夹"

// move_folder: after UPDATE parent_id, set sort_order = next_sibling_sort(conn, new_parent_id)
```

- [ ] **Step 4: Tests PASS**

- [ ] **Step 5: Commit**

```powershell
git commit -m "feat(kb): same-parent folder reorder and append sort on move"
```

---

### Task 3: Commands + frontend API/store

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/tauri.ts`
- Modify: `src/stores/workbench.ts`

**Interfaces:**
- Produces:
  - `reorder_notes(ids: Vec<i64>)`
  - `tauriApi.reorderNotes(ids: number[]): Promise<void>`
  - `store.reorderNotes(ids: number[])`
  - `Note.sort_order: number` in TS

- [ ] **Step 1: Register command**

Mirror `reorder_note_folders`:

```rust
#[tauri::command]
pub fn reorder_notes(state: State<'_, DbState>, ids: Vec<i64>) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::reorder(&conn, &ids).map_err(|e| e.to_string())
}
```

Add to `invoke_handler!` in `lib.rs`.

- [ ] **Step 2: TypeScript**

```typescript
// Note interface
sort_order?: number

reorderNotes: (ids: number[]) => invoke<void>('reorder_notes', { ids }),
```

Store:

```typescript
async function reorderNotes(ids: number[]) {
  if (!isTauri()) { /* optimistic local sort_order */ return }
  await tauriApi.reorderNotes(ids)
  await refreshNotes() // prefer path that preserves content if list_meta wipe still present — use existing refreshNotes; do not expand scope to fix wipe
}
```

Export `reorderNotes` from `useStore()`.

- [ ] **Step 3: `npm run build` (or `npx vue-tsc --noEmit`) — PASS**

- [ ] **Step 4: Commit**

```powershell
git commit -m "feat(kb): reorder_notes command and store wiring"
```

---

### Task 4: Selection, FolderPlus, ▲▼

**Files:**
- Modify: `src/components/NoteFolderTree.vue`
- Modify: `src/components/NoteList.vue` (only if blank clear must bubble)

**Interfaces:**
- Consumes: `props.selectedFolderId`
- Produces: blank click → `emit('select-folder', null)`; FolderPlus → `startCreate(selectedFolderId ?? null)`

- [ ] **Step 1: Fix sort stepper**

```typescript
function onSortStepPointer(e: MouseEvent) {
  const el = e.currentTarget as HTMLElement
  const mid = el.getBoundingClientRect().height / 2
  bumpSort(e.offsetY < mid ? -1 : 1) // ▲ decrease, ▼ increase
}
```

- [ ] **Step 2: FolderPlus uses selection**

```typescript
function onFolderPlusClick() {
  const parent = props.selectedFolderId ?? null
  startCreate(parent)
}
// template: @click="onFolderPlusClick"
```

- [ ] **Step 3: Blank clear on tree body**

On `.nl-tree-body` `@click`:

```typescript
function onTreeBodyClick(e: MouseEvent) {
  const t = e.target as HTMLElement | null
  if (!t) return
  if (t.closest('.tree-row, .edit-row, .tree-drop-line, button, input')) return
  if (editing.value) cancelEdit()
  emit('select-folder', null)
}
```

Do not clear when clicking tree head / tags (outside body).

- [ ] **Step 4: Manual / browser check** — FolderPlus under selected folder shows `showCreateEditUnder`; blank clears highlight; ▲ decreases number.

- [ ] **Step 5: Commit**

```powershell
git commit -m "fix(kb): tree blank deselect, FolderPlus parent, sort stepper direction"
```

---

### Task 5: Pointer DnD — nest + sibling reorder (folders & notes)

**Files:**
- Modify: `src/components/NoteFolderTree.vue` (extend / replace root-only drag)
- Optional Create: `src/utils/noteTreeHit.ts` + vitest/node assert if project has a front test runner; otherwise keep pure helpers tested via ad-hoc or skip and rely on Rust + handtest
- Modify: tree row templates — `data-folder-id`, `data-note-id`, drop-target class

**Interfaces:**
- Consumes: `store.moveFolder`, `store.setNoteFolder`, `store.reorderFolders`, `store.reorderNotes`
- Produces: drag UX per spec §3

- [ ] **Step 1: Tree sort by `sort_order`**

```typescript
function notesInFolder(folderId: number | null): Note[] {
  return props.notes
    .filter((n) => (n.folder_id ?? null) === folderId)
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0) || a.id - b.id)
}
// childrenOf already uses folder.sort_order via useNoteFolders — verify ASC
```

- [ ] **Step 2: Hit-test helper (inline or util)**

```typescript
type DropAim =
  | { kind: 'nest'; folderId: number }
  | { kind: 'folder-gap'; parentId: number | null; beforeId: number | null; afterId: number | null }
  | { kind: 'note-gap'; folderId: number | null; beforeId: number | null; afterId: number | null }

/** mid 40% of folder row height = nest; outer 30% = gap above/below */
function aimFromPoint(clientY: number, rowEl: HTMLElement, row: TreeRow): DropAim { /* … */ }
```

- [ ] **Step 3: Generalize pointer drag**

- Start drag from folder row (any depth) or note row (threshold 5px).
- While moving: compute aim; show insert line OR `drop-target` on folder; hover-expand collapsed folder after 400ms.
- On up:
  - **nest folder** → `moveFolder(id, targetId)` (backend appends sort).
  - **nest note** → `setNoteFolder(id, targetId)`.
  - **folder-gap** → build sibling id list under `parentId`, splice dragged id, if parent changed `moveFolder` first then `reorderFolders(ids)`.
  - **note-gap** → same with `reorderNotes`; if folder changed `setNoteFolder` first.
  - Illegal self/descendant → toast.
- Esc / blur → cancel.
- `body.note-folder-dragging` cursor grabbing (existing).

Mark rows: `data-tree-folder="id"`, `data-tree-note="id"` (replace root-only `data-root-folder` or keep both).

- [ ] **Step 4: `vue-tsc` / build PASS**

- [ ] **Step 5: Commit**

```powershell
git commit -m "feat(kb): pointer nest-on-drop and sibling reorder for folders and notes"
```

---

### Task 6: Acceptance pass

**Files:** none (manual + tests)

- [ ] **Step 1: Rust filters**

```powershell
# folder + note reorder tests
cargo test --manifest-path src-tauri/Cargo.toml --lib -- repo::folder::tests repo::note::tests -- --test-threads=1
```

Expected: PASS (after manifest embed on Windows).

- [ ] **Step 2: GUI checklist (spec)**

- [ ] Blank clears folder selection  
- [ ] FolderPlus: no selection → top create; selection → under folder  
- [ ] New note: no selection / top-level note → top-level note  
- [ ] ▲ decreases sort number  
- [ ] Drag folder onto folder → becomes last child folder  
- [ ] Drag note onto folder → last note in folder  
- [ ] Gap between root folders reorders / promotes  
- [ ] Folders always listed before notes in same parent  

- [ ] **Step 3: Commit polish if any**

```powershell
git commit -m "fix(kb): tree focus/dnd acceptance polish"
```

---

## Spec coverage (self-review)

| Spec item | Task |
|-----------|------|
| Blank deselect (A) | T4 |
| FolderPlus parent vs top | T4 |
| New note parent from selection | T4 (existing NoteList + clear) |
| ▲▼ direction | T4 |
| Nest on folder row | T5 |
| Gap sibling reorder + root promote | T5 |
| Notes + folders both draggable | T5 |
| Isolated sort_order + folders-first render | T1 + T5 |
| `reorder` same-parent folders | T2 |
| `reorder_notes` | T1 + T3 |
| No HTML5 / no import changes | Global |

## Placeholder scan

No TBD steps; commands and signatures named above.

## Type consistency

- `note::reorder` / `reorder_notes` / `reorderNotes(ids: number[])`
- `folder::reorder` same-parent; `reorderFolders` unchanged name
- `Note.sort_order: i64` / optional `number` on TS with `?? 0` in sorts
