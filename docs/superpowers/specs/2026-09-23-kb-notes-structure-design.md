# 设计：知识库结构段（文件夹 + Markdown 导入）

- 日期：2026-09-23
- 状态：已评审（对话确认 §1–§4）
- 分支：`feature/kb-notes-structure`（基于 `develop`）
- 关联：
  - `docs/knowledge-base/PRD-个人知识库系统.md`（本段 ≈ M2+M4）
  - `docs/knowledge-base/开发文档-技术设计.md`（实现细节基线；**删除语义与本段范围以本文为准**）
  - `docs/superpowers/specs/2026-09-23-fork-branch-strategy-design.md`

## 1. 目标与范围

在 x-hub 速记中交付**多级文件夹**与 **Markdown 批量导入**，可单独验收「能归档、能导入」。

### 1.1 本段交付

- 表 `note_folders`；`notes.folder_id` / `notes.source_path`
- 文件夹 CRUD（含防环移动）与笔记归档
- `import_markdown` 递归导入与去重更新
- 速记 UI：文件夹树 + 导入对话框
- 索引钩子 stub：`kb_index_note` 无副作用，供导入/保存调用点预留

### 1.2 本段不做

- `kb_chunks` / `kb_meta` 表、`embed.rs`、混合检索、`kb_ask`
- `KnowledgeView`、侧栏「知识库」导航
- Ollama / 嵌入配置 UI
- 真实增量/全量索引

### 1.3 相对技术设计的产品覆盖

| 主题 | 技术设计原文 | 本段裁定 |
|------|--------------|----------|
| 删除文件夹 | `move_to=None` 级联删除子树笔记 | **不删笔记**；子文件夹升到直接父级；该节点**直属笔记** → `folder_id=NULL`（未分类） |
| 导入后索引 | spawn 真索引 | 调用 stub 钩子 |
| 知识库视图 / kb 表 | 同文档后半 | 下一段 |

## 2. 架构

```
NoteList / ImportDialog / workbench store
        ↓ tauriApi
commands: folder CRUD, set_note_folder, import_markdown, kb_index_note(stub)
        ↓
repo/folder.rs + repo/note.rs
        ↓
SQLite: note_folders + notes(folder_id, source_path)
```

挂钩：`create_note` / `update_note` / `import_markdown` 成功后 → `kb_hooks::on_notes_changed(ids)` → stub `kb_index_note`。

工作分支：跟踪或重建 `feature/kb-notes-structure`，合入目标为 `develop`。

## 3. 数据模型

本段迁移**只**包含：

```sql
CREATE TABLE IF NOT EXISTS note_folders (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  parent_id INTEGER REFERENCES note_folders(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
);
-- notes: ensure_column folder_id INTEGER REFERENCES note_folders(id) ON DELETE SET NULL
-- notes: ensure_column source_path TEXT
```

- 存量笔记：`folder_id IS NULL` = 未分类，无迁移向导
- **不**创建 `kb_chunks` / `kb_meta`

### 3.1 `ON DELETE CASCADE` 与升迁删除

`parent_id` 带 `ON DELETE CASCADE`。删除实现必须先搬迁子行再删父行，避免误删子孙文件夹：

1. 校验文件夹存在
2. `UPDATE note_folders SET parent_id = <被删节点的 parent_id> WHERE parent_id = :id`
3. `UPDATE notes SET folder_id = NULL WHERE folder_id = :id`（或依赖随后 DELETE 的 `ON DELETE SET NULL`，择一并测）
4. `DELETE FROM note_folders WHERE id = :id`

根级删除：子文件夹 `parent_id` 更新为 `NULL`。

## 4. 后端命令

| 命令 | 行为 |
|------|------|
| `list_note_folders` | 扁平列表；含 `notes_count` / `subtree_notes`（与技术设计模型一致） |
| `create_note_folder(parent_id, name)` | name trim 非空；父存在；建议深度 ≤ 8 |
| `rename_note_folder` | 同上 |
| `move_note_folder(id, new_parent_id)` | 防环：禁止移到自身或子孙 |
| `delete_note_folder(id)` | **固定** §3.1 升迁语义。若保留 `move_to: Option<i64>`：仅接受 `None`；`Some(_)` → 明确中文错误「本版本不支持迁移删除」 |
| `set_note_folder(note_id, folder_id)` | `None` = 未分类 |
| `list_notes` / `get_initial_data` | `Note` 带 `folder_id` / `source_path`（serde default） |
| `list_notes_by_folder` | 可选；前端本地按 `folder_id` 过滤即可则不加 |
| `import_markdown` | 见 §5 |
| `kb_index_note(note_id)` | stub：`Ok(())`，无 IO |

错误风格：`Result<T, String>`，可读中文。注册进 `lib.rs` invoke_handler；前端一律经 `tauri.ts`。

模块落点（与技术设计对齐，可微调）：

- `repo/folder.rs`（新建）
- `repo/note.rs` 扩展
- `models.rs`：`NoteFolder`、`ImportResult`、扩展 `Note`
- `knowledge.rs`：导入 + stub（或 `kb_hooks` 小模块）
- `commands.rs` / 分组注册文件夹命令
- `db.rs`：`migrate` + `ensure_column`

## 5. Markdown 导入

1. 递归扫描；跳过隐藏；仅 `.md` / `.markdown`；目录深度 ≤ 8
2. 相对根路径 → 逐级 `find_or_create` 文件夹；文件名去扩展名 → 标题；内容 → content
3. 导入键 = `source_path`（正斜杠归一）
   - 已存在 → 更新标题/内容；目录变了则改 `folder_id`；`updated++`
   - 不存在 → 新建；`imported++`
   - 未知格式 → `skipped`；单文件 > 2MB → `failed`
4. 候选 > 2000 → 截断并说明
5. 返回 `ImportResult { imported, updated, skipped, failed, total, errors（≤20）}`
6. 可选进度 Channel；UI 至少 Toast 汇总
7. 成功后对 imported+updated id 走索引钩子（stub）
8. 部分失败不整单回滚

手工速记：`source_path = NULL`，可归档进文件夹。

## 6. 索引钩子（stub）

| 触发 | 行为 |
|------|------|
| `import_markdown` 末尾 | `on_notes_changed(ids)` |
| `create_note` / `update_note` 成功后 | 同上（后端挂接） |
| stub 实现 | 立即成功；不写库、不访问网络 |

RAG 段只替换实现，不改调用点。前端可不调 `kbIndexNote`（由后端挂钩）。

## 7. 前端

| 文件 | 动作 |
|------|------|
| `src/api/tauri.ts` | 扩展 Note；文件夹 + 导入 + `kbIndexNote` |
| `src/stores/workbench.ts` | `folders` 与 CRUD/导入/刷新；不加 kbAsk/Knowledge 状态 |
| `src/components/NoteList.vue` | 左树（全部 / 未分类 / 文件夹）+ 右列表；右键；拖拽归档 |
| `src/components/ImportDialog.vue` | 新增 |
| `src/components/NoteEditor.vue` | 可选只读文件夹路径 |
| `src/index/index.vue` | **不加**知识库导航；打开笔记时尽量展开父链 |
| `ConfirmDialog` | 删除确认 |

删除确认文案：

> 将删除文件夹「X」。其下子文件夹会移到上一级；该文件夹内的 N 篇笔记将变为未分类。笔记不会被删除。

错误：后端中文 Err → Toast；导入 `errors` 可展示；防环提示清晰；stub 异常仅 log，不挡保存/导入。

## 8. 测试与验收

### 8.1 自动化

- `repo/folder`：升迁删除、防环、创建重名策略（若有）
- 导入：同 `source_path` 更新不重复；目录变化迁文件夹
- 使用 `db::init_in_memory`

### 8.2 产品验收

1. 多级文件夹 CRUD + 防环移动
2. 删除：子文件夹升父级、直属笔记→未分类、笔记保留
3. 导入还原树、统计正确、再导入更新不重复
4. 全部 / 未分类 / 文件夹筛选正确；存量在未分类
5. 无知识库侧栏、无 Ollama 硬依赖；`kb_index_note` 可调且无副作用
6. 速记编辑、标签、全局搜索回归通过

## 9. 非目标与后续

- RAG / 知识地图 / 嵌入配置 → 另开 `feature/kb-rag` 与独立 spec
- 删除时「迁移到指定文件夹」UI → 可后续加；本段 API 不实现 `move_to=Some` 行为
- 发版版本号 → 结构段合入 `develop` 并与 RAG 或单独里程碑再定

## 10. 成功标准（本段结束时）

1. `feature/kb-notes-structure` 合入 `develop` 后，速记可用文件夹与导入
2. `master` 仍无本产品提交（双轨策略不变）
3. 无 `kb_chunks` 迁移；无 Knowledge 导航
4. 技术设计中与本段冲突的删除/索引描述，以实现与本文为准（可在 RAG 段回写技术设计差量）
