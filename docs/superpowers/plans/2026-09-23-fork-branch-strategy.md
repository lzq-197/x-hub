# Fork 双轨分支策略落地 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 fork 上落地「`master` = 上游镜像、`develop` = 产品集成」双轨，并入库知识库规格文档、准备好第一段功能分支。

**Architecture:** `upstream` 只读跟踪原 MIT 仓库；`origin` 推送自有分支。产品与文档只进 `develop`。知识库实现拆成后续独立计划：先 `feature/kb-notes-structure`，再 `feature/kb-rag`。本计划不写业务代码。

**Tech Stack:** Git、GitHub fork（`lzq-197/x-hub`）、PowerShell（Windows）

## Global Constraints

- 禁止向 `master` 合入产品/知识库代码（`master` 只合上游）
- 禁止 `develop` → `master` 反向 merge
- 禁止对已推送的 `master` / `develop` force-push
- 禁止提交 `pnpm-lock.yaml`（未切换包管理器前）
- 禁止提交 `.cursor/` 与本地索引目录
- 本计划不实现知识库功能（见 `docs/knowledge-base/`）；功能实现另开计划
- Spec：`docs/superpowers/specs/2026-09-23-fork-branch-strategy-design.md`

## File map

| 路径 | 职责 |
|------|------|
| `.gitignore` | 忽略 `.cursor/`（及确认 `.codegraph/` 已忽略） |
| `docs/knowledge-base/PRD-个人知识库系统.md` | 产品需求（已存在，未跟踪 → 入库） |
| `docs/knowledge-base/开发文档-技术设计.md` | 技术设计（已存在，未跟踪 → 入库） |
| `docs/superpowers/specs/2026-09-23-fork-branch-strategy-design.md` | 分支策略设计（已在 `develop`） |
| `docs/superpowers/plans/2026-09-23-fork-branch-strategy.md` | 本落地计划 |

---

### Task 1: 登记 `upstream` remote

**Files:**
- Modify: Git remotes only（无仓库文件）

**Interfaces:**
- Consumes: 维护者从 GitHub fork 页「About / forked from」复制的上游 clone URL
- Produces: remote 名 `upstream`，可 `git fetch upstream`

- [ ] **Step 1: 取得上游 URL**

在浏览器打开 `https://github.com/lzq-197/x-hub`，页面顶部应显示 `forked from <owner>/<repo>`。复制该上游仓库的 HTTPS clone URL（形如 `https://github.com/<owner>/<repo>.git`）。

在 PowerShell 中赋值（把引号内换成真实 URL）：

```powershell
$UpstreamUrl = "https://github.com/<owner>/<repo>.git"
```

- [ ] **Step 2: 若尚无 `upstream` 则添加**

```powershell
git remote -v
git remote add upstream $UpstreamUrl
```

若已存在 `upstream` 且 URL 错误：

```powershell
git remote set-url upstream $UpstreamUrl
```

- [ ] **Step 3: 验证 fetch**

```powershell
git fetch upstream
git remote -v
git branch -r
```

Expected:
- `upstream` 的 fetch/push URL 均为 `$UpstreamUrl`
- `git branch -r` 中出现 `upstream/master` 或 `upstream/main`（记下实际上游默认分支名，下文记为 `$UpstreamDefault`）

若上游默认分支是 `main`：

```powershell
$UpstreamDefault = "main"
```

若是 `master`：

```powershell
$UpstreamDefault = "master"
```

- [ ] **Step 4: 本任务无代码 commit**

（仅 remote 变更，不产生提交。）

---

### Task 2: 推送并确认 `develop` 为产品主线

**Files:**
- None（分支与远程）

**Interfaces:**
- Consumes: 本地已有 `develop`（含设计文档提交 `2b2f49a` 或其后继）
- Produces: `origin/develop` 可拉取

- [ ] **Step 1: 确认本地在 `develop` 且含设计文档**

```powershell
git checkout develop
git log -3 --oneline
Test-Path docs/superpowers/specs/2026-09-23-fork-branch-strategy-design.md
```

Expected: 当前分支 `develop`；`Test-Path` 为 `True`；近期提交含分支策略设计文档。

若本地尚无 `develop`（干净克隆的例外路径）：

```powershell
git checkout master
git checkout -b develop
```

- [ ] **Step 2: 推送 `develop` 到 `origin`**

```powershell
git push -u origin develop
```

Expected: 远程出现 `origin/develop`，upstream 跟踪设置成功。

- [ ] **Step 3: 验证 `master` 仍与 `origin/master` 对齐且无知识库文档**

```powershell
git checkout master
git status -sb
Test-Path docs/knowledge-base
Test-Path docs/superpowers/specs/2026-09-23-fork-branch-strategy-design.md
git checkout develop
```

Expected:
- `master` 上 `docs/knowledge-base` 与设计 spec **不存在**（或至少未提交进 `master` 历史）
- 回到 `develop` 后两路径均存在（spec 已在；knowledge-base 可能仍未跟踪，见 Task 4）

- [ ] **Step 4: 本任务无新文件 commit**（仅 push）

---

### Task 3: 忽略编辑器本地目录

**Files:**
- Modify: `.gitignore`（在「Tool working directories」或「Editor」段追加 `.cursor/`）

**Interfaces:**
- Consumes: 现有 `.gitignore`（已含 `.codegraph/`）
- Produces: `git status` 不再列出 `.cursor/`

- [ ] **Step 1: 确认当前仍能看到未跟踪的 `.cursor/`**

```powershell
git status -sb
```

Expected: 出现 `?? .cursor/`（若本地无该目录，Step 3 的负向检查改为「确保规则存在」即可）。

- [ ] **Step 2: 在 `.gitignore` 的 Tool working directories 段加入 `.cursor/`**

在 `.codegraph/` 那一段附近增加一行，使该段成为：

```gitignore
# Tool working directories
.omo/
.workbuddy/
.impeccable/
.playwright-mcp/
.agents/
.codegraph/
.cursor/
.opencode/
.opencode-sessions/
skills-lock.json
```

- [ ] **Step 3: 验证忽略生效**

```powershell
git check-ignore -v .cursor/
git status -sb
```

Expected:
- `check-ignore` 输出指向 `.gitignore` 中的 `.cursor/` 规则
- `git status` **不再**出现 `?? .cursor/`

- [ ] **Step 4: Commit**

```powershell
git add .gitignore
git commit -m "chore: ignore .cursor local editor directory"
```

---

### Task 4: 知识库 PRD/技术设计入库 `develop`

**Files:**
- Create (tracked): `docs/knowledge-base/PRD-个人知识库系统.md`
- Create (tracked): `docs/knowledge-base/开发文档-技术设计.md`

**Interfaces:**
- Consumes: 工作区已有未跟踪文件
- Produces: `develop` 上可浏览完整知识库规格；满足设计「合入顺序 0」

- [ ] **Step 1: 确认文件存在且内容非空**

```powershell
Get-Item docs/knowledge-base/PRD-个人知识库系统.md, docs/knowledge-base/开发文档-技术设计.md |
  Select-Object FullName, Length
```

Expected: 两个文件 `Length` 均明显大于 0（PRD ~数 KB，技术设计更大）。

- [ ] **Step 2: 仅添加知识库文档（不要加 `pnpm-lock.yaml`）**

```powershell
git add docs/knowledge-base/PRD-个人知识库系统.md docs/knowledge-base/开发文档-技术设计.md
git status
git diff --cached --stat
```

Expected: staged 仅上述两个文件（或该目录下仅此二者）；**无** `pnpm-lock.yaml`。

- [ ] **Step 3: Commit**

```powershell
git commit -m "docs(kb): add personal knowledge-base PRD and tech design"
```

- [ ] **Step 4: 推送 `develop`**

```powershell
git push origin develop
```

- [ ] **Step 5: 确认 `master` 未被污染**

```powershell
git checkout master
git log --oneline -1
git cat-file -e "develop:docs/knowledge-base/PRD-个人知识库系统.md" 2>$null; if ($LASTEXITCODE -eq 0) { "develop has PRD: yes" }
git cat-file -e "master:docs/knowledge-base/PRD-个人知识库系统.md" 2>$null; if ($LASTEXITCODE -ne 0) { "master has PRD: no (good)" }
git checkout develop
```

Expected: develop 有 PRD blob；master 无该路径。

---

### Task 5: 试跑一次上游同步（验证双轨）

**Files:**
- None（或仅 merge 提交，若上游有新提交）

**Interfaces:**
- Consumes: Task 1 的 `$UpstreamDefault`；`origin`/`upstream` 已就绪
- Produces: 证明 §5 同步流程可执行；`master` 仍无知识库路径

- [ ] **Step 1: Fetch 并合并上游到 `master`**

```powershell
git fetch upstream
git checkout master
git merge "upstream/$UpstreamDefault"
```

Expected:
- 若已是最新：`Already up to date.`
- 若有更新：产生 merge/ff 提交，无业务文件被误删

若出现冲突：按上游意图解决宿主文件；**不要**在此次冲突中把 `docs/knowledge-base` 合进 `master`。

- [ ] **Step 2: 推送镜像 `master`**

```powershell
git push origin master
```

- [ ] **Step 3: 将 `master` 合入 `develop`**

```powershell
git checkout develop
git merge master
```

Expected: 成功；若有冲突，在 `develop` 解决。知识库文档必须保留在 `develop`。

- [ ] **Step 4: 推送 `develop`（若有新 merge 提交）**

```powershell
git push origin develop
```

- [ ] **Step 5: 验收成功标准 1–4（分支层）**

```powershell
git remote -v
git branch -vv
git checkout master
Test-Path docs/knowledge-base
git checkout develop
Test-Path docs/knowledge-base/PRD-个人知识库系统.md
Test-Path docs/knowledge-base/开发文档-技术设计.md
```

Expected:
- remotes 含 `origin` 与 `upstream`
- `master` 上 `Test-Path docs/knowledge-base` 为 `False`
- `develop` 上两个知识库文档为 `True`

---

### Task 6: 创建并推送 `feature/kb-notes-structure`

**Files:**
- None（空功能分支，待后续知识库实现计划填写代码）

**Interfaces:**
- Consumes: 最新 `develop`（含知识库文档）
- Produces: `origin/feature/kb-notes-structure`，作为下一计划（文件夹+导入）的工作分支
- 明确不创建：`feature/kb-rag`（须等 structure 合回 `develop` 后再拉）

- [ ] **Step 1: 从最新 `develop` 拉分支**

```powershell
git checkout develop
git pull origin develop
git checkout -b feature/kb-notes-structure
```

- [ ] **Step 2: 推送并设置跟踪**

```powershell
git push -u origin feature/kb-notes-structure
```

- [ ] **Step 3: 记录分支拓扑验收**

```powershell
git branch -vv
git log --oneline --graph --decorate -8
```

Expected: 可见 `develop`、`feature/kb-notes-structure`；当前在 feature 分支；尚未出现 `feature/kb-rag`。

- [ ] **Step 4: 本任务无额外文件 commit**（分支点已含文档）

---

### Task 7: GitHub 保护项（手工清单）

**Files:**
- None（GitHub 网页设置）

**Interfaces:**
- Consumes: 已推送的 `master` / `develop`
- Produces: 降低误操作风险（可选但推荐）

- [ ] **Step 1: 保护 `master`**

打开：`https://github.com/lzq-197/x-hub/settings/branches`

为 `master` 添加 branch protection rule（按你账号权限能开的最严合理项）：
- 限制直接 push（若单人且无规则可选，则至少自己遵守「只经上游同步流程 push master」）
- 不要要求「必须先合 develop」之类会逼你污染镜像的规则

- [ ] **Step 2: 决定是否改 default branch 为 `develop`**

Settings → General → Default branch → 可改为 `develop`。

含义：克隆后默认进入产品线；仓库首页默认展示 `develop`。若希望访客仍看到「干净上游镜像」，则保持 default = `master`。

本任务记录选择（二选一，执行后打勾）：

- 选择 A：default = `develop`
- 选择 B：default 保持 `master`

- [ ] **Step 3: 无 git commit**

---

### Task 8: 提交本计划文档（若尚未入库）

**Files:**
- Create: `docs/superpowers/plans/2026-09-23-fork-branch-strategy.md`

**Interfaces:**
- Consumes: 本文件内容
- Produces: 计划可在 `develop` 上被后续 agent 读取

- [ ] **Step 1: 确保在 `develop`（不要把计划只留在 feature 分支）**

```powershell
git checkout develop
```

若计划文件是在 feature 分支创建的，先 cherry-pick 或把文件拷回 `develop` 再提交。

- [ ] **Step 2: Add 并 commit**

```powershell
git add docs/superpowers/plans/2026-09-23-fork-branch-strategy.md
git status
git commit -m "docs: add fork dual-track branch strategy implementation plan"
git push origin develop
```

- [ ] **Step 3: 将 `develop` 合入功能分支（保持 feature 不落后）**

```powershell
git checkout feature/kb-notes-structure
git merge develop
git push origin feature/kb-notes-structure
```

---

## 后续计划（本计划结束后另开，勿混进本计划任务）

1. **知识库 M1 环境手册执行**（Ollama + `bge-m3`）— 可写短 checklist，不占分支
2. **`feature/kb-notes-structure` 实现计划** — 依据 `docs/knowledge-base/开发文档-技术设计.md` 中文件夹/导入章节
3. **合入 `develop` 后** 再开 `feature/kb-rag` 实现计划 — 索引 + RAG + 统计
4. **M6 联调与从 `develop` 打 tag 发版**

---

## Spec coverage (self-review)

| Spec 项 | 对应任务 |
|---------|----------|
| 登记 `upstream` | Task 1 |
| 创建/推送 `develop` | Task 2（本地可能已有） |
| 知识库文档 → `develop` | Task 4 |
| `.cursor/` 入 ignore | Task 3 |
| 不提交 `pnpm-lock.yaml` | Task 4 Step 2 显式排除 |
| 上游同步流程可走通 | Task 5 |
| `feature/kb-notes-structure` | Task 6 |
| 不提前开 `feature/kb-rag` | Task 6 接口说明 |
| GitHub 保护 / default | Task 7 |
| 成功标准 1–4 | Task 5 Step 5 |
| 成功标准 5（structure→rag→tag） | 后续计划（本计划非目标） |
| 不实现知识库代码 | Global Constraints + 后续计划 |
