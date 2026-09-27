# 设计：知识库多轮会话 + 豆包式布局

- 日期：2026-09-27
- 状态：对话已确认（方案 1：独立 `kb_*` 会话表）
- 关联：
  - `docs/superpowers/specs/2026-09-25-kb-rag-mvp-design.md`（RAG / `kb_ask`）
  - `docs/superpowers/specs/2026-09-27-kb-ask-ui-polish-design.md`（来源分组 / 引用跳转 / 澄清条，本版消息气泡内复用）
  - `docs/knowledge-base/PRD-个人知识库系统.md`（F4.6 需同步修订）
  - `src/components/KnowledgeView.vue`、`src-tauri/src/knowledge.rs`、`src-tauri/src/repo/chat.rs`（对照模式）

## 1. 问题与目标

### 1.1 现状

- 知识库问答为**单轮、无历史**（PRD F4.6）；答案与模型选择仅活在 `KnowledgeView` 组件内存。
- 侧栏用 `v-else-if` 挂载视图，离开知识库即卸载 → 切回后对话与模型丢失。
- 输入区在顶部，布局与豆包等常见 AI 产品不一致。

### 1.2 交付

| 项 | 说明 |
|----|------|
| 多轮会话 | 真多轮：历史消息进 LLM；每轮独立 RAG 检索 |
| 历史列表 | 左栏：置顶 / 项目（简单分组文件夹）/ 最近；默认打开活跃或最新会话 |
| 清理 | 单会话删除 + 清空全部（二次确认） |
| 状态保持 | 切视图 / 重启后恢复上次会话 + 该会话模型；Top-K 用全局嵌入默认 |
| UI | 底部大圆角输入 + 发送钮；消息流在上；顶栏索引状态保留 |

### 1.3 不做

- Ctrl+1…9 快捷切会话
- 项目级系统提示词 / 协作 / 分享 / 云同步
- 与 AI 对话（`chat_*`）历史合并
- 知识地图 / 标签云（原 P1）
- 会话内搜索、消息编辑、重新生成
- 用 `keep-alive` 代替落盘（可选优化，非本版依赖）

## 2. 方案选择

| 方案 | 结论 |
|------|------|
| **1. 独立 `kb_projects` / `kb_sessions` / `kb_messages`** | **采用**：边界清晰，citations 自然落库，不污染 `ChatPanel` |
| 2. 复用 `chat_sessions` + `kind=kb` | 拒绝：与 PRD「不合并」冲突，字段污染风险高 |
| 3. 仅前端/配置记忆 | 拒绝：无法可靠支撑历史 / 项目 / 清空 |

## 3. 数据模型

### 3.1 表

```sql
CREATE TABLE IF NOT EXISTS kb_projects (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  sort_order INTEGER,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
);

CREATE TABLE IF NOT EXISTS kb_sessions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  title TEXT NOT NULL DEFAULT '新对话',
  model_name TEXT NOT NULL DEFAULT '',
  project_id INTEGER REFERENCES kb_projects(id) ON DELETE SET NULL,
  pinned INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
);

CREATE TABLE IF NOT EXISTS kb_messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id INTEGER NOT NULL REFERENCES kb_sessions(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
  content TEXT NOT NULL DEFAULT '',
  citations_json TEXT,  -- 助手消息：Citation[] 的 JSON；user 为 NULL
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
);

CREATE INDEX IF NOT EXISTS idx_kb_sessions_updated ON kb_sessions(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_kb_messages_session ON kb_messages(session_id, id);
```

- 删项目：`project_id` SET NULL，会话回到「最近」，**不**删会话。
- 删会话：CASCADE 删消息。
- 清空全部：删除全部 `kb_sessions`（消息级联）；**保留**空项目行。
- 新建项目：`sort_order = COALESCE(MAX(sort_order),0)+1`；列表无手动拖拽排序时也可退化为按 `name`。

### 3.2 列表分区规则

| 区 | 条件 | 排序 |
|----|------|------|
| 置顶 | `pinned = 1` | `updated_at DESC` |
| 项目 | 有对应 `kb_projects` 行；其下会话 `project_id = 该 id` | 项目按 `sort_order` / 名；会话 `updated_at DESC` |
| 最近 | `pinned = 0` AND `project_id IS NULL` | `updated_at DESC` |

**置顶与项目并存**：`pinned = 1` 且带 `project_id` 的会话同时出现在「置顶」区与所属项目文件夹内（两边都能找到）。「最近」区不包含已置顶或已归入项目的会话。

### 3.3 配置

- `AppConfig.kb_active_session_id: Option<i64>`（serde default `None`）。
- 必须登记进 `BACKEND_MANAGED_FIELDS` + `merge_disk_authoritative`（经 `set_kb_active_session` 写盘），防止前端整份 `save_config` 冲掉。
- 模型：存在 `kb_sessions.model_name`（按会话）。
- Top-K：继续用 `kb_top_k` 全局默认；提问工具栏可临时改本次参数，**不**写入会话。

### 3.4 标题

- 默认「新对话」。
- 首条 user 落库后，若仍为默认标题 → 截取问题前约 30 字（去换行）自动改名。
- 支持手动 `rename_kb_session`。

## 4. 命令边界

前缀 `kb_`，与 `chat_*` 平行；实现可对照 `repo/chat.rs` / `commands` 会话 CRUD，落在 `repo/kb_chat.rs`（或 `knowledge` 子模块）+ `commands` / `knowledge` 注册。

| 命令 | 作用 |
|------|------|
| `list_kb_projects` / `create_kb_project` / `rename_kb_project` / `delete_kb_project` | 项目 CRUD |
| `list_kb_sessions` | 返回会话列表（含 `pinned` / `project_id` / `model_name` / 时间）；前端分组 |
| `create_kb_session` | 可选 `modelName` / `projectId`；并可选设为活跃 |
| `rename_kb_session` / `delete_kb_session` | 单条 |
| `pin_kb_session(id, pinned)` | 置顶切换 |
| `move_kb_session_to_project(id, projectId?)` | `None` = 移出到最近 |
| `clear_kb_sessions` | 删全部会话 |
| `list_kb_messages(sessionId)` | 按 id 升序 |
| `set_kb_active_session(id?)` | 写 `kb_active_session_id` |
| `kb_ask` | **改为**必填 `sessionId`；签名见 §5 |

前端 `tauriApi` 与 `lib.rs` invoke 清单同步；浏览器预览 `isTauri()` 守卫不变。

**破坏性**：旧「无 session」的 `kb_ask(question, modelId, topK)` 前端路径删除；无兼容层（功能尚未发版依赖外的稳定 API，或仅本机开发期使用）。

## 5. 多轮 RAG（`kb_ask`）

### 5.1 流程

1. 校验会话存在；若传入 `modelId`，更新该会话 `model_name`。
2. INSERT user 消息；必要时自动改标题。
3. **本轮独立检索**（embed → hybrid_search；Top-K = 参数或 `resolve_top_k` 全局默认）。不复用上一轮 hits。
4. 组装 LLM messages：
   - `system`：有命中 → 现有 `build_rag_system_prompt(hits)`；无命中 → `NO_HIT_SYSTEM`
   - 历史：该会话 user/assistant **正文**按时间序（不含 citations）
   - 截断：保留最近 **20 轮**（40 条 message），或按字符预算约 **12k**（先到为准）；system 不计入轮次截断
   - 本轮 user 已是历史最后一条
5. `stream_chat` → Channel `Chunk`；成功则 INSERT assistant（`content` + `citations_json`）→ `Done`（含 answer、citations、message 元数据按需、`kbStatus` 可选）。
6. 更新会话 `updated_at`。

### 5.2 事件形状

在现有 `KbAskEvent` 上保持 `chunk` / `done` / `error`；`done` 仍带 `citations`。可选增加 `session` 快照便于刷新左栏标题/时间——实现时若前端已本地更新可省略。

### 5.3 前端消息 UI

- 气泡时间线：user 右、assistant 左（或对称卡片，对齐设计令牌即可）。
- 每条 assistant 下挂**该轮**来源 UI（复用 ask-ui-polish：分组 / 引用跳转 / 澄清条）。
- 流式：临时 assistant 气泡；`error` + partial 保留。
- 生成中：禁用发送、禁用切会话（或切会话前 toast 拒绝）；**允许**用户切到其他侧栏视图——请求后台继续，回知识库后 `list_kb_messages` 刷新。

## 6. 布局与交互

```
┌─ kb-bar（索引状态 / 重建 / 嵌入设置）─────────────────────┐
├─ 左栏 ~240px ──────┬─ 右栏 ───────────────────────────────┤
│ [+ 新对话]          │  空态短提示 / 消息流                   │
│ 置顶 · …            │  （新消息在下，容器向上滚）            │
│ 项目 · 📁 … ▾       │                                      │
│ 最近 · …（最新在上）│                                      │
│ 清空全部            ├──────────────────────────────────────┤
│                     │  模型 | Top-K                         │
│                     │  ┌ 圆角输入 ────────────── [发送] ┐  │
│                     │  └ Enter 发送 / Shift+Enter 换行 ───┘ │
└─────────────────────┴──────────────────────────────────────┘
```

### 6.1 左栏

- 进入视图：`list` 项目+会话 → 读 `kb_active_session_id` → 有效则打开；否则「最近」第一条；皆无则空态。
- 「新对话」：`create_kb_session` 并设活跃、清空消息区。
- 空态下首次发送：若无活跃会话则先 create 再 ask。
- 项操作：单击切换；悬停/右键菜单 = 重命名 / 置顶 / 移到项目 / 删除。
- 项目：新建、重命名、删除；文件夹默认展开（折叠状态本版可不跨重启持久化）。
- 「清空全部」→ `ConfirmDialog` → `clear_kb_sessions` → 重置活跃 id。

### 6.2 输入区

- 自顶部迁到底部；大圆角容器 + 自适应高度 textarea（约 6–8 行上限）+ 实心发送（图标为主）。
- 模型 / Top-K 在输入框上方一行紧凑 `AppSelect`。
- 样式只用现有设计令牌；壁纸透底与 `.card` 口径一致。

### 6.3 切视图保持

不依赖 keep-alive：落盘 `kb_active_session_id` + SQLite 消息/模型；再次进入按 §6.1 恢复。活跃会话已删则回落最近第一条或空态。

## 7. 文档与 PRD

修订 `docs/knowledge-base/PRD-个人知识库系统.md` **F4.6**：

- 由「单轮、无多轮记忆」改为「多轮会话；独立 `kb_*` 表；与 AI 对话历史分离」。
- 补充：左栏置顶/项目/最近；清理；切视图保持会话与模型。

## 8. 测试与验收

### 8.1 自动化

- repo：项目 CRUD、置顶/移项目、删项目置空 `project_id`、会话级联删消息、清空保留项目。
- `kb_ask` 纯函数级：历史截断（20 轮 / 字符预算）、组消息顺序（system → 历史 → 当前已在历史末）。
- config：`kb_active_session_id` 在 `BACKEND_MANAGED_FIELDS` 合并测试中生效。
- `npm run build`；`npm run tauri:test` 相关用例。

### 8.2 手动

- 多轮追问能理解上文；每轮来源独立。
- 置顶 / 项目移动 / 最近分区正确；单删与清空。
- 默认打开活跃或最新会话。
- 切到其他功能再回：同一会话 + 同一模型；Top-K 为全局默认。
- 底部输入观感接近豆包；索引顶栏仍可用。
- 生成中切走侧栏，回来后答案已落库可见。

## 9. 实现落点（供计划拆分）

| 层 | 文件（预期） |
|----|----------------|
| 迁移 | `src-tauri/src/db.rs` |
| 模型 | `src-tauri/src/models.rs` |
| Repo | `src-tauri/src/repo/kb_chat.rs`（新） |
| Ask | `src-tauri/src/knowledge.rs`（`kb_ask` 签名与历史拼装） |
| 命令注册 | `lib.rs` + `api/tauri.ts` |
| 配置 | `config.rs`（字段 + merge + 清单） |
| UI | `KnowledgeView.vue`（左栏 + 消息流 + 底栏；抽子组件若过大） |
| 确认框 | 复用 `ConfirmDialog` |
| PRD | `docs/knowledge-base/PRD-个人知识库系统.md` |
