# 设计：速记树 — 选中/新建焦点、夹内拖放与隔离排序

- 日期：2026-09-24
- 状态：对话已确认（方案 1）
- 分支：`feature/kb-notes-structure`
- 关联：
  - `docs/superpowers/specs/2026-09-23-kb-notes-explorer-tree-design.md`（资源管理器布局基线）
  - `docs/superpowers/specs/2026-09-23-kb-notes-structure-design.md`（文件夹数据层）

## 1. 目标与范围

修速记左侧树的选中/新建焦点问题，修正排序步进方向，并补齐「拖到文件夹上松手即纳入」的资源管理器行为；文件夹与笔记使用**隔离的** `sort_order`，同夹内始终先夹后笔记。

### 1.1 本段交付

1. **空白清选中**：树滚动区内点在行外空隙 → 取消文件夹选中（`selectedFolderId = null`）。
2. **FolderPlus 跟选中**：无选中 → 树顶新建顶级夹；有选中 → 在该夹下首行插入子夹创建态（并展开父夹）。
3. **新建笔记父级**：无选中夹 / 当前焦点来自顶级笔记 → `folder_id = null`；有选中夹 → 放入该夹。
4. **排序 ▲▼**：上半减小 `sort_order`，下半增大（数字越小越靠前）。
5. **指针拖放（非 HTML5）**：
   - 落在文件夹**行中** → 移入该夹；
   - 落在**行间缝** → 同级重排；根级缝可将嵌套夹升为顶级。
   - 可拖：**文件夹 + 笔记**。
6. **数据**：`reorder_note_folders` 支持任意同 `parent_id` 一组；`notes` 增加独立 `sort_order` + `reorder_notes`。

### 1.2 本段不做

- HTML5 DnD / 恢复笔记 HTML5 拖归档
- 多选拖、跨窗口拖
- 改导入 / 删除升迁 / `source_path` / `list_meta` 冲正文

## 2. 选中与新建

### 2.1 选中语义

| 操作 | `selectedFolderId` |
|------|-------------------|
| 单击文件夹行 | 该夹 id；并切换展开 |
| 单击夹内笔记 | 其 `folder_id`（保留「在此夹新建」） |
| 单击顶级笔记 | `null` |
| 单击树体内空白（非 `.tree-row` / `.edit-row`） | `null`；若正在行内编辑 → 取消编辑且清选中 |

空白**仅**指树滚动容器内、未命中行的空隙（方案 A），不含标签筛选条、树头按钮。

### 2.2 FolderPlus

- `selectedFolderId == null` → `startCreate(null)`，编辑行在树顶。
- `selectedFolderId != null` → `startCreate(id)`，展开该夹，编辑行用已有 `showCreateEditUnder` 挂在该夹下第一行位置。

### 2.3 新建笔记

- 标题栏「＋」：有选中夹 → `emit('create', id)`；否则 → 顶级（现有按名创建条逻辑不变：留空=顶级）。

### 2.4 排序步进

`onSortStepPointer`：`offsetY < mid` → `bumpSort(-1)`（▲）；否则 `bumpSort(+1)`（▼）。下限仍为 1。

## 3. 拖放命中

继续用指针实现（与待办行拖、现有根级夹拖一致），**禁止** HTML5 DnD。

| 落点 | 拖文件夹 | 拖笔记 |
|------|----------|--------|
| 目标文件夹行中部（`drop-target` 高亮） | `move_folder` 父=目标；夹序 = 目标下子夹 `max(sort)+1` | `set_note_folder`；笔记序 = 该夹笔记 `max(sort)+1` |
| 行间缝（插入线） | 同 `parent_id` 兄弟重排；缝在根且源非根 → 先升顶级再按缝插入 | 缝在「笔记段」→ 同夹笔记重排；缝在「文件夹段」→ **移入缝上方那一夹**（笔记序末尾），不在夹与夹之间插笔记行 |
| 落在笔记行中部 | 不作为「移入」；按指针相对行上/下半当作该笔记前后的缝 | 同左 |
| 自身 / 子孙夹 | toast，不写库 | — |

悬停折叠夹约 400ms → 自动展开，便于投入。

Esc / 窗口失焦 → 取消拖拽，不写库。

## 4. 数据模型与渲染

### 4.1 隔离排序

同一父级下：

- 子文件夹用 `note_folders.sort_order`（1…n）
- 直属笔记用 `notes.sort_order`（1…n）
- **两套可同为 1**；互不占用

### 4.2 渲染

```
[子文件夹按 folder.sort_order ASC]
[直属笔记按 note.sort_order ASC]
```

永不夹–笔记交错。拖**夹**入 B → 排在 B 下最后一个子文件夹之后。拖**笔记**入 B → 排在该夹笔记序末尾（界面上仍在全部子夹之后）。

### 4.3 Schema / API

- `notes.sort_order INTEGER NOT NULL DEFAULT 0`（仅列默认；迁移与每次创建都写显式 1…n / max+1，树不依赖 0）  
  迁移：按 `folder_id`（含 NULL）分组，组内按当前 `updated_at DESC` 赋 1…n。
- `reorder_note_folders(ids)`：校验所有 id 同 `parent_id`（含全为根）；写入 1…n。删除「仅根」限制。
- `reorder_notes(ids)`：校验同 `folder_id`；写入 1…n。
- 创建笔记：目标夹（或顶级）下 `max(sort_order)+1`。

前端：`NoteFolderTree` 扩展现有 pointer 拖；`tauriApi` / store 增加 `reorderNotes`；树列表排序改读 `sort_order`。

## 5. 错误边界

- 移入自身/子孙、跨父 reorder 列表、同级重名、深度 > 8 → 现有或新增明确 toast；失败后 `refreshFolders` + 必要 `refreshNotes`。
- 落库失败不以乐观 UI 为最终真相。

## 6. 测试

- Rust：同父 reorder（根 + 嵌套）；拒跨父；笔记 `sort_order` 迁移；`reorder_notes` 同夹。
- 纯函数：步进方向；行中 vs 缝命中（若抽出）。
- 手测：空白清选、FolderPlus 顶/子、拖夹入夹、拖笔记入夹、根缝升顶级、▲▼。

## 7. 架构选择

**就地扩展** `NoteFolderTree` 现有 pointer 拖（方案 1），不新抽 composable、不用 HTML5 DnD。
