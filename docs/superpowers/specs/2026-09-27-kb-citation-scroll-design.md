# 设计：知识库引用来源跳转原文位置（字级）

- 日期：2026-09-27
- 状态：对话已确认（精度 C + 方案 1：索引时写入原文偏移）
- 关联：
  - `docs/superpowers/specs/2026-09-25-kb-rag-mvp-design.md`（分块 / 问答 / Citation）
  - `docs/superpowers/specs/2026-09-26-kb-chunk-utf8-strip-design.md`（去标记须按 Unicode）
  - `src-tauri/src/repo/knowledge.rs`（`chunk_markdown`）
  - `src/components/KnowledgeView.vue` / `src/index/index.vue` / `src/components/NoteEditor.vue`

## 1. 问题

知识库提问后，「来源」点击仅 `emit('open-note', noteId)`，速记打开在文档顶部，无法落到回答所引用的片段。

根因：

- 打开协议只有笔记 id，无定位信息。
- `kb_chunks` 存 `heading` + 去 Markdown 后的 `content`，**没有**相对 `notes.content` 的偏移。
- 去标记后的 `content` 与编辑器原文不完全一致，不能靠字符串硬搜充当可靠字级定位。

## 2. 目标与范围

### 2.1 交付

| 项 | 说明 |
|----|------|
| 精度 | 字级：滚到片段在原文中的起止附近，短暂高亮选区 |
| 索引 | 分块时写入原文 UTF-8 字节偏移；嵌入仍用去标记 `content` |
| 打开 | 点来源 / 答案内 `[n]` → 打开速记并定位 |
| 迁移 | 旧 chunk 偏移为「未知」；重建或该篇增量索引后生效 |
| 文案 | 「重建索引」旁提示：要用来源跳原文位置，请重建一次（或编辑保存触发该篇增量）；可与乱码修复提示合并为一条 |

### 2.2 不做

- 改块长 / Top-K / 检索 / 嵌入模型
- 启动时静默全量重建
- 为定位单独开预览只读窗
- 保证分屏源码模式的字级选区（以所见即所得为主）

## 3. 架构

```mermaid
flowchart LR
  save[笔记保存] --> hook[kb_hooks 增量索引]
  hook --> chunk[chunk_markdown 带偏移]
  chunk --> db["kb_chunks.md_start / md_end"]
  ask[kb_ask] --> cite[Citation 含偏移]
  cite --> click[点来源]
  click --> open[打开速记]
  open --> editor[NoteEditor 选区 + scrollIntoView]
```

口径：

- 偏移相对索引时的 `notes.content`（UTF-8 **字节**下标，半开区间 `[start, end)`）。
- 偏移对齐的是切块前**原文区间**（可含 `**`、链接等）；嵌入输入仍为去标记文本。
- 保存后走现有 `kb_hooks::on_notes_changed` → `index_note_async`，该篇偏移随增量更新。
- 全库重建进行中时增量仍跳过（保持现状）；重建结束后 chunk 带新偏移。

## 4. 库表与分块偏移

### 4.1 Schema

`kb_chunks` 追加两列（`ensure_column` 幂等迁移）：

| 列 | 类型 | 含义 |
|----|------|------|
| `md_start` | `INTEGER NOT NULL DEFAULT -1` | 原文 UTF-8 字节起点；`-1` = 旧数据/未知 |
| `md_end` | `INTEGER NOT NULL DEFAULT -1` | 原文 UTF-8 字节终点（半开）；同上 |

`Citation` / `kb_ask` 返回体带 `md_start` / `md_end`（无效时前端视为 `null`）。历史消息 JSON 缺字段视为无偏移。

旧 chunk **不**回填数值；用户点「重建索引」或编辑保存触发该篇增量后才有有效偏移。

### 4.2 分块算法（不换切块策略，只补偏移）

改 `src-tauri/src/repo/knowledge.rs` 的 `chunk_markdown`：

1. **`parse_segments`**：每段记下整篇 `md` 上的 `[seg_start, seg_end)`。按行扫描用累计字节下标；`str::lines()` 不含 `\n`，拼回区间时必须把换行算进偏移。
2. **去标记**：对段内原文 `strip` 时同步建**对齐表**（去标记串下标 → 原文字节下标）。禁止「先整段 strip 再猜位置」。
3. **长段切开**（`split_long_text`）：在去标记串上切，经对齐表映回原文 `[piece_start, piece_end)`。
4. **合并进同一 chunk**：`md_start` = 第一片起点，`md_end` = 最后一片终点（中间可含空行；高亮略宽可接受）。
5. **代码块**：不 strip，偏移即段区间；超长切开保持连续偏移。

校验：`0 ≤ md_start < md_end ≤ md.len()`；失败则该 chunk 写 `-1/-1`，不阻断索引。

### 4.3 索引写入

`replace_note_chunks` / 行元组扩展携带 `md_start`/`md_end`；嵌入成功或失败路径都写入偏移（与 content/heading 同生命周期）。

## 5. 前端打开与高亮

### 5.1 协议

1. `KnowledgeView.vue`：`emit('open-note', { noteId, mdStart, mdEnd })`；无效偏移时只传 `noteId`（行为与现网一致）。
2. `index.vue`：切 `notes`、选中笔记；经 `ref` 或短时 `pendingReveal`（`{ noteId, mdStart, mdEnd }`）交给编辑器。
3. `NoteEditor.vue`：Crepe **挂载且正文已加载后**再定位（`flush: 'post'` + 就绪门闩）。

### 5.2 定位算法（所见即所得）

1. UTF-8 工具按字节切出 `notes.content` 的 `[mdStart, mdEnd)` → `rawSlice`（**禁止**用 JS `string.slice` 直接吃字节下标）。
2. 将 `rawSlice` 转为可匹配纯文本（口径接近现有去标记：剥链接留文案、剥强调），在 ProseMirror `doc` 文本中查找。
3. 命中 → `TextSelection` + `scrollIntoView`；短暂高亮（Decoration / CSS，约 2s 后清除）。
4. 多处命中：优先取文档中第一次出现；若同时传入 `heading` 且能解析到对应标题节点，则在多处命中中取「最靠近该标题」的一处。

### 5.3 失败回退（静默，不弹错）

| 情况 | 行为 |
|------|------|
| `md_start`/`md_end` 无效或越界 | 只打开文档 |
| 纯文本搜不到 | 尝试滚到 `heading` 链最后一级标题；再失败则只打开 |
| 用户正在编辑同一篇且 dirty | 仍定位（基于当前 doc / `localContent`）；不强制先保存 |
| 历史 Citation 无偏移 | 只打开笔记 |

### 5.4 类型

`src/api/tauri.ts` 的 `Citation` 增加 `md_start` / `md_end`（`number | null`）。

## 6. 错误处理与测试

### 6.1 错误 / 降级

| 场景 | 处理 |
|------|------|
| 偏移校验失败 | 该 chunk `-1/-1`，索引其余步骤照常 |
| 重建中增量跳过 | 保持现状 |
| 嵌入失败 | 仍写 heading/content/偏移 |
| 打开定位失败 | §5.3 静默回退，无 toast 刷屏 |

### 6.2 测试

- **Rust**（`repo/knowledge.rs`）：中文 + `**`/`[]()` 偏移对齐；`md` 切片含标记原文且去标记 `content` 正确；长段多片不越界/不重叠；迁移后缺列默认 `-1`。
- **前端**：UTF-8 字节切片工具可单测；编辑器选区以手测清单为准——点带来源 → 打开并高亮；无效偏移 → 仅打开。

### 6.3 验收

1. 重建（或编辑保存）后提问 → 点「来源」→ 速记打开且视口落在引用附近，文字高亮约 2 秒。
2. 未重建的旧索引：点来源仍能打开笔记，不报错。
3. `npm run tauri:test` 中 knowledge / chunk 相关用例（含新增偏移用例）通过。

## 7. 风险

| 风险 | 缓解 |
|------|------|
| Crepe 纯文本与 Markdown 切片不一致 | 标题回退；对齐表单测 |
| JS UTF-16 vs UTF-8 字节 | 专用切片工具 |
| 合并 chunk 高亮偏宽 | 可接受；验收以「能看见引用句」为准 |
| 只改代码不重建 → 旧 chunk 无偏移 | 文案提示；增量保存可修单篇 |
