# 设计：Fork 双轨分支策略（上游镜像 + 知识库产品线）

- 日期：2026-09-23
- 状态：已评审（对话确认 §1–§4）
- 仓库：`lzq-197/x-hub`（fork，MIT 上游）
- 关联：`docs/knowledge-base/PRD-个人知识库系统.md`、`docs/knowledge-base/开发文档-技术设计.md`

## 1. 目标与约束

在 fork 上长期跟进上游 MIT 仓库的同时，按知识库 PRD（M1–M6）实现自有功能，且：

- 上游镜像不被产品改动污染
- 知识库按依赖拆成两段功能分支合入
- 发版落在 fork，不依赖上游替你发 Release

## 2. 已确认决策

| 决策点 | 结论 |
|--------|------|
| 与上游关系 | 长期跟上游（定期 merge） |
| `master` 角色 | 上游镜像，不直接合产品代码 |
| 集成主线 | `develop` |
| 知识库分支拆分 | 两段：`feature/kb-notes-structure`（≈M2+M4）+ `feature/kb-rag`（≈M3+M5） |
| 拓扑方案 | 双轨 + 两段功能（不引入长单分支；首版不引入 `release/*`） |

## 3. Remote 与分支角色

| 名字 | 角色 | 规则 |
|------|------|------|
| `upstream` | 上游 MIT 原仓库 | 只读 fetch；默认不 push（除非日后正式提 PR） |
| `origin` | 本 fork（`https://github.com/lzq-197/x-hub.git`） | 所有自有分支的 push 目标 |
| `master` | 上游镜像 | 只合 `upstream` 默认分支；禁止合知识库/产品改动 |
| `develop` | 产品集成主线 | 基于 fork 当前 `master`（v0.6.5 基线）创建；文档与功能最终合入点 |
| `feature/kb-notes-structure` | 知识库第一段 | 文件夹 CRUD + Markdown 导入 + 速记侧栏树 UI |
| `feature/kb-rag` | 知识库第二段 | 分块/嵌入/向量检索/RAG 问答/知识地图与标签云 |
| M1（Ollama） | 本机环境 | 不占 Git 分支 |

**待补：** `upstream` 的准确 Git URL（对话时本机无 `gh`，未能自动探测 fork parent）。登记 remote 前由维护者填入。

## 4. 一次性初始化

1. `git remote add upstream <UPSTREAM_URL>`
2. 从当前 `master` 创建并推送 `develop`：`git checkout -b develop && git push -u origin develop`
3. 将 `docs/knowledge-base/*` 合入 `develop`（可直接提交，或经短生命周期 `docs/kb-spec` 再合）
4. 从 `develop` 拉 `feature/kb-notes-structure` 开始实现
5. （建议）GitHub 上保护 `master`；可选将 default branch 改为 `develop`（接受首页默认展示产品线）

## 5. 日常工作流

**产品开发：**

1. 从最新 `develop` 拉出或继续 `feature/*`
2. 在功能分支提交并自测
3. 合回 `develop`（本地 merge 或 PR：`feature/*` → `develop`）
4. `git push origin develop`
5. 禁止将功能分支直接合进 `master`

**上游同步（建议：上游发版时 / 大改前 / 至少每 2–4 周）：**

```text
git fetch upstream
git checkout master
git merge upstream/master    # 若上游默认分支名不同则替换
git push origin master

git checkout develop
git merge master             # 冲突在 develop 解决一次
git push origin develop
```

打开中的 `feature/kb-*`：在同步后的 `develop` 上对其 `git merge develop`（推荐 merge，避免改写已推送历史）。

**冲突原则：** 与上游重叠的宿主行为优先对齐上游；知识库专属路径（如 `kb_*`、知识库视图）以本产品为准。

**禁止：**

- 在 `master` 上直接改产品代码并 push
- 将 `develop` 反向 merge 进 `master`（会污染镜像）
- 对已推送的 `master` / `develop` 做 force-push

## 6. 知识库合入顺序与发版

| 顺序 | 动作 | 说明 |
|------|------|------|
| 0 | `docs/knowledge-base/*` → `develop` | PRD + 技术设计先入库 |
| 1 | M1 环境 | 安装 Ollama、拉取 `bge-m3` 等 |
| 2 | `feature/kb-notes-structure` → `develop` | 可单独验收「结构可用」 |
| 3 | 从已含 structure 的 `develop` 拉 `feature/kb-rag` | 禁止与 structure 长期并行双改同一套 notes/db 基线 |
| 4 | `feature/kb-rag` → `develop` | M6 联调与回归在 `develop` 完成 |
| 5 | 从 `develop` 打 tag 发版 | 例如下一产品版本 `v0.7.0`；Release 挂在 `origin` |

发版版本号同步沿用仓库既有发版清单（README → package.json → tauri.conf → Cargo.toml → AGENTS.md + RELEASE_NOTES）。

功能分支合入并稳定后删除远程/本地 `feature/kb-*`。`develop` 与 `master` 长期保留。

首版不引入 `release/*`；若联调期很长且需同时改 develop，再按需增加发版分支。

## 7. 工作区未跟踪文件处置

| 路径 | 处置 |
|------|------|
| `docs/knowledge-base/` | 纳入版本库，优先合入 `develop` |
| `pnpm-lock.yaml` | 默认不提交（除非明确切换为 pnpm） |
| `.cursor/` | 不提交；应写入 `.gitignore` |
| 本地索引目录（如 `.codegraph/`） | 不提交 |

## 8. 成功标准

1. `git remote` 同时存在 `origin` 与 `upstream`
2. `master` 跟踪上游；`develop` 含产品基线与知识库文档
3. 新功能只从 `develop` 分支拉出，只合回 `develop`
4. §5 上游同步流程可完整走通，且 `master` 无知识库代码
5. 知识库按 structure → rag 两段合入；M6 在 `develop` 验收后打 tag

## 9. 非目标

- 不在本设计中实现知识库功能代码（见知识库 PRD/技术设计）
- 不强制首版使用 `release/*` 或 rebase 同步策略
- 不规定必须向上游提 PR（保留 `upstream` 只读即可）
