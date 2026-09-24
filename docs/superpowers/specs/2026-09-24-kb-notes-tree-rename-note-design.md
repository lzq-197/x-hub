# 设计：速记树内联重命名笔记

- 日期：2026-09-24
- 状态：对话已确认（方案 A）
- 分支：`feature/kb-notes-structure`
- 关联：`NoteFolderTree` 文件夹双击重命名；`NoteEditor` 标题栏

## 1. 目标与范围

树里的**笔记名称**支持与文件夹同款的内联修改：双击进入编辑，右键也有「重命名」。只改标题，不暴露序号步进。

### 1.1 本段交付

- 双击笔记行 → 内联重命名（名称输入 + 确认；无 sort）
- 右键菜单增加「重命名」（建议置顶，其下仍为移动类项）
- 空标题 trim 后落库为「无标题笔记」（与右侧编辑器 `normalizeTitle` 一致）
- 标题-only 后端命令，**绝不改写 `content`**（避免 `list_meta` 空正文误存）
- 当前打开的笔记被树重命名后，编辑器标题在标题框未聚焦时同步

### 1.2 本段不做

- 不为笔记重命名暴露序号步进（排序继续靠拖拽）
- 不改 `update_note` 的 title+content 语义
- 不加 F2 等快捷键（除非实现时文件夹已有且可一行复用）
- 不改列表视图以外的入口（本需求只做树）

## 2. 现状

| 对象 | 双击 | 右键重命名 | 标题落盘 |
|------|------|------------|----------|
| 文件夹 | `onFolderDblClick` → `startRename` + sort 行 | 有 | `rename_note_folder` |
| 笔记 | 无（仅 click 选中） | 无 | 仅 `NoteEditor` → `update_note(title, content)` |

`EditState` 今日只有 `create` / `rename`（folder）。`saveNote` 会写 content，树侧元信息刷新后 content 常为空，不能拿来只改标题。

## 3. 交互

- 双击：`preventDefault` + `stopPropagation`；进入 `rename-note`；若未选中则一并选中该笔记。
- 右键：「重命名」→ 同一 `startRenameNote`。
- 键位：与文件夹一致——Enter / 确认钮提交，Esc 取消；**不做失焦自动提交**。
- 标题未变：退出编辑态，不打 API。
- 拖拽 / suppress click：不进入重命名（沿用现有吞事件）。

## 4. 架构与数据流

```
NoteFolderTree
  EditState += { mode: 'rename-note', note }
  commit → store.renameNote(id, title)
       → tauriApi.renameNote
       → commands::rename_note
       → note::rename (UPDATE title, updated_at only)
```

### 4.1 前端

- `NoteFolderTree.vue`：扩展 `EditState`；笔记行编辑态复用 `edit-row` 样式，隐藏 sort UI。
- `workbench.ts`：`renameNote(id, title)`——先 `normalizeTitle`（空 →「无标题笔记」），再 invoke；就地更新 `state.notes[i].title` / `updated_at`，保留既有 `content`。
- `tauri.ts`：`renameNote(id, title)`。

### 4.2 后端

- `repo/note.rs`：`rename(conn, id, title)`——只更新 `title` + `updated_at`；缺行返回与 `update` 同款 `NOT_FOUND` 前缀错误。
- 空标题归一：在 **store 侧**做（与编辑器一致）；Rust 可再做一道防御性 trim+空串归一，但单一真相文案为「无标题笔记」。
- `commands::rename_note` + `lib.rs` 注册；成功后 `kb_hooks::on_notes_changed(&[id])`（与 `update_note` 同口径）。

### 4.3 与 NoteEditor 同步

今日 `syncLocal` 仅在笔记 `id` 变化时调用。补：`watch(() => props.note?.title)`——当新 title 与 `localTitle` 不同，且标题输入框**未聚焦**时，将 `localTitle` 设为新 title。正文与 content 的 `dirty` 不动。聚焦中不打断用户正在输入。

与「正文派生标题」共存：显式重命名后标题不是空/「无标题笔记」时，现有 `deriveTitleFromContent` 门控不会覆盖。

## 5. 错误处理

| 情况 | 行为 |
|------|------|
| 标题未变 | 取消编辑态，无 API |
| 后端失败 | toast，**留在编辑态** |
| NOT_FOUND | toast，取消编辑态 |
| 网络/非 Tauri | 浏览器预览可本地改 store 标题，或禁用入口（与其它 store 命令一致） |

## 6. 测试与验收

### 6.1 自动化

- Rust：`rename` 改标题、`content` 不变、空串经归一后的标题、NOT_FOUND。
- 前端手测为主（树交互）；不强制新 Vue 单测除非仓库已有同类组件测。

### 6.2 验收清单

- [ ] 双击笔记进入内联改名（无序号步进）
- [ ] 右键有「重命名」
- [ ] 空标题 →「无标题笔记」
- [ ] 改名后正文仍在（尤其 list_meta 刷新后的笔记）
- [ ] 打开态改名：标题框未聚焦时编辑器标题跟上
- [ ] 拖拽松手不会误进重命名

## 7. 架构选择

方案 A：扩展现有 `editing` + 标题-only API。不用 `saveNote(title, content)`（方案 B，有正文被清空风险）；不做成「只聚焦右侧标题框」（方案 C，不符合「和文件夹一样」）。
