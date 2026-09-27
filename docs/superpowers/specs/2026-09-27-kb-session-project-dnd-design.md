# 设计：知识库会话侧栏 — 项目拖放与分区排序

- 日期：2026-09-27
- 状态：对话已确认（方案 1：两列序 + 指针拖放 + `place` 聚合写库）
- 关联：
  - `docs/superpowers/specs/2026-09-27-kb-chat-sessions-design.md`（置顶 / 项目 / 最近分区与 `move`/`pin`）
  - `docs/superpowers/specs/2026-09-24-kb-notes-tree-focus-dnd-design.md`（指针拖放命中模型）
  - `src/components/KbSessionSidebar.vue`、`src-tauri/src/repo/kb_chat.rs`

## 1. 问题与目标

### 1.1 现状

- 空项目展开时固定显示「暂无会话」（`KbSessionSidebar`：`block.sessions.length === 0`）。会话在「最近」而未归入项目时，空态会一直出现，读起来像故障。
- 归属变更仅靠右键「移到项目 / 移出项目」；无拖放。
- 各区列表按 `updated_at DESC`，无手动序。
- 后端已有 `move_kb_session_to_project` / `pin_kb_session`，但无 `sort_order` / 聚合落点命令。

### 1.2 交付

| 项 | 说明 |
|----|------|
| 去掉「暂无会话」文案 | 空项目改为矮投放热区（无字或极淡） |
| 拖放改归属 | 任意会话 → 项目；项目 → 其他项目；拖到「最近」→ `project_id = null` |
| 拖放改置顶 | 拖入置顶区 = 置顶；拖出到最近/项目 = 取消置顶并按落点改 `project_id` |
| 三区手动重排 | 置顶 / 各项目 / 最近各自排序 |
| 双出现独立序 | `pinned=1` 且带 `project_id` 时，置顶序与项目内序互不影响 |
| 菜单保留 | 右键与拖放写库结果一致（菜单移入 → 目标桶末尾） |

### 1.3 不做

- 项目文件夹拖拽排序 / 嵌套项目
- HTML5 DnD、多选拖、拖拽幽灵预览窗
- 与 `chat_*` 会话合并
- 改 RAG / `kb_ask` 检索逻辑

## 2. 方案选择

| 方案 | 结论 |
|------|------|
| **1. `sort_order` + `pin_sort_order` + 指针拖放 + `place_kb_session`** | **采用**：满足独立序与拖放语义；对齐速记树 |
| 2. 单列全局 `sort_order` | 拒绝：无法做到置顶与项目独立序 |
| 3. 序写入 `AppConfig` | 拒绝：与 `kb_*` 真相源分裂，备份/清空易漂移 |

## 3. 数据模型

### 3.1 Schema（幂等迁移）

`kb_sessions` 新增：

| 列 | 含义 |
|----|------|
| `sort_order INTEGER NOT NULL DEFAULT 0` | **归属桶**内序：同一 `project_id`（含 `NULL`）一组；数字越小越靠前 |
| `pin_sort_order INTEGER NOT NULL DEFAULT 0` | **仅置顶区**内序；`pinned=0` 时忽略 |

迁移：按现有分区规则分组，组内按当前 `updated_at DESC` 赋 `1…n`，避免升级后观感乱跳。

### 3.2 分区规则（展示）

| 区 | 成员 | 排序键 |
|----|------|--------|
| 置顶 | `pinned = 1` | `pin_sort_order ASC`，同分 `updated_at DESC` |
| 项目 P | `project_id = P.id`（含已置顶） | `sort_order ASC`，同分时间倒序 |
| 最近 | `pinned = 0 AND project_id IS NULL` | `sort_order ASC`，同分时间倒序 |

置顶与项目并存时会话仍双出现；两套序互不覆盖。

### 3.3 写库语义

- 改 `project_id`：旧桶摘掉并重密序；新桶按落点缝或末尾插入后重写该桶 `sort_order`。
- 置顶：`pinned=1`，按置顶缝或 `pin_sort_order = max+1`。
- 取消置顶：`pinned=0`；若落在项目/最近，同时写 `project_id` 与该桶 `sort_order`。
- 删项目：仍 `ON DELETE SET NULL`；被打回的会话进入「最近」桶末尾并重密序。
- 新建会话：默认进最近（或创建时带的 `projectId`）桶末尾；不自动置顶。
- 清空全部会话：现有行为不变；项目行保留。

## 4. 命令边界

| 命令 | 作用 |
|------|------|
| `place_kb_session` | **聚合落点**（推荐松手只调一次）：事务内改 `pinned` / `project_id` + 重密相关桶序 |
| `reorder_kb_sessions` | `zone: 'pinned' \| 'recent' \| 'project'`；`project` 时必填 `projectId`；校验 ids 均属该区，写回 `1…n`（置顶写 `pin_sort_order`，其余写 `sort_order`） |
| `move_kb_session_to_project` / `pin_kb_session` | 保留；内部改为维护对应序（菜单路径：目标桶末尾） |

`place_kb_session` 目标形态（实现期可微调字段名，语义锁定）：

- 置顶区某缝 / 区空白
- 最近区某缝 / 区头或空白
- 项目 P 文件夹行 / 空项目投放带 / 项目内某缝

失败整单回滚；前端不以乐观 UI 为最终真相，成功后 `list_kb_sessions` 刷新。

前端 `tauriApi` 与 `lib.rs` invoke 清单同步。

## 5. 拖放命中

### 5.1 约束

- 指针拖放（mousedown → move → up），**禁止 HTML5 DnD**。
- 仅在 `KbSessionSidebar` 内；`disabled`（生成中）禁止起拖，toast 同现有。
- Esc / 窗口失焦 → 取消，不写库。
- 折叠项目：悬停约 400ms 自动展开。
- 项目文件夹行本身不拖（项目排序本版不做）。

### 5.2 落点表

| 落点 | 效果 |
|------|------|
| 「最近」区标题或区内空白 | `pinned=0`，`project_id=null`，插入缝或末尾 |
| 置顶区标题 / 空白 / 会话行缝 | `pinned=1`；`project_id` **保持不变**（可与项目双出现）；序按置顶缝 |
| 项目文件夹行中部或空项目矮投放带 | `project_id=P`；若源从**置顶区**拖出到项目 → 同时 `pinned=0`；插入末尾或缝 |
| 项目内会话行缝 | 同项目重排；跨区则先归属再按缝插入 |
| 会话行中部 | 按上/下半当作该行前/后缝（不嵌套） |
| 侧栏外 / 无效 | 取消 |

置顶且在项目中的会话两处各有一行；从哪一行拖，命中规则相同；源行半透明。

### 5.3 与菜单

右键「置顶 / 移到「…」/ 移出项目」保留；写库结果与等价拖放一致（无缝信息时落到目标桶末尾）。

## 6. UI 空态与视觉

### 6.1 空项目

- **删除**文案「暂无会话」。
- 展开且无会话时：文件夹下一条矮投放带（约 0.5～1 行高，无字或极淡），不抢视线。
- 拖拽悬停投放带或文件夹行中部 → `drop-target` 高亮（设计令牌，不新增色板）。
- 有会话后投放带消失。

### 6.2 分区空态

| 区 | 行为 |
|----|------|
| 置顶 | 可始终保留区头以便空列表也可投入；或拖近时提供等价落点——实现须保证「拖到置顶」在零置顶会话时仍可用 |
| 最近 | 保留区头；空列表时区头+空白可投（移出项目主落点） |
| 无项目 | 保留「暂无项目 + 新建」 |

### 6.3 拖拽反馈

- 源行半透明 + `grabbing`
- 合法缝：细插入线；合法容器：容器级高亮
- 非法：无插入线，松手取消
- 项目右侧计数 = 该 `project_id` 会话数（含双出现），随列表刷新
- 本版不做跟指针的幽灵预览窗

## 7. 测试与验收

### 7.1 自动化

- 迁移后旧会话各组 `1…n` 稳定
- `place`：进项目 / 换项目 / 到最近（unpin）/ 到置顶（pin）事务完整
- `reorder`：拒跨区 id；置顶只改 `pin_sort_order`
- 删项目 → 会话进最近且序可密
- `npm run build`；相关用例走 `npm run tauri:test`

### 7.2 手动

1. 空项目无「暂无会话」，有矮投放带；拖入后带消失、计数 +1  
2. 最近 → 项目 → 另一项目 → 拖到「最近」移出  
3. 拖到置顶区置顶；再拖到项目/最近取消置顶  
4. 三区各自重排；置顶∩项目双出现时两处序互不影响  
5. 菜单移入/置顶与拖放结果一致  
6. 生成中无法拖；Esc 取消不写库  

## 8. 实现落点

| 层 | 文件（预期） |
|----|----------------|
| 迁移 | `src-tauri/src/db.rs` |
| 模型 | `src-tauri/src/models.rs`（`KbSession` 增字段） |
| Repo | `src-tauri/src/repo/kb_chat.rs`（place / reorder / 密序辅助） |
| 命令 | `knowledge.rs` + `lib.rs` + `api/tauri.ts` |
| UI | `KbSessionSidebar.vue`（空态 + 指针拖）；`KnowledgeView.vue`（刷新列表） |
