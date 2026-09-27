# KB Multi-turn Sessions + Doubao Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist knowledge-base Q&A as multi-turn sessions with a Doubao-style left rail (pinned / projects / recent) and bottom composer, restoring the last session + model when returning to the KB view.

**Architecture:** New SQLite tables `kb_projects` / `kb_sessions` / `kb_messages` (isolated from `chat_*`). `kb_ask` requires `session_id`, appends user/assistant rows, runs fresh RAG each turn, and sends truncated session history to the LLM. Frontend `KnowledgeView` becomes left rail + message timeline + bottom composer; `kb_active_session_id` in `AppConfig` (backend-managed) restores the open session across view switches and restarts.

**Tech Stack:** Tauri 2, rusqlite, Vue 3, existing `stream_chat` / `ConfirmDialog` / `AppSelect` / `kbAskDecorate` helpers; no new crates/npm deps

## Global Constraints

- Spec: `docs/superpowers/specs/2026-09-27-kb-chat-sessions-design.md`
- Do **not** merge with `chat_sessions` / `ChatPanel`
- History truncation: last **20 turns** (40 messages) **or** ~**12k** chars of history body, whichever binds first; system prompt excluded from that budget
- Model per session (`kb_sessions.model_name`); Top-K stays global (`kb_top_k`), toolbar override is request-only
- Pinned sessions with `project_id` appear in **both** 置顶 and that project; 最近 = `pinned=0 AND project_id IS NULL`
- Delete project → `ON DELETE SET NULL` (sessions return to 最近); clear all sessions keeps empty projects
- `kb_active_session_id` must be in `merge_disk_authoritative` + `BACKEND_MANAGED_FIELDS` + `AppConfig` comment list in `tauri.ts`
- Design tokens only; `:global()` must wrap full selectors (AGENTS 43)
- Windows Rust tests: `npm run tauri:test` (never bare `cargo test`)
- Breaking: `kb_ask` gains required `sessionId`; update `tauriApi.kbAsk` accordingly
- Commit only when user approved execution / asked to commit

## File map

| Path | Responsibility |
|------|----------------|
| `src-tauri/src/db.rs` | Create `kb_projects` / `kb_sessions` / `kb_messages` + indexes in `migrate` |
| `src-tauri/src/models.rs` | `KbProject`, `KbSession`, `KbMessage` structs |
| `src-tauri/src/repo/kb_chat.rs` | CRUD for projects/sessions/messages + cascade/clear tests |
| `src-tauri/src/repo/mod.rs` | `pub mod kb_chat` |
| `src-tauri/src/config.rs` | `kb_active_session_id` + merge + `BACKEND_MANAGED_FIELDS` |
| `src-tauri/src/knowledge.rs` | `truncate_kb_history`, `kb_ask(session_id, …)` persist + assemble |
| `src-tauri/src/commands.rs` (or `knowledge.rs`) | Thin Tauri commands for CRUD / active session |
| `src-tauri/src/lib.rs` | Register new commands |
| `src/api/tauri.ts` | Types + `tauriApi` wrappers; comment list for backend-managed field |
| `src/components/KbSessionSidebar.vue` | Left rail: sections, menus, new/clear |
| `src/components/KnowledgeView.vue` | Layout shell, message timeline, ask, restore, bottom composer |
| `docs/knowledge-base/PRD-个人知识库系统.md` | Revise F4.6 |

---

### Task 1: Models + DB migration

**Files:**
- Modify: `src-tauri/src/models.rs` (after `Citation` / near chat models)
- Modify: `src-tauri/src/db.rs` (`migrate` batch after existing `kb_meta` block ~520–549)

**Interfaces:**
- Produces:
  - `KbProject { id: i64, name: String, sort_order: Option<i64>, created_at: String, updated_at: String }`
  - `KbSession { id: i64, title: String, model_name: String, project_id: Option<i64>, pinned: bool, created_at: String, updated_at: String }`
  - `KbMessage { id: i64, session_id: i64, role: String, content: String, citations_json: Option<String>, created_at: String }`
- Consumes: none

- [ ] **Step 1: Add structs to `models.rs`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KbProject {
    pub id: i64,
    pub name: String,
    pub sort_order: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KbSession {
    pub id: i64,
    pub title: String,
    pub model_name: String,
    pub project_id: Option<i64>,
    pub pinned: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KbMessage {
    pub id: i64,
    pub session_id: i64,
    pub role: String,
    pub content: String,
    /// Assistant: JSON array of Citation; user: null
    pub citations_json: Option<String>,
    pub created_at: String,
}
```

Serialize `pinned` as bool; map SQLite `INTEGER 0/1` in repo row mappers.

- [ ] **Step 2: Append tables to `migrate` `execute_batch` (same batch as `kb_meta` or a new batch right after)**

Use the exact DDL from the spec §3.1 (`kb_projects`, `kb_sessions` with `REFERENCES kb_projects(id) ON DELETE SET NULL`, `kb_messages` CASCADE, both indexes).

- [ ] **Step 3: Add a migrate smoke test in `db.rs` tests**

```rust
#[test]
fn kb_chat_tables_exist_after_migrate() {
    let conn = init_in_memory().unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('kb_projects','kb_sessions','kb_messages')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 3);
}
```

- [ ] **Step 4: Run test (RED until tables added; then GREEN)**

Run: `npm run tauri:test -- kb_chat_tables_exist_after_migrate`
Expected: PASS after Step 2–3

- [ ] **Step 5: Commit** (when executing with commit approval)

```bash
git add src-tauri/src/models.rs src-tauri/src/db.rs
git commit -m "feat(kb): add kb_projects/sessions/messages schema"
```

---

### Task 2: `repo/kb_chat.rs` CRUD + tests

**Files:**
- Create: `src-tauri/src/repo/kb_chat.rs`
- Modify: `src-tauri/src/repo/mod.rs` — add `pub mod kb_chat;`

**Interfaces:**
- Consumes: `KbProject`, `KbSession`, `KbMessage`; `crate::repo::now`; `db::init_in_memory` in tests
- Produces (all `rusqlite::Result`):
  - Projects: `list_projects`, `create_project(name)`, `rename_project(id, name)`, `delete_project(id)`
  - Sessions: `list_sessions`, `get_session(id)`, `create_session(title, model_name, project_id: Option<i64>)`, `rename_session`, `set_session_model`, `set_pinned(id, pinned: bool)`, `move_to_project(id, project_id: Option<i64>)`, `delete_session`, `clear_sessions` (DELETE FROM kb_sessions), `touch_session`
  - Messages: `list_messages(session_id)`, `add_message(session_id, role, content, citations_json: Option<&str>)`, `get_message(id)`
  - Title helper: `auto_title_from_question(q: &str) -> String` (trim, collapse whitespace/newlines, chars().take(30).collect(); empty → `"新对话"`)

`create_project`: `sort_order = COALESCE((SELECT MAX(sort_order) FROM kb_projects), 0) + 1`.

`list_projects`: `ORDER BY sort_order ASC, id ASC`.

`list_sessions`: `ORDER BY updated_at DESC` (frontend partitions).

- [ ] **Step 1: Stub module + failing test `project_delete_nulls_session_project_id`**

```rust
#[test]
fn project_delete_nulls_session_project_id() {
    let conn = init_in_memory().unwrap();
    let p = create_project(&conn, "开源").unwrap();
    let s = create_session(&conn, "新对话", "m", Some(p.id)).unwrap();
    assert_eq!(s.project_id, Some(p.id));
    delete_project(&conn, p.id).unwrap();
    let s2 = get_session(&conn, s.id).unwrap();
    assert_eq!(s2.project_id, None);
    assert!(list_projects(&conn).unwrap().is_empty());
}
```

- [ ] **Step 2: Run RED**

Run: `npm run tauri:test -- project_delete_nulls_session_project_id`
Expected: compile fail / FAIL

- [ ] **Step 3: Implement full CRUD** (mirror `repo/chat.rs` style; `pinned` column INTEGER; `row_to_session` uses `row.get::<_, i64>(4)? != 0`)

Also implement and test:
- `message_cascade_on_session_delete`
- `clear_sessions_keeps_projects`
- `auto_title_from_question_truncates` (`"a".repeat(40)` → len 30)
- `set_pinned_and_move`

- [ ] **Step 4: Run GREEN**

Run: `npm run tauri:test -- kb_chat`
Expected: all `kb_chat` tests PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/repo/kb_chat.rs src-tauri/src/repo/mod.rs
git commit -m "feat(kb): kb_chat repo CRUD for projects sessions messages"
```

---

### Task 3: `kb_active_session_id` config (backend-managed)

**Files:**
- Modify: `src-tauri/src/config.rs`
- Modify: `src/api/tauri.ts` — `AppConfig` comment list only (field optional in interface for typing if needed; prefer **comment-only** like `skill_roots` OR add `kb_active_session_id?: number | null` — **add the optional field** so `getUiConfig` consumers can read it if exposed; still backend-managed on save)

**Interfaces:**
- Produces: `AppConfig.kb_active_session_id: Option<i64>` default `None`
- Consumes: existing `merge_disk_authoritative` / `BACKEND_MANAGED_FIELDS` tests

- [ ] **Step 1: Add field after `kb_top_k`**

```rust
/// 知识库当前打开的会话 id（只经 `set_kb_active_session` 写盘）
#[serde(default)]
pub kb_active_session_id: Option<i64>,
```

Update `Default` impl, `merge_disk_authoritative`:

```rust
merged.kb_active_session_id = disk.kb_active_session_id;
```

Add `"kb_active_session_id"` to `BACKEND_MANAGED_FIELDS`.

In `make_disk_config` test helper (or equivalent), set `kb_active_session_id: Some(42)` so merge tests keep covering it.

- [ ] **Step 2: Update `tauri.ts` AppConfig comment checklist** to include `kb_active_session_id`；optional:

```ts
/** 后端管理：当前知识库会话 id（经 set_kb_active_session） */
kb_active_session_id?: number | null
```

- [ ] **Step 3: Run config merge tests**

Run: `npm run tauri:test -- merge_keeps_backend_managed_fields`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/config.rs src/api/tauri.ts
git commit -m "feat(kb): persist kb_active_session_id as backend-managed config"
```

---

### Task 4: Tauri commands + frontend API wrappers

**Files:**
- Modify: `src-tauri/src/knowledge.rs` **or** `commands.rs` — prefer thin commands in `knowledge.rs` next to other `kb_*` for cohesion, using `State<DbState>`
- Modify: `src-tauri/src/lib.rs` — register handlers
- Modify: `src/api/tauri.ts` — types + methods

**Interfaces:**
- Produces commands (camelCase args from frontend):
  - `list_kb_projects` → `Vec<KbProject>`
  - `create_kb_project(name: String)` → `KbProject`
  - `rename_kb_project(id, name)` → `KbProject`
  - `delete_kb_project(id)` → `()`
  - `list_kb_sessions` → `Vec<KbSession>`
  - `create_kb_session(modelName: Option<String>, projectId: Option<i64>)` → `KbSession` (title `"新对话"`)
  - `rename_kb_session(id, title)` → `KbSession`
  - `delete_kb_session(id)` → `()`; if deleted id == `kb_active_session_id`, clear active in config
  - `pin_kb_session(id, pinned: bool)` → `KbSession`
  - `move_kb_session_to_project(id, projectId: Option<i64>)` → `KbSession`
  - `clear_kb_sessions` → `()`; set `kb_active_session_id = None`
  - `list_kb_messages(sessionId)` → `Vec<KbMessage>`
  - `set_kb_active_session(id: Option<i64>)` → `()` (load/save config)
- Frontend mirrors with camelCase invoke payloads

`create_kb_session` should call `set_kb_active_session(Some(id))` after create (or frontend calls both — **prefer command sets active** to avoid races).

- [ ] **Step 1: Implement commands + register in `lib.rs` next to existing `knowledge::kb_*`**

- [ ] **Step 2: Add TS types + `tauriApi` methods**

```ts
export interface KbProject {
  id: number
  name: string
  sort_order: number | null
  created_at: string
  updated_at: string
}
export interface KbSession {
  id: number
  title: string
  model_name: string
  project_id: number | null
  pinned: boolean
  created_at: string
  updated_at: string
}
export interface KbMessage {
  id: number
  session_id: number
  role: string
  content: string
  citations_json: string | null
  created_at: string
}
```

- [ ] **Step 3: Smoke via existing in-memory unit test calling repo through one command path optional; at minimum `cargo` compile**

Run: `npm run build` (frontend types) and `npm run tauri:test -- kb_chat`
Expected: PASS / build OK

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/knowledge.rs src-tauri/src/commands.rs src-tauri/src/lib.rs src/api/tauri.ts
git commit -m "feat(kb): expose kb session CRUD commands to frontend"
```

---

### Task 5: Multi-turn `kb_ask` (history truncate + persist)

**Files:**
- Modify: `src-tauri/src/knowledge.rs`
- Modify: `src/api/tauri.ts` — `kbAsk(sessionId, question, modelId, topK, onEvent)`

**Interfaces:**
- Consumes: `repo::kb_chat::*`, existing embed/search/`stream_chat`/`build_rag_system_prompt`
- Produces:
  - `pub(crate) fn truncate_kb_history(msgs: &[(String, String)], max_messages: usize, max_chars: usize) -> Vec<(String, String)>`
    - `msgs` = chronological `(role, content)` **without** system; already includes the just-inserted user turn
    - Keep a **suffix** such that `len <= max_messages` (40) and sum of content chars `<= max_chars` (12000); drop from the front; prefer not splitting a user/assistant pair awkwardly — if dropping, drop whole messages from front
  - `kb_ask(app, session_id: i64, question, model_id, top_k, on_event)`

**Algorithm for `kb_ask`:**

1. Lock DB: `get_session`; err if missing.
2. If `model_id` non-empty: `set_session_model`.
3. `add_message(..., "user", question, None)`; if title == `"新对话"`, `rename_session` with `auto_title_from_question`.
4. Unlock; run embed + `hybrid_search` (same as today).
5. Lock: `list_messages`; map to `(role, content)`; `truncate_kb_history(&all, 40, 12000)`.
6. Build `Vec<ChatMessage>`: system (RAG or NO_HIT) + truncated history via `chat_msg`.
7. Stream; on success `add_message(..., "assistant", reply, Some(serde_json::to_string(&citations)?))`; `touch_session`; send `Done`.
8. On stream error: still send `Error` with partial; **do not** insert empty assistant (optional: insert partial assistant — **skip insert on error**, match current non-persist behavior except user row already saved).

- [ ] **Step 1: Write failing unit tests for `truncate_kb_history`**

```rust
#[test]
fn truncate_keeps_last_messages_by_count() {
    let msgs: Vec<(String, String)> = (0..50)
        .map(|i| {
            (
                if i % 2 == 0 { "user" } else { "assistant" }.into(),
                format!("m{i}"),
            )
        })
        .collect();
    let out = truncate_kb_history(&msgs, 40, 100_000);
    assert_eq!(out.len(), 40);
    assert_eq!(out[0].1, "m10");
    assert_eq!(out.last().unwrap().1, "m49");
}

#[test]
fn truncate_binds_on_char_budget() {
    let msgs = vec![
        ("user".into(), "a".repeat(5000)),
        ("assistant".into(), "b".repeat(5000)),
        ("user".into(), "c".repeat(5000)),
        ("assistant".into(), "d".repeat(100)),
    ];
    let out = truncate_kb_history(&msgs, 40, 12_000);
    assert!(out.len() < 4);
    assert_eq!(out.last().unwrap().1, "d".repeat(100));
    let chars: usize = out.iter().map(|(_, c)| c.chars().count()).sum();
    assert!(chars <= 12_000);
}
```

- [ ] **Step 2: Run RED → implement `truncate_kb_history` → GREEN**

- [ ] **Step 3: Rewrite `kb_ask` signature and body** as above; update `tauriApi.kbAsk`:

```ts
kbAsk: (
  sessionId: number,
  question: string,
  modelId: string,
  topK: number | null,
  onEvent: (e: KbAskEvent) => void,
) => {
  const channel = new Channel<KbAskEvent>()
  channel.onmessage = onEvent
  return invoke<void>('kb_ask', { sessionId, question, modelId, topK, onEvent: channel })
},
```

- [ ] **Step 4: Run**

Run: `npm run tauri:test -- truncate_`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/knowledge.rs src/api/tauri.ts
git commit -m "feat(kb): multi-turn kb_ask with session history and RAG"
```

---

### Task 6: `KbSessionSidebar.vue` (left rail)

**Files:**
- Create: `src/components/KbSessionSidebar.vue`

**Interfaces:**
- Props: `sessions: KbSession[]`, `projects: KbProject[]`, `activeId: number | null`, `disabled: boolean` (asking)
- Emits: `new`, `select(id)`, `rename(id, title)`, `pin(id, pinned)`, `move(id, projectId: number | null)`, `delete(id)`, `clear-all`, `create-project(name)`, `rename-project(id, name)`, `delete-project(id)`
- Computed partitions inside component:
  - `pinned = sessions.filter(s => s.pinned)` sorted by `updated_at` desc (or trust API order and filter)
  - `recent = sessions.filter(s => !s.pinned && s.project_id == null)`
  - per project: `sessions.filter(s => s.project_id === p.id)` (includes pinned)

UI sketch:
- Button「新对话」
- Section headers: 置顶 / 项目 / 最近
- Project rows: folder icon + name; nested session buttons; 「+」新建项目
- Footer「清空全部」→ emit `clear-all` (parent shows `ConfirmDialog`)
- Context menu or hover actions for rename/pin/move/delete (reuse `ContextMenu.vue` if practical; else compact `···` menu with `setTimeout(0)` open pattern from Suda)

- [ ] **Step 1: Scaffold component with partitions + emit wiring (no styles polish yet)**

- [ ] **Step 2: Basic scoped styles** (~240px wide, section labels `--text-3` micro, active row `--bg-card-soft` / brand tint, design tokens only)

- [ ] **Step 3: Commit**

```bash
git add src/components/KbSessionSidebar.vue
git commit -m "feat(kb): add KbSessionSidebar left rail"
```

---

### Task 7: Wire sessions into `KnowledgeView` (restore + ask + timeline)

**Files:**
- Modify: `src/components/KnowledgeView.vue`

**Interfaces:**
- Consumes: Task 4–6 APIs; existing `kbAskDecorate` / `renderMarkdown` / citation UI
- State: `projects`, `sessions`, `activeSessionId`, `messages` (parsed: for assistant, `citations: Citation[]` from `JSON.parse(citations_json || '[]')`), `asking`, streaming buffer on last assistant

**Mount / restore:**

```ts
async function bootstrapSessions() {
  if (!isTauri()) return
  projects.value = await tauriApi.listKbProjects()
  sessions.value = await tauriApi.listKbSessions()
  const cfg = await tauriApi.getUiConfig()
  const want = cfg.kb_active_session_id ?? null
  const ok = want != null && sessions.value.some((s) => s.id === want)
  if (ok) await openSession(want!)
  else if (sessions.value.length) {
    // prefer first recent-like: first in list that is !pinned && !project, else sessions[0]
    const recent = sessions.value.find((s) => !s.pinned && s.project_id == null)
    await openSession((recent ?? sessions.value[0]).id)
  } else {
    activeSessionId.value = null
    messages.value = []
  }
}

async function openSession(id: number) {
  if (asking.value) { showToast('生成中，请稍候'); return }
  activeSessionId.value = id
  await tauriApi.setKbActiveSession(id)
  messages.value = (await tauriApi.listKbMessages(id)).map(parseMsg)
  const s = sessions.value.find((x) => x.id === id)
  if (s?.model_name) selectedModel.value = displayModelName(s.model_name) || selectedModel.value
}
```

**Submit:**

1. Ensure session: if `!activeSessionId` → `createKbSession({ modelName: selectedModel })` then refresh list.
2. Optimistic push user message; clear input; `asking=true`.
3. `tauriApi.kbAsk(sessionId, q, selectedModel, askTopK, handler)`.
4. On chunk: update streaming assistant bubble.
5. On done: refresh `listKbMessages` + `listKbSessions` (title/updated_at); stop asking.
6. On error: show error on bubble; asking=false.

Replace single `askedOnce`/`streamText` card with `v-for="m in messages"` timeline; assistant rows reuse citation/clarify decorate on `m.content` + `m.citations`.

Keep index bar + embed dialog unchanged at top.

- [ ] **Step 1: Integrate sidebar + bootstrap + openSession + create/delete/pin/move/project handlers**

- [ ] **Step 2: Replace ask UI with message list + `submitAsk` using `sessionId`**

- [ ] **Step 3: `npm run build` typecheck**

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/components/KnowledgeView.vue
git commit -m "feat(kb): session restore and multi-turn timeline in KnowledgeView"
```

---

### Task 8: Doubao-style bottom composer + clear-all confirm

**Files:**
- Modify: `src/components/KnowledgeView.vue`
- Modify: `src/components/KbSessionSidebar.vue` (if needed for spacing)

**Layout CSS (structure):**

```
.kb-body { display: grid; grid-template-columns: 240px minmax(0,1fr); flex: 1; min-height: 0; }
.kb-chat { display: flex; flex-direction: column; min-height: 0; }
.kb-messages { flex: 1; overflow: auto; padding: … }
.kb-composer-dock { flex-shrink: 0; padding: 12px 16px 16px; border-top: 1px solid var(--border-soft); }
.kb-composer-box { border-radius: var(--radius-lg); /* large pill-ish */ background: var(--frost-surface); … }
.kb-composer-box textarea { border: 0; resize: none; max-height: ~160px; }
.kb-send { /* solid brand pill, icon Send */ }
```

Move model + Top-K selects into `.kb-composer-dock` above the box (not above messages).

Add `ConfirmDialog` for clear-all:

```vue
<ConfirmDialog
  :visible="clearOpen"
  title="清空全部会话？"
  message="将删除所有知识库对话记录，项目文件夹会保留为空。"
  confirm-text="清空"
  tone="danger"
  @confirm="onClearConfirm"
  @cancel="clearOpen = false"
/>
```

Autosize textarea like `ChatPanel` (`scrollHeight` clamp).

User bubble right-aligned; assistant left; empty state centered short copy:「基于你的笔记提问」.

- [ ] **Step 1: Implement layout + composer styles + ConfirmDialog**

- [ ] **Step 2: Visual/manual check checklist** (dev build when executing): left rail partitions, bottom input, switch view away/back keeps session+model

- [ ] **Step 3: `npm run build`**

- [ ] **Step 4: Commit**

```bash
git add src/components/KnowledgeView.vue src/components/KbSessionSidebar.vue
git commit -m "feat(kb): Doubao-style bottom composer and clear-all confirm"
```

---

### Task 9: PRD F4.6 revision

**Files:**
- Modify: `docs/knowledge-base/PRD-个人知识库系统.md`

- [ ] **Step 1: Replace F4.6 bullet** with:

```markdown
- **F4.6 会话形态**：知识库问答为**多轮会话**（独立表 `kb_projects` / `kb_sessions` / `kb_messages`，与 AI 对话 `chat_*` 分离）。左栏提供置顶 / 项目分组 / 最近；支持单条删除与清空全部。每轮提问独立 RAG 检索，并将本会话历史（截断）注入模型以实现追问。切换其他功能再返回时恢复上次会话及其对话模型；Top-K 使用全局嵌入默认。
```

Also fix §1 / any “不做对话历史与知识库问答的合并” line — keep “不合并到 AI 对话”， clarifymulti-turn is KB-local.

- [ ] **Step 2: Commit**

```bash
git add docs/knowledge-base/PRD-个人知识库系统.md
git commit -m "docs(kb): update PRD F4.6 for multi-turn sessions"
```

---

## Spec coverage (self-review)

| Spec requirement | Task |
|------------------|------|
| Independent `kb_*` tables | 1–2 |
| Projects as folders; delete nulls project_id | 2 |
| Pin + project dual listing; recent filter | 6–7 |
| Single delete + clear all + confirm | 4, 6–8 |
| Model per session; Top-K global | 5, 7 |
| `kb_active_session_id` backend-managed | 3–4, 7 |
| Multi-turn RAG + truncate 20 / 12k | 5 |
| Bottom composer Doubao layout | 8 |
| Citations per assistant turn (reuse polish) | 7 |
| Allow leave view while generating; refresh on return | 7 (`onMounted`/bootstrap refresh) |
| PRD F4.6 | 9 |
| No chat merge / no Ctrl+1–9 / no project system prompt | Global constraints (out of scope) |

**Placeholder scan:** none intentional.

**Type consistency:** `sessionId` / `projectId` / `modelName` camelCase on wire; Rust `session_id` / `project_id` / `model_name` in structs; `pinned: bool` in TS ↔ INTEGER in SQL.
