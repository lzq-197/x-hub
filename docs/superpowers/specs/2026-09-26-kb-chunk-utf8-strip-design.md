# 设计：知识库分块清洗 UTF-8 乱码修复

- 日期：2026-09-26
- 状态：对话已确认（方案 A）
- 关联：
  - `docs/superpowers/specs/2026-09-25-kb-rag-mvp-design.md`（分块 / 索引路径）
  - `src-tauri/src/repo/knowledge.rs`（`strip_inline_links` / `strip_emphasis`）

## 1. 问题

Markdown 导入后，速记正文中文正常，但知识库问答「来源」snippet 与注入 RAG 的 chunk 正文出现拉丁杂符乱码。

根因：`chunk_markdown` → `strip_markdown` → `strip_inline_links` / `strip_emphasis` 按 **字节** 扫描，并用 `out.push(bytes[i] as char)` 把单个 UTF-8 续字节写成 Latin-1 字符。中文（多为 3 字节）被拆碎写入 `kb_chunks.content`；`notes.content` 不经此路径，故打开文档正常。

## 2. 目标与范围

### 2.1 交付

| 项 | 说明 |
|----|------|
| 修复 | `strip_inline_links`、`strip_emphasis` 改为按 Unicode 标量（`chars` / `char_indices`）扫描与输出 |
| 单测 | 纯中文、中文 + 加粗/链接；断言 chunk `content` 含正确汉字、不含典型乱码形态 |
| 文案 | 知识库「重建索引」旁一句提示：若来源曾乱码，请点一次重建 |

### 2.2 不做

- 替换为第三方 Markdown 去标记库
- 索引时跳过去标记
- 启动时静默全量重嵌（文档多、嵌入成本高）
- 导入侧编码探测（GBK 等）——本 bug 与文件编码无关
- 改块长 / Top-K / 检索算法

## 3. 实现口径

### 3.1 清洗函数

两处函数保持现有语义（剥链接留标签文案、剥 `**`/`~~`/`*`/`_` 强调），仅改遍历方式：

- 用 `char_indices()` 或 `chars()` 前进；禁止对 `as_bytes()[i]` 做 `as char` 写回。
- Markdown 标记均为 ASCII，用 `ch == '['` 等字符比较即可；多字节汉字整字原样 `push`。
- `parse_link_text` 已用 `char_indices`，可复用；注意返回的 `next` 推进量须与新循环的「字符/字节」口径一致（若循环按字节下标，`next` 仍为字节偏移；若循环按字符，须统一）。推荐：**整函数改为字符迭代，偏移一律按字符或一律按 `char_indices` 的字节下标，禁止混用**。

### 3.2 回归测试（`repo/knowledge.rs`）

至少覆盖：

1. 无 Markdown 标记的中文段落 → `content` 等于（或包含）原文中文。
2. `**中文加粗**`、`[中文](http://example.com)` → 去标记后正文仍为正确中文。
3. 现有 ASCII 用例（`chunk_markdown_strips_inline_markdown` 等）保持绿。

### 3.3 已有坏块策略

- **不**自动全库重建。
- 用户点知识库「重建索引」走现有 `kb_rebuild_index`（清 `kb_chunks` + 全量 `index_note_force`）即可修好。
- 之后单篇保存仍走增量索引，该篇会自然重切。
- UI：在重建按钮附近加一句短说明（勿弹模态打断）。

## 4. 验收

1. 导入含中文的 Markdown → 速记打开正文正常。
2. 点「重建索引」完成后，就同一笔记提问 →「来源」snippet 为可读中文。
3. `npm run tauri:test` 中与 `knowledge` / `chunk_markdown` 相关用例通过（含新增中文用例）。

## 5. 风险

| 风险 | 缓解 |
|------|------|
| 只修代码不重建 → 旧乱码仍在库里 | 文案提示；验收步骤含重建 |
| `char_indices` 与字节偏移混用导致 panic / 漏剥标记 | 单测覆盖链接与强调；实现时统一一种下标口径 |
