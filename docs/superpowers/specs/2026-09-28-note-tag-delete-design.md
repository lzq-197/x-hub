# 设计：速记标签删除与最后一篇孤儿确认

- 日期：2026-09-28
- 状态：对话已确认（方案 2）
- 关联：
  - `src-tauri/src/repo/tag.rs`（`delete` / `set_note_tags`）
  - `src-tauri/src/db.rs`（`note_tags.tag_id … ON DELETE CASCADE`）
  - `src/components/NoteList.vue`（标签筛选条）
  - `src/components/NoteEditor.vue`（底部标签行）
  - 对齐参考：`TodoView.vue` 删标签确认文案与 `ConfirmDialog` 用法

## 1. 问题

速记侧栏标签 pill 目前只能筛选，没有删除入口。用户需要：

1. 能删除标签定义；
2. 删除后，所有笔记上该标签的关联一并摘掉；
3. 笔记本身不删；其它标签保留；
4. 在编辑器底部从某篇笔记摘掉标签时，若该篇是该标签的最后一篇关联，询问是否同时删除标签定义。

后端 `delete_tag` 与 FK `ON DELETE CASCADE` 已覆盖「删定义 → 级联摘关联」；缺的是侧栏入口、用法计数，以及编辑器「最后一篇」确认流。

## 2. 目标与范围

### 2.1 交付

| 项 | 说明 |
|----|------|
| 侧栏删除 | 标签 pill 右键 →「删除标签」→ 确认 → `delete_tag` |
| 级联语义 | 定义删除后全库该标签关联清空；笔记行与其它标签不动 |
| 用法计数 | 新增 `tag_note_count(tag_id) -> number`（或等价 COUNT 查询 + 命令封装） |
| 编辑器孤儿确认 | 底部 × 摘标签前查 count；`count === 1` 时弹确认 |
| 确认 / 取消 | 确认 = 摘关联 + 删定义；取消 = **只摘关联**，定义保留（空标签仍出现在筛选条） |
| 状态同步 | 删定义后更新 `state.tags`、刷新 note-tags 映射；若当前筛的是该标签则回「全部」 |
| 单测 | `delete` 级联摘关联且笔记仍在；`tag_note_count` 覆盖 0/1/多 |

### 2.2 不做

- 删笔记时的孤儿标签清理弹窗
- 标签重命名 / 合并
- 改动待办标签体系（`todo_tags` 已有独立删除流）
- 表结构迁移（现有 CASCADE 已足够）
- 撤销栈 / 软删除

## 3. 实现口径

### 3.1 数据层

- 表结构不变：`note_tags.tag_id REFERENCES tags(id) ON DELETE CASCADE`。
- `repo/tag.rs::delete` 保持 `DELETE FROM tags WHERE id = ?`；关联由 FK 级联清除。
- 新增 `repo/tag.rs::note_count(conn, tag_id) -> i64`：`SELECT COUNT(*) FROM note_tags WHERE tag_id = ?1`。
- 新命令 `tag_note_count` → 前端 `tauriApi.tagNoteCount(id)`；在 `lib.rs` / `api/tauri.ts` 登记。
- 「从本笔记摘标签」继续用现有 `set_note_tags`（全量替换，写入不含该 id 的列表）。

### 3.2 侧栏右键删除（`NoteList.vue`）

- 「全部」不提供删除。
- 具体标签：左键筛选不变；右键打开 `ContextMenu`（`setTimeout(0)` 置位，同速达 / 文件夹树，避免全局关闭监听立刻关掉菜单）。
- 菜单项：仅「删除标签」。
- `ConfirmDialog`（对齐待办）：
  - 标题：`删除标签`
  - 正文：`「{名}」会从所有笔记上摘掉，标签本身也会删除。`
  - hint：`这个操作不能撤销。`
  - tone：`danger`；确认文案：`删除标签`
- 确认后：`store.deleteTag(id)`；若 `activeTagId === id` 则置 `null`；刷新筛选用的 note-tags 映射。

### 3.3 编辑器底部 ×（`NoteEditor.vue`）

流程：

1. 用户点某 chip 的 ×。
2. 调用 `tag_note_count(tagId)`。
3. 若 `count > 1`：直接从本地 `noteTags` 去掉该 id 并 `persistTags()`，无弹窗。
4. 若 `count === 1`：弹出确认：
   - 标题：`同时删除标签？`
   - 正文：`这是最后一篇使用「{名}」的笔记。是否同时删除该标签？`
   - hint：`取消则只从本笔记摘掉，标签定义会保留。`
   - 确认文案：`同时删除`
   - **确认**：摘关联（`persistTags`）+ `store.deleteTag(tagId)`
   - **取消**：仅摘关联（`persistTags`），定义保留
5. 若计数查询失败：保守路径——只摘关联、不弹窗、不自动 `delete_tag`。

`removeTag` 改为异步并防重入；失败时 toast，chip 回滚到操作前。

### 3.4 Store

- 现有 `deleteTag`：调用后端后从 `state.tags` 过滤掉该 id。
- 调用方负责：刷新 `loadNoteTagsMap`、清掉失效的筛选选中态、编辑器侧若正显示该标签则同步 chip（打开笔记时本就会 `getNoteTags`，同会话内删定义后编辑器本地列表也要去掉该 id）。

### 3.5 错误处理

| 场景 | 行为 |
|------|------|
| `delete_tag` 失败 | toast「标签没能删除，请重试」；UI 不假装已删 |
| `set_note_tags` 失败 | toast「标签更新失败，请重试」；chip 回滚 |
| `tag_note_count` 失败 | 只摘关联，不删定义 |
| 确认弹窗展示期间再次点击 | 忽略（busy） |

## 4. 测试与验收

### 4.1 单测（Rust）

1. 两篇笔记挂标签 A，一篇另挂标签 B；`delete(A)` → A 的关联清空，两篇笔记仍在，B 关联仍在，`tags` 中无 A。
2. `note_count`：无关联 → 0；一篇 → 1；两篇 → 2；删定义后 → 0（或查不到定义）。

### 4.2 手工验收

1. 侧栏右键删「测试」→ 多笔记上该标签消失，其它标签保留，笔记仍在；筛选条无该 pill。
2. 仅一篇笔记使用某标签时，编辑器点 × → 确认「同时删除」→ 定义与 pill 消失。
3. 同上，点取消 → 本笔记 chip 去掉，筛选条仍有该空标签，点选后列表为空。
4. 多篇共用标签时点 × → 无弹窗，只摘本篇。

## 5. 风险

| 风险 | 缓解 |
|------|------|
| 计数与摘关联之间另一窗口改了关联（本地单用户极少） | 可接受；确认后仍以 `delete_tag` / `set_note_tags` 终态为准 |
| 取消后留下永不使用的空标签 | 符合产品选择；用户可随时侧栏右键删掉 |
| ContextMenu 同步置位被立刻关闭 | 必须 `setTimeout(0)`，与仓库约定一致 |
| store 删标签后筛选 / 编辑器不同步 | 调用方统一清 `activeTagId` + 刷新 map + 更新本地 `noteTags` |
