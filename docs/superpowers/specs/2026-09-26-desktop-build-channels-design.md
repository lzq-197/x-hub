# 桌面构建双渠道（prod / release）设计

**日期：** 2026-09-26  
**状态：** 已批准（对话确认 §1–§3）  
**范围：** 本机试包脚本与产物目录；不改正式发版 / updater / 服务端地址

## 背景

本仓库同时服务两条线：

- **自用线**：在 fork（`origin`）的 `master` 上开发（知识库等），需要本地桌面 exe 试跑。
- **官方线**：偶尔需要对照 / 使用 `upstream`（`dckxx/x-hub`）最新正式代码的桌面包。

现状：

- `pnpm run build` / `npm run build` **只编前端**（`vue-tsc` + `vite`），供 Tauri `beforeBuildCommand` 使用。
- 桌面 exe 靠 `pnpm run tauri:build`；版本号需手改 `tauri.conf.json`；切分支、拉官方、区分产物全靠记忆。
- 本地试包约定（AGENTS 发版清单）：`tauri.conf.json` 用 `X.Y.Z+MMDDHHmm`，正式提交前还原三位数。

目标：两条命令把「切分支 + 版本戳 + tauri build + 拷贝产物 + 收尾还原」绑死，且两套产物可区分。

## 决策摘要

| 项 | 选择 |
|----|------|
| 脏工作区 | 非空则中止（不 stash） |
| `build:prod` 分支 | 本地 `master` |
| `build:release` 分支 | fetch 后对齐 `upstream/master`（本地跟踪名 `upstream-master`） |
| 版本号 | prod：`X.Y.Z+MMDDHHmm`；release：官方三位数 `X.Y.Z` |
| 收尾 | 还原 `tauri.conf.json` version；切回构建前分支 |
| 产物 | 拷贝到 `dist-desktop/<prod\|release>/`，避免只靠覆盖 `target/release/x-hub.exe` 区分 |

**推荐实现形态：** 方案 2（双脚本 + 临时改版本 + 按渠道拷贝）。

## §1 目标与入口

| 命令 | 代码来源 | 关于里版本 | 产物 |
|------|----------|------------|------|
| `pnpm run build:prod` | 本地 `master` | `X.Y.Z+MMDDHHmm` | `dist-desktop/prod/x-hub-<ver>.exe` |
| `pnpm run build:release` | `upstream/master`（先 fetch） | 官方 `X.Y.Z` | `dist-desktop/release/x-hub-<ver>.exe` |

**保持不变：**

- `pnpm run build`：仅前端。
- `pnpm run tauri:build`：手动场景（已在正确分支、自行管版本）。

**实现：** `package.json` 两条脚本 → 共用 `scripts/desktop-build.mjs`，参数 `prod` | `release`。Windows 下用 `node` + git 子进程。

## §2 流程与护栏

共用流水线（差异仅在分支与版本戳）：

1. **脏检查**：`git status --porcelain` 非空 → 非 0 退出，提示先提交或还原；不 stash。
2. **进程占用**：存在名为 `x-hub` 的进程 → 中止，提示先退出应用（避免 `x-hub.exe` rename 失败）。
3. **记起点**：记录当前分支名（或 detached SHA），供失败/成功后还原。
4. **切代码**
   - `prod`：`git checkout master`（已在则跳过）。
   - `release`：`git fetch upstream` → `git checkout -B upstream-master upstream/master`（强制与上游一致）。
5. **版本戳**（只改 `src-tauri/tauri.conf.json` 的 `"version"`；不改 `package.json` / `Cargo.toml`）
   - `prod`：剥掉已有 `+…` 得基线 → 写成 `X.Y.Z+MMDDHHmm`（本地时区）。
   - `release`：保持官方三位数；若意外带 `+` 则剥掉再编。
6. **构建**：`pnpm exec tauri build`（或等价）。失败也必须走收尾。
7. **拷贝产物**：`src-tauri/target/release/x-hub.exe` → `dist-desktop/<prod|release>/x-hub-<version>.exe`（目录按需创建；同名覆盖）。
8. **收尾（finally）**
   - 用步骤 5 **之前保存在内存的原文**写回 `tauri.conf.json` 的 version（不用 `git checkout --` 该文件）。
   - `git checkout` 回步骤 3 的起点。
9. **成功输出**：打印 exe 绝对路径与版本号。

其它：

- `dist-desktop/` 加入 `.gitignore`。
- 脚本不 `commit`、不 `push`、不改 git config。

## §3 边界、文档与非目标

**边界**

- 无 `upstream` remote → `build:release` 中止，提示添加 upstream。
- `master` 或 `upstream/master` 不存在 → 中止并说明。
- `release` 使用的本地分支名固定为 `upstream-master`；日常开发仍在 `master`。

**文档**

- `AGENTS.md`「命令速查」增加 `build:prod` / `build:release` 两行说明。
- 不改正式发版清单（tag / CI）；本脚本仅本机试包渠道。

**非目标（本轮不做）**

- 修改 updater，使带 `+` 的本地包永不弹官方更新（semver 下 `0.6.5+meta` 仍可能提示升到 `0.7.0`；若需要另开任务）。
- 自建更新频道 / 改 `DEFAULT_SERVER_URL`。
- 额外打 NSIS/安装包（沿用当前 `tauri build` 的 exe 产出）。
- 自动 commit / push / 发 GitHub Release。

## 验收标准

- 干净工作区下，`pnpm run build:prod` 产出 `dist-desktop/prod/x-hub-*.exe`，关于页版本含 `+`，结束后回到原分支且 `tauri.conf.json` 无残留 `+`。
- 干净工作区下，`pnpm run build:release` 在已配置 `upstream` 时对齐官方 tip，产出 `dist-desktop/release/x-hub-X.Y.Z.exe`，结束后回到原分支。
- 脏工作区或 `x-hub` 进程占用时，命令失败且不改分支/版本文件。
- `pnpm run build` 行为与今日一致（仅前端）。

## 实现触点（供后续 plan）

- 新增 `scripts/desktop-build.mjs`
- `package.json`：`build:prod` / `build:release`
- `.gitignore`：`dist-desktop/`
- `AGENTS.md` 命令速查两行
