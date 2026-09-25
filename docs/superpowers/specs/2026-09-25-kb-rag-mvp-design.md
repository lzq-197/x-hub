# 设计：知识库 RAG 端到端最小切片（索引 + 问答 + 来源卡片）

- 日期：2026-09-25
- 状态：对话已确认（方案 A）
- 分支：建议在 `master` 上新开 `feature/kb-rag-mvp`（或继续 kb 系列分支）
- 关联：
  - `docs/knowledge-base/PRD-个人知识库系统.md`（M3 + M5 最小切片）
  - `docs/knowledge-base/开发文档-技术设计.md`（算法与命令形状的单一技术基线）
  - 已完成：文件夹树 / Markdown 导入 / `kb_hooks` stub / `kb_index_note` stub

## 1. 目标与范围

打通「笔记 → 向量索引 → 单轮 RAG 问答 → 引用来源 → 打开笔记」主路径，使 PRD 验收标准 §5.3–5.4 的核心可测。

### 1.1 本段交付

| 能力 | 说明 |
|------|------|
| 库表 | `kb_chunks`、`kb_meta`（DDL 与技术设计 §4 一致） |
| 嵌入 | 新模块 `embed.rs`：OpenAI 兼容 `POST …/embeddings`；默认 `http://127.0.0.1:11434/v1` + `bge-m3`；API Key 存钥匙串 `x-hub-embed` |
| 分块 / 检索 | `repo/knowledge.rs`：Markdown 结构分块、BLOB 小端 f32、余弦、混合检索（技术设计 §5.6） |
| 索引触发 | 保存/新建 2s 防抖增量；导入成功后对变更 note_id 增量；知识库「重建索引」全量（进度写 `kb_meta` + Channel） |
| 问答 | `kb_ask`：检索 Top-K → 注入 system → 复用 `chat::stream_chat` + 现有 `chat_models`；事件 `Chunk` / `Done{answer,citations}` / `Error` |
| 前端 | 侧栏「知识库」+ `KnowledgeView`：问答区、索引状态栏、嵌入设置弹窗（含 Top-K）；来源卡片点击打开速记笔记 |
| 钩子 | 替换 `index_note_stub` / `kb_hooks` 为真索引调度 |

### 1.2 本段不做

- 知识地图、标签云、顶部「总览数字卡」（状态栏保留已索引笔记数 / 片段数 / 模型 / 上次时间即可）
- 来源「定位片段」滚动高亮（仅打开笔记）
- 多轮问答记忆
- 扩展桥新增 `kb.*` 能力（本段不做）
- 新增任何 crate / 前端图表库

### 1.3 环境前提（文档交付，非代码门禁）

本机需 Ollama 可连且已拉 `bge-m3`。未就绪时仍可进入知识库视图；索引/问答走明确失败路径，不影响速记。

## 2. 架构与数据流

```
notes 变更 (saveNote / addNote / import / rebuild)
        │
   kb_hooks → kb_index_note（真实现）
        │ 互斥：全量重建中则增量跳过或返回「索引任务进行中」
        │ 分块 → embed_batch → 写 kb_chunks；更新 kb_meta
        ▼
提问 kb_ask → 混合检索 Top-K → system 注入片段 [1..n]
        │ stream_chat（chat_models / 现有 keyring）
        ▼
Channel: Chunk | Done{ answer, citations, kb_status } | Error
        │
KnowledgeView 流式渲染 + 来源卡片 → 切速记并打开 note_id
```

### 2.1 模块落点

| 单元 | 职责 |
|------|------|
| `src-tauri/src/db.rs` | `migrate` 追加 `kb_chunks` / `kb_meta` |
| `src-tauri/src/embed.rs` | 嵌入客户端 + 连通性测试 |
| `src-tauri/src/repo/knowledge.rs` | 分块、向量存取、混合检索、chunk CRUD |
| `src-tauri/src/knowledge.rs` | **扩展现有文件**（保留导入）：索引 / 状态 / ask / embed 配置命令 |
| `src-tauri/src/kb_hooks.rs` | 调度真索引（去掉 stub 循环） |
| `src-tauri/src/commands.rs` / `lib.rs` | 去掉 stub 注册；注册新命令 |
| `src-tauri/src/config.rs` + `api/tauri.ts` `AppConfig` | `kb_embed_base_url` / `kb_embed_model` / `kb_top_k` |
| `src-tauri/src/models.rs` | `KbChunkHit` / `Citation` / `KbStatus` / `KbAskEvent` 等 |
| `src/components/KnowledgeView.vue` | 知识库主视图 |
| `src/index/index.vue` | `navigation` 增加 `kb`；渲染 `KnowledgeView`；提供打开笔记回调 |
| `src/stores/workbench.ts` | `saveNote`/`addNote` 成功后 2s 防抖 `kbIndexNote` |

命令形状、事件字段、提示词文案以 `开发文档-技术设计.md` §5.4 / §5.6.3 为准（本 spec 不重复粘贴长模板，实现时对照该文档）。

### 2.2 并发与状态

- 全量重建：`kb_meta.status=indexing`，清空 `kb_chunks` 后重写；`AtomicBool`（或等价）防止重入。
- 增量：重建进行中则跳过；单篇失败标记 chunk/`kb_meta.error` 可读中文，不阻塞其它笔记。
- 维度：按嵌入响应实际 `dim` 存储，不硬编码 1024。

## 3. 前端交互

### 3.1 KnowledgeView 布局（本段）

顶栏索引状态 + 问答区（全宽或左栏）；**不渲染**知识地图 / 标签云列。

**顶栏**
- 展示：嵌入模型、片段数、已索引笔记数、上次索引时间、状态灯（idle 灰 / indexing 蓝+进度% / done 绿 / error 红+文案）
- 操作：「重建索引」（indexing 时禁用）、「嵌入设置」

**问答区**
- 模型下拉：`chat_models`；平台条目折叠为「x-hub 平台」（与 `ChatPanel` / `PLATFORM_ENTRY_NAME` 同口径）
- Top-K：可覆盖，默认读 `kb_top_k`（3–10）
- Enter 发送，Shift+Enter 换行；发送至 done/error 前禁用
- 流式答案：`renderMarkdown`（`src/utils/markdownHtml.ts`），禁止裸 `marked`
- 来源卡片：`[n]`、笔记标题、文件夹路径、heading、snippet（截断）；点击 → `activeView='notes'` + 打开该笔记；**无**「定位片段」

**无命中**
- `citations` 为空时展示提示条；模型侧按技术设计无片段 system 提示开头声明

**嵌入设置弹窗**
- base URL、模型名、API Key（脱敏 / 钥匙串）、测试连接、Top-K 保存进配置

**轮询**
- indexing：约 1s；空闲：约 30s；调 `kb_get_status`

**自动索引**
- 前端 `saveNote`/`addNote` 成功 → 2s 防抖 `kbIndexNote`
- 导入：后端对 imported/updated ids 触发增量（与现有 `on_notes_changed` 对齐）
- 索引失败：前端静默；用户在状态栏见 error

## 4. 错误处理

| 情况 | 行为 |
|------|------|
| Ollama / 嵌入不可用 | 索引 → `status=error` + 中文原因；问答 → `Error` 事件 + 引导嵌入设置；速记不受影响 |
| 重建重入 / 重建中增量 | 「索引任务进行中」或跳过增量 |
| 无可用 chat 模型 / 无效 model_id | 发送前校验 + toast |
| 单 chunk 嵌入失败 | 该行 `embedding=NULL` + `embed_error`；不进向量检索；关键词降级仍可补 |
| Key / 隐私 | Key 不进 `app.json`；日志不打印 Key 与过长片段（摘要 ≤ 80 字） |

## 5. 测试与验收

### 5.1 自动化

- Rust：分块标题链、BLOB 往返、余弦、空笔记跳过、混合检索基础排序（可对小 fixture）
- 既有 `import_markdown` / folder 测试保持绿

### 5.2 手测验收（本段）

- [ ] 对 ≥3 篇笔记「重建索引」后片段数 > 0，状态回到 done
- [ ] 针对库内明确知识提问：答案含 `[n]`，来源卡片对应笔记；点击打开该笔记
- [ ] 无相关内容时有明确未命中提示（或模型开头声明）
- [ ] 关闭 Ollama 后重建给出错误 + 可重试；速记仍可用
- [ ] 编辑保存后增量索引：该笔记片段更新
- [ ] 嵌入设置：保存 / 测试连接 / Key 脱敏
- [ ] 回归：文件夹树、导入、普通 AI 对话、全局搜索

## 6. 架构选择

方案 A：按技术设计落地本切片（扩展现有 `knowledge.rs` + 新 `embed.rs` / `repo/knowledge.rs` + `KnowledgeView`）。

不采用：仅后端无 UI（达不到端到端）；引入 sqlite-vss / 新依赖（违反零新增依赖）。

下一段（本 spec 之外）：知识地图 + 标签云 + 总览卡 + 来源「定位片段」。
