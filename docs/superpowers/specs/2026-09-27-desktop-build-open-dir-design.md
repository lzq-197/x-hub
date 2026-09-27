# 设计：桌面构建成功后打开产物目录

- 日期：2026-09-27
- 状态：对话已确认（方案 1）
- 关联：`docs/superpowers/specs/2026-09-26-desktop-build-channels-design.md`、`scripts/desktop-build.mjs`

## 1. 目标

`pnpm run build:prod` / `build:release` 成功结束后，自动打开本次产物所在目录（`dist-desktop/<channel>/`），方便立刻找到 exe。

## 2. 范围

### 2.1 做

| 项 | 说明 |
|----|------|
| 触发条件 | 仅当构建与收尾还原均成功（`!buildFailed && !restoreFailed`） |
| 打开路径 | `dirname(copyArtifact 返回的 destPath)`，即 `dist-desktop/prod` 或 `dist-desktop/release` |
| 打开方式 | 只打开目录，**不** `/select` 具体 exe |
| 平台 | Windows：`explorer <dir>`；macOS：`open`；Linux：`xdg-open` |
| 失败策略 | 打开目录失败 → `console.warn`，**不**改进程 exit code |

### 2.2 不做

- 失败构建时打开目录
- 选中 / 启动 exe
- 改裸 `tauri:build` / `npm run build`（前端）
- 环境变量开关（需要时可后续加）

## 3. 实现口径

在 `scripts/desktop-build.mjs`：

1. `let destPath = null`，在 `try` 内 `copyArtifact` 成功后赋值。
2. `finally` 收尾不变。
3. 末尾在判定失败 `process.exit(1)` 之前：若 `!buildFailed && !restoreFailed` 且 `destPath` 有值，则 `openArtifactDir(dirname(destPath))`。
4. `openArtifactDir(dir)`：按 `process.platform` 选命令；`spawnSync`、`stdio: 'ignore'`（或 pipe）；不 `shell: true`（路径已是绝对路径）。

## 4. 测试

- 优先：对「是否应打开」的条件（成功才开）用现有 `tests/desktop-build-*.mjs` 风格补一条小断言，或手测一次 prod 构建。
- 不强求对真实 `explorer` 做集成测（本机环境相关）。

## 5. 验收

- `build:prod` / `build:release` 成功后资源管理器打开对应 `dist-desktop/<channel>/`。
- 构建失败或还原失败时不打开、exit 仍为 1。
