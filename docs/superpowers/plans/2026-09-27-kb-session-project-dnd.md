# KB Session Sidebar Project DnD + Zone Sort Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let KB chat sessions drag between projects / recent / pinned with independent manual order per zone, and replace the empty-project「暂无会话」copy with a short drop hit strip.

**Architecture:** Add `sort_order` (membership bucket) and `pin_sort_order` (pinned zone only) on `kb_sessions`. One transactional `place_kb_session` applies pin/project + densified order; `reorder_kb_sessions` covers pure same-zone rewrites. Sidebar uses pointer DnD (no HTML5), mirroring `NoteFolderTree` hit rules (row mid → gap, container highlight, 400ms expand).

**Tech Stack:** Tauri 2, rusqlite, Vue 3, TypeScript; no new crates/npm deps

## Global Constraints

- Spec: `docs/superpowers/specs/2026-09-27-kb-session-project-dnd-design.md`
- Pointer drag only — never HTML5 DnD (Tauri `dragDropEnabled`)
- Design tokens only; `:global()` must wrap full selectors (AGENTS 43)
- Windows Rust tests: `npm run tauri:test` (never bare `cargo test`)
- Keep context-menu pin/move; menu paths append to target bucket end
- Drag from **pinned zone** onto a project → `pinned=0`; same-session reorder inside a project keeps pin
- Commit only when user approved execution / asked to commit

## File map

| Path | Responsibility |
|------|----------------|
| `src-tauri/src/db.rs` | ALTER + backfill `sort_order` / `pin_sort_order`; update CREATE for fresh DBs |
| `src-tauri/src/models.rs` | `KbSession` fields; `KbPlaceTarget` / place args for commands |
| `src-tauri/src/repo/kb_chat.rs` | densify, create/pin/move/place/reorder, delete_project side effects, tests |
| `src-tauri/src/knowledge.rs` | `place_kb_session` / `reorder_kb_sessions` commands |
| `src-tauri/src/lib.rs` | Register new commands |
| `src/api/tauri.ts` | Types + wrappers |
| `src/components/KbSessionSidebar.vue` | Zone sort, empty strip, pointer DnD |
| `src/components/KnowledgeView.vue` | Wire `place` refresh |

---

### Task 1: Schema + model fields

**Files:**
- Modify: `src-tauri/src/models.rs` (`KbSession`)
- Modify: `src-tauri/src/db.rs` (`kb_sessions` CREATE + ALTER after that batch)
- Modify: `src-tauri/src/repo/kb_chat.rs` (`SESSION_COLS`, `row_to_session` only — behavior in Task 2)
- Modify: `src/api/tauri.ts` (`KbSession` interface)

**Interfaces:**
- Produces: `KbSession { …, sort_order: i64, pin_sort_order: i64 }`
- Consumes: none

- [ ] **Step 1: Extend `KbSession` in `models.rs`**

```rust
pub struct KbSession {
    pub id: i64,
    pub title: String,
    pub model_name: String,
    pub project_id: Option<i64>,
    pub pinned: bool,
    pub sort_order: i64,
    pub pin_sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}
```

- [ ] **Step 2: Update CREATE TABLE `kb_sessions` in `db.rs` migrate batch**

Add columns to the `CREATE TABLE IF NOT EXISTS kb_sessions` block:

```sql
sort_order INTEGER NOT NULL DEFAULT 0,
pin_sort_order INTEGER NOT NULL DEFAULT 0,
```

(after `pinned`, before `created_at`)

- [ ] **Step 3: Idempotent ALTER + backfill (after the kb_* CREATE batch)**

```rust
let kb_sess_cols: Vec<String> = conn
    .prepare("PRAGMA table_info(kb_sessions)")?
    .query_map([], |row| row.get(1))?
    .collect::<rusqlite::Result<Vec<_>>>()?;
if !kb_sess_cols.iter().any(|c| c == "sort_order") {
    conn.execute(
        "ALTER TABLE kb_sessions ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0",
        [],
    )?;
}
if !kb_sess_cols.iter().any(|c| c == "pin_sort_order") {
    conn.execute(
        "ALTER TABLE kb_sessions ADD COLUMN pin_sort_order INTEGER NOT NULL DEFAULT 0",
        [],
    )?;
}
// Backfill once when either column was just added OR both are still all-zero with rows:
backfill_kb_session_sort_orders(conn)?;
```

Implement `backfill_kb_session_sort_orders`:

1. For each distinct `IFNULL(project_id, -1)` group: `ORDER BY updated_at DESC, id DESC` → assign `sort_order = 1..n`.
2. For `pinned = 1` rows: `ORDER BY updated_at DESC, id DESC` → assign `pin_sort_order = 1..n`.
3. Unpinned rows: `pin_sort_order = 0`.

Guard so re-running migrate does not reshuffle: only backfill when `sort_order` column was newly added in this migrate pass (same pattern as `notes.sort_order` — call backfill inside the `if !…sort_order` branch; if only `pin_sort_order` is missing, backfill pins only).

- [ ] **Step 4: Update `SESSION_COLS` + `row_to_session`**

```rust
const SESSION_COLS: &str =
    "id, title, model_name, project_id, pinned, sort_order, pin_sort_order, created_at, updated_at";

fn row_to_session(row: &rusqlite::Row) -> Result<KbSession> {
    Ok(KbSession {
        id: row.get(0)?,
        title: row.get(1)?,
        model_name: row.get(2)?,
        project_id: row.get(3)?,
        pinned: row.get::<_, i64>(4)? != 0,
        sort_order: row.get(5)?,
        pin_sort_order: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}
```

- [ ] **Step 5: Mirror fields on `KbSession` in `src/api/tauri.ts`**

```ts
export interface KbSession {
  id: number
  title: string
  model_name: string
  project_id: number | null
  pinned: boolean
  sort_order: number
  pin_sort_order: number
  created_at: string
  updated_at: string
}
```

- [ ] **Step 6: Smoke test in `db.rs` tests**

```rust
#[test]
fn kb_sessions_have_sort_columns_after_migrate() {
    let conn = init_in_memory().unwrap();
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(kb_sessions)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(cols.iter().any(|c| c == "sort_order"));
    assert!(cols.iter().any(|c| c == "pin_sort_order"));
}
```

- [ ] **Step 7: Run tests**

Run: `npm run tauri:test -- db::tests::kb_sessions_have_sort_columns_after_migrate`
Expected: PASS (fix compile errors in kb_chat tests that construct `KbSession` literals if any — update those structs).

- [ ] **Step 8: Commit** (only if user asked)

```bash
git add src-tauri/src/models.rs src-tauri/src/db.rs src-tauri/src/repo/kb_chat.rs src/api/tauri.ts
git commit -m "feat(kb): add session sort_order and pin_sort_order columns"
```

---

### Task 2: Repo densify / place / reorder

**Files:**
- Modify: `src-tauri/src/repo/kb_chat.rs`
- Modify: `src-tauri/src/models.rs` (add place/reorder request types next to `KbSession`)

**Interfaces:**
- Produces:
  - `densify_membership(conn, project_id: Option<i64>)` — rewrite `sort_order` 1..n for all rows with that `project_id` (NULL bucket = all `project_id IS NULL`), order by current `sort_order ASC, id ASC`
  - `densify_pinned(conn)` — rewrite `pin_sort_order` 1..n for `pinned=1`
  - `create_session` → append `sort_order = max+1` in target membership bucket; `pin_sort_order=0`
  - `set_pinned(conn, id, pinned)` → if true append pin order; if false zero pin order + densify_pinned
  - `move_to_project(conn, id, project_id)` → change project, densify old+new membership; **does not** change `pinned`
  - `delete_project` → after DELETE, `densify_membership(conn, None)` for recent-visible rows only: rewrite `sort_order` for `project_id IS NULL AND pinned = 0`
  - `reorder_sessions(conn, zone, project_id, ids: &[i64])`
  - `place_session(conn, id, source_zone, target) -> KbSession`
- Zone string / enum:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KbSessionZone {
    Pinned,
    Recent,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KbPlaceTarget {
    pub zone: KbSessionZone,
    /// Required when `zone == Project`
    pub project_id: Option<i64>,
    /// Insert before this session id within the target zone list; `None` = append
    pub before_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KbDragSourceZone {
    Pinned,
    Recent,
    Project,
}
```

**`place_session` rules (lock these):**

1. Load session `id`.
2. Match `target.zone`:
   - `Pinned`: set `pinned=1`; leave `project_id` unchanged.
   - `Recent`: set `pinned=0`, `project_id=None`.
   - `Project`: require `project_id`; set `project_id`; if `source_zone == Pinned` set `pinned=0` (else keep `pinned`).
3. Build ordered id list for the **target zone after membership/pin change**, excluding `id`, then insert `id` before `before_id` or at end.
4. Persist pin/project fields, then:
   - if target pinned → `reorder` pin list via `pin_sort_order`
   - else → densify/rewrite membership `sort_order` for that bucket (and densify old membership bucket if `project_id` changed; densify pinned if pin flipped off)
5. Touch `updated_at` once at end.
6. Return `get_session`.

**`reorder_sessions`:**

- `Pinned`: every id must have `pinned=1`; write `pin_sort_order = 1..n`
- `Recent`: every id must have `pinned=0 && project_id IS NULL`; write `sort_order`
- `Project`: every id must have `project_id == Some(project_id)`; write `sort_order`
- Reject mixed lists with `INVALID_ARGUMENT: …`

- [ ] **Step 1: Write failing tests in `kb_chat.rs`**

```rust
#[test]
fn create_appends_sort_in_bucket() {
    let conn = init_in_memory().unwrap();
    let a = create_session(&conn, "a", "m", None).unwrap();
    let b = create_session(&conn, "b", "m", None).unwrap();
    assert_eq!(a.sort_order, 1);
    assert_eq!(b.sort_order, 2);
}

#[test]
fn place_into_project_from_recent() {
    let conn = init_in_memory().unwrap();
    let p = create_project(&conn, "P").unwrap();
    let s = create_session(&conn, "t", "m", None).unwrap();
    let out = place_session(
        &conn,
        s.id,
        KbDragSourceZone::Recent,
        &KbPlaceTarget {
            zone: KbSessionZone::Project,
            project_id: Some(p.id),
            before_id: None,
        },
    )
    .unwrap();
    assert_eq!(out.project_id, Some(p.id));
    assert!(!out.pinned);
    assert_eq!(out.sort_order, 1);
}

#[test]
fn place_from_pinned_zone_to_project_unpins() {
    let conn = init_in_memory().unwrap();
    let p = create_project(&conn, "P").unwrap();
    let s = create_session(&conn, "t", "m", Some(p.id)).unwrap();
    set_pinned(&conn, s.id, true).unwrap();
    let out = place_session(
        &conn,
        s.id,
        KbDragSourceZone::Pinned,
        &KbPlaceTarget {
            zone: KbSessionZone::Project,
            project_id: Some(p.id),
            before_id: None,
        },
    )
    .unwrap();
    assert!(!out.pinned);
    assert_eq!(out.project_id, Some(p.id));
}

#[test]
fn place_to_pinned_keeps_project_id() {
    let conn = init_in_memory().unwrap();
    let p = create_project(&conn, "P").unwrap();
    let s = create_session(&conn, "t", "m", Some(p.id)).unwrap();
    let out = place_session(
        &conn,
        s.id,
        KbDragSourceZone::Project,
        &KbPlaceTarget {
            zone: KbSessionZone::Pinned,
            project_id: None,
            before_id: None,
        },
    )
    .unwrap();
    assert!(out.pinned);
    assert_eq!(out.project_id, Some(p.id));
    assert_eq!(out.pin_sort_order, 1);
}

#[test]
fn place_to_recent_clears_project_and_pin() {
    let conn = init_in_memory().unwrap();
    let p = create_project(&conn, "P").unwrap();
    let s = create_session(&conn, "t", "m", Some(p.id)).unwrap();
    set_pinned(&conn, s.id, true).unwrap();
    let out = place_session(
        &conn,
        s.id,
        KbDragSourceZone::Pinned,
        &KbPlaceTarget {
            zone: KbSessionZone::Recent,
            project_id: None,
            before_id: None,
        },
    )
    .unwrap();
    assert!(!out.pinned);
    assert_eq!(out.project_id, None);
}

#[test]
fn reorder_pinned_independent_of_project_order() {
    let conn = init_in_memory().unwrap();
    let p = create_project(&conn, "P").unwrap();
    let a = create_session(&conn, "a", "m", Some(p.id)).unwrap();
    let b = create_session(&conn, "b", "m", Some(p.id)).unwrap();
    set_pinned(&conn, a.id, true).unwrap();
    set_pinned(&conn, b.id, true).unwrap();
    reorder_sessions(&conn, KbSessionZone::Pinned, None, &[b.id, a.id]).unwrap();
    reorder_sessions(&conn, KbSessionZone::Project, Some(p.id), &[a.id, b.id]).unwrap();
    assert_eq!(get_session(&conn, b.id).unwrap().pin_sort_order, 1);
    assert_eq!(get_session(&conn, a.id).unwrap().pin_sort_order, 2);
    assert_eq!(get_session(&conn, a.id).unwrap().sort_order, 1);
    assert_eq!(get_session(&conn, b.id).unwrap().sort_order, 2);
}

#[test]
fn reorder_rejects_cross_zone() {
    let conn = init_in_memory().unwrap();
    let p = create_project(&conn, "P").unwrap();
    let a = create_session(&conn, "a", "m", None).unwrap();
    let b = create_session(&conn, "b", "m", Some(p.id)).unwrap();
    let err = reorder_sessions(&conn, KbSessionZone::Recent, None, &[a.id, b.id]).unwrap_err();
    assert!(err.to_string().contains("INVALID"));
}

#[test]
fn delete_project_densifies_recent() {
    let conn = init_in_memory().unwrap();
    let p = create_project(&conn, "P").unwrap();
    let r = create_session(&conn, "r", "m", None).unwrap();
    let s = create_session(&conn, "s", "m", Some(p.id)).unwrap();
    delete_project(&conn, p.id).unwrap();
    let s2 = get_session(&conn, s.id).unwrap();
    assert_eq!(s2.project_id, None);
    // both in recent bucket with dense 1..n
    let mut recent: Vec<_> = list_sessions(&conn)
        .unwrap()
        .into_iter()
        .filter(|x| !x.pinned && x.project_id.is_none())
        .collect();
    recent.sort_by_key(|x| x.sort_order);
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].sort_order, 1);
    assert_eq!(recent[1].sort_order, 2);
    let _ = r;
}

#[test]
fn menu_move_appends_to_bucket() {
    let conn = init_in_memory().unwrap();
    let p = create_project(&conn, "P").unwrap();
    let a = create_session(&conn, "a", "m", Some(p.id)).unwrap();
    let b = create_session(&conn, "b", "m", None).unwrap();
    let moved = move_to_project(&conn, b.id, Some(p.id)).unwrap();
    assert_eq!(moved.sort_order, 2);
    assert_eq!(get_session(&conn, a.id).unwrap().sort_order, 1);
}
```

- [ ] **Step 2: Run tests — expect FAIL**

Run: `npm run tauri:test -- kb_chat::tests::place_into_project_from_recent`
Expected: FAIL (function missing / wrong behavior)

- [ ] **Step 3: Implement helpers + update create/pin/move/delete**

Sketch (adapt to file style):

```rust
fn next_membership_sort(conn: &Connection, project_id: Option<i64>) -> Result<i64> {
    let max: Option<i64> = match project_id {
        Some(pid) => conn.query_row(
            "SELECT MAX(sort_order) FROM kb_sessions WHERE project_id = ?1",
            params![pid],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT MAX(sort_order) FROM kb_sessions WHERE project_id IS NULL",
            [],
            |r| r.get(0),
        )?,
    };
    Ok(max.unwrap_or(0) + 1)
}

fn next_pin_sort(conn: &Connection) -> Result<i64> {
    let max: Option<i64> = conn.query_row(
        "SELECT MAX(pin_sort_order) FROM kb_sessions WHERE pinned = 1",
        [],
        |r| r.get(0),
    )?;
    Ok(max.unwrap_or(0) + 1)
}
```

Update `create_session` INSERT to set `sort_order = next_membership_sort(...)`, `pin_sort_order = 0`.

Update `set_pinned`: on `true`, set `pin_sort_order = next_pin_sort`; on `false`, set `pin_sort_order = 0` then densify pinned.

Update `move_to_project`: if same project return; else densify old bucket after move; set `sort_order = next` in new bucket.

Update `delete_project`: after DELETE, densify `project_id IS NULL AND pinned = 0`.

- [ ] **Step 4: Implement `reorder_sessions` + `place_session`**

Follow Interfaces rules above. Use a transaction (`conn.unchecked_transaction()` or explicit BEGIN) inside `place_session`.

- [ ] **Step 5: Run all new kb_chat tests**

Run: `npm run tauri:test -- kb_chat::`
Expected: PASS

- [ ] **Step 6: Commit** (only if user asked)

```bash
git add src-tauri/src/repo/kb_chat.rs src-tauri/src/models.rs
git commit -m "feat(kb): place/reorder sessions with zone sort orders"
```

---

### Task 3: Tauri commands + frontend API

**Files:**
- Modify: `src-tauri/src/knowledge.rs` (after `move_kb_session_to_project`)
- Modify: `src-tauri/src/lib.rs` (`invoke_handler`)
- Modify: `src/api/tauri.ts`

**Interfaces:**
- Produces:
  - `place_kb_session(id, sourceZone, target) -> KbSession`
  - `reorder_kb_sessions(zone, projectId, ids) -> ()`
- CamelCase IPC args (Tauri default): `sourceZone`, `projectId`, `beforeId`

- [ ] **Step 1: Add commands**

```rust
#[tauri::command]
pub fn place_kb_session(
    state: State<'_, DbState>,
    id: i64,
    source_zone: KbDragSourceZone,
    target: KbPlaceTarget,
) -> Result<KbSession, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    kb_chat::place_session(&conn, id, source_zone, &target).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reorder_kb_sessions(
    state: State<'_, DbState>,
    zone: KbSessionZone,
    project_id: Option<i64>,
    ids: Vec<i64>,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    kb_chat::reorder_sessions(&conn, zone, project_id, &ids).map_err(|e| e.to_string())
}
```

Export the enums/structs from `models` (or `kb_chat`) so commands can deserialize them — prefer `models.rs` so `api` types stay parallel.

- [ ] **Step 2: Register in `lib.rs`** next to other `kb_` session commands

- [ ] **Step 3: Add `tauriApi` wrappers**

```ts
export type KbSessionZone = 'pinned' | 'recent' | 'project'
export type KbDragSourceZone = 'pinned' | 'recent' | 'project'

export interface KbPlaceTarget {
  zone: KbSessionZone
  projectId?: number | null
  beforeId?: number | null
}

// inside tauriApi:
placeKbSession: (
  id: number,
  sourceZone: KbDragSourceZone,
  target: KbPlaceTarget,
) =>
  invoke<KbSession>('place_kb_session', {
    id,
    sourceZone,
    target: {
      zone: target.zone,
      projectId: target.projectId ?? null,
      beforeId: target.beforeId ?? null,
    },
  }),

reorderKbSessions: (
  zone: KbSessionZone,
  ids: number[],
  projectId?: number | null,
) =>
  invoke<void>('reorder_kb_sessions', {
    zone,
    projectId: projectId ?? null,
    ids,
  }),
```

Serde on Rust side: use `#[serde(rename_all = "camelCase")]` on `KbPlaceTarget`; for zone enums use `#[serde(rename_all = "snake_case")]` **or** `rename_all = "lowercase"` matching the TS string `'pinned'|'recent'|'project'`. **Lock to lowercase** in both ends:

```rust
#[serde(rename_all = "lowercase")]
pub enum KbSessionZone { Pinned, Recent, Project }
```

- [ ] **Step 4: `npm run build`** (frontend typecheck)

Expected: PASS

- [ ] **Step 5: Commit** (only if user asked)

```bash
git add src-tauri/src/knowledge.rs src-tauri/src/lib.rs src-tauri/src/models.rs src/api/tauri.ts
git commit -m "feat(kb): expose place_kb_session and reorder_kb_sessions"
```

---

### Task 4: Sidebar sort + empty drop strip (no drag yet)

**Files:**
- Modify: `src/components/KbSessionSidebar.vue`

**Interfaces:**
- Consumes: `session.sort_order` / `pin_sort_order`
- Produces: UI without「暂无会话」; empty-project strip; pinned section header always visible when projects/recent exist or always visible

- [ ] **Step 1: Replace sort comparators**

```ts
function bySortAsc(a: KbSession, b: KbSession) {
  if (a.sort_order !== b.sort_order) return a.sort_order - b.sort_order
  return b.updated_at.localeCompare(a.updated_at)
}
function byPinSortAsc(a: KbSession, b: KbSession) {
  if (a.pin_sort_order !== b.pin_sort_order) return a.pin_sort_order - b.pin_sort_order
  return b.updated_at.localeCompare(a.updated_at)
}

const pinned = computed(() =>
  props.sessions.filter((s) => s.pinned).slice().sort(byPinSortAsc),
)
const recent = computed(() =>
  props.sessions
    .filter((s) => !s.pinned && s.project_id == null)
    .slice()
    .sort(bySortAsc),
)
// projectBlocks sessions: .sort(bySortAsc)
```

- [ ] **Step 2: Remove「暂无会话」; add drop strip**

Replace:

```vue
<p v-if="block.sessions.length === 0" class="kb-empty-hint">暂无会话</p>
```

With:

```vue
<div
  v-if="block.sessions.length === 0 && isExpanded(block.project.id)"
  class="kb-drop-strip"
  data-kb-drop="project-empty"
  :data-project-id="block.project.id"
  aria-hidden="true"
/>
```

CSS (scoped, tokens only):

```css
.kb-drop-strip {
  height: 10px;
  margin: 2px 8px 6px;
  border-radius: 6px;
  border: 1px dashed transparent;
}
.kb-drop-strip.is-hot {
  border-color: color-mix(in srgb, var(--brand-500) 55%, transparent);
  background: color-mix(in srgb, var(--brand-500) 12%, transparent);
}
```

- [ ] **Step 3: Ensure 置顶区头 always mounts** (even when `pinned.length === 0`) so Task 5 can target it — keep the section wrapper; empty body OK.

- [ ] **Step 4: Manual check in UI** — empty project shows strip, no「暂无会话」; order matches DB after pin/move via menu (append).

- [ ] **Step 5: Commit** (only if user asked)

```bash
git add src/components/KbSessionSidebar.vue
git commit -m "fix(kb): zone sort display and empty-project drop strip"
```

---

### Task 5: Pointer DnD + KnowledgeView `place`

**Files:**
- Modify: `src/components/KbSessionSidebar.vue`
- Modify: `src/components/KnowledgeView.vue`

**Interfaces:**
- Sidebar emits `place: [id, sourceZone, target]`
- Parent calls `tauriApi.placeKbSession` then `refreshSessionLists`
- Hit model (pointer only):

| Hit | `target` |
|-----|----------|
| 置顶区头 / 置顶空白 / 置顶缝 before S | `{ zone:'pinned', beforeId: S\|null }` |
| 最近区头 / 空白 / 缝 before S | `{ zone:'recent', beforeId: S\|null }` |
| 项目行中部 / `.kb-drop-strip` | `{ zone:'project', projectId, beforeId: null }` |
| 项目内缝 before S | `{ zone:'project', projectId, beforeId: S\|null }` |
| 会话行中部 | upper half → before that id; lower → before next / null |
| Outside / invalid | cancel |

- Threshold ~4px before drag starts (avoid fighting click-to-select).
- Esc / blur cancel; `disabled` blocks pointerdown start.
- Collapsed project: hover 400ms → expand.
- Classes: source `is-dragging` (opacity ~0.45); seam line `.kb-insert-line`; container `.drop-target` / strip `.is-hot`.
- `sourceZone` = which list row initiated the drag (`pinned` | `project` | `recent`).

- [ ] **Step 1: Add emit + parent handler**

In sidebar `defineEmits` add:

```ts
place: [id: number, sourceZone: KbDragSourceZone, target: KbPlaceTarget]
```

In `KnowledgeView.vue`:

```ts
async function onPlaceSession(
  id: number,
  sourceZone: KbDragSourceZone,
  target: KbPlaceTarget,
) {
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  try {
    await tauriApi.placeKbSession(id, sourceZone, target)
    await refreshSessionLists()
  } catch (e) {
    showToast(`移动失败：${String(e)}`)
  }
}
```

Template: `@place="onPlaceSession"`.

- [ ] **Step 2: Implement pointer drag state machine in sidebar**

Keep logic inside `KbSessionSidebar.vue` (YAGNI — no new composable unless file exceeds ~900 lines). Pattern reference: `NoteFolderTree.vue` pointerdown/move/up + window keydown/blur.

Minimal state:

```ts
type DragState = {
  id: number
  sourceZone: KbDragSourceZone
  startY: number
  startX: number
  active: boolean // past threshold
}
const drag = ref<DragState | null>(null)
const hot = ref<null | { kind: 'container' | 'gap'; key: string; beforeId: number | null }>(null)
```

On `pointerup` with valid hot → `emit('place', …)` then clear.

Attach `data-kb-drop` attributes:

- `data-kb-drop="pinned-head" | "recent-head" | "project-row" | "project-empty" | "session"`
- `data-kb-zone`, `data-session-id`, `data-project-id` as needed

Hit-test with `document.elementFromPoint` + `closest('[data-kb-drop]')`.

- [ ] **Step 3: Styles for drag/insert/drop-target** using existing brand/frost tokens; no new palette.

- [ ] **Step 4: Manual acceptance (spec §7.2)**

1. Empty project: no「暂无会话」, strip receives drop, count +1  
2. Recent → project → other project → drop on 最近  
3. Drop on 置顶 pins; drag from 置顶 onto project unpins  
4. Reorder within each zone; dual-appear pin vs project orders independent  
5. Menu move/pin still works (append)  
6. Generating: cannot drag; Esc cancels  

- [ ] **Step 5: `npm run build` + `npm run tauri:test -- kb_chat::`**

Expected: PASS

- [ ] **Step 6: Commit** (only if user asked)

```bash
git add src/components/KbSessionSidebar.vue src/components/KnowledgeView.vue
git commit -m "feat(kb): pointer drag sessions across projects and zones"
```

---

## Spec coverage checklist

| Spec requirement | Task |
|------------------|------|
| Remove「暂无会话」+ drop strip | 4 |
| Drag ↔ projects / to 最近 | 2, 5 |
| Drag pin / unpin from pinned zone | 2, 5 |
| Independent pin vs project order | 1, 2, 4 |
| Three-zone manual reorder | 2, 5 |
| Menu parity (append) | 2 |
| `place` transactional API | 2, 3 |
| Pointer-only DnD | 5 |
| delete project → recent densify | 2 |
| Tests listed in spec §7.1 | 1–2 |

## Self-review notes

- Zone serde locked to **lowercase** (`pinned`/`recent`/`project`) on both ends — do not mix snake_case.
- `sourceZone === pinned` is the only path that forces unpin when landing on a project; reordering the project-row copy of a pinned session must pass `sourceZone: 'project'`.
- Fresh CREATE and ALTER both define the two columns; backfill only when ALTER adds `sort_order`.
