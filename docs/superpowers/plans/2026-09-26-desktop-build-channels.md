# Desktop Build Channels (prod / release) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `pnpm run build:prod` and `pnpm run build:release` so local desktop exes are built from the right branch, stamped with distinguishable versions, copied under `dist-desktop/`, and the working tree is restored afterward.

**Architecture:** Pure version/conf helpers live in a small ESM module (unit-tested with `node:test`). A CLI `scripts/desktop-build.mjs` orchestrates git guards, checkout, stamp, `pnpm exec tauri build`, copy, and `finally` restore. `package.json` only wires the two entry scripts.

**Tech Stack:** Node.js ESM, `node:test`, git CLI, pnpm, `@tauri-apps/cli` (`tauri build`), Windows process check via `tasklist`

**Spec:** `docs/superpowers/specs/2026-09-26-desktop-build-channels-design.md`

## Global Constraints

- Dirty working tree → abort (no stash)
- `build:prod` → checkout `master`; version `X.Y.Z+MMDDHHmm` (local TZ)
- `build:release` → `git fetch upstream` then `git checkout -B upstream-master upstream/master`; version three-part only
- Always restore `tauri.conf.json` version from in-memory original; always checkout back to pre-build branch/SHA
- Only touch `"version"` in `src-tauri/tauri.conf.json` (not `package.json` / `Cargo.toml`)
- Do not commit, push, or change git config
- `pnpm run build` stays frontend-only; do not change `beforeBuildCommand`
- Out of scope: updater skip for `+` builds, NSIS, self-hosted update channel
- If `src-tauri/tauri.conf.json` currently has a leftover `+` from a manual build, restore to three-part **before** relying on dirty-tree abort (commit or revert that file first)

## File map

| Path | Responsibility |
|------|----------------|
| `scripts/lib/desktop-build-version.mjs` | Pure helpers: strip meta, stamp prod, read/write conf version string |
| `scripts/desktop-build.mjs` | CLI: dirty/process/git/stamp/build/copy/finally |
| `tests/desktop-build-version.test.mjs` | Unit tests for pure helpers |
| `package.json` | `build:prod` / `build:release` scripts |
| `.gitignore` | Ignore `dist-desktop/` |
| `AGENTS.md` | Command cheat-sheet two lines |

---

### Task 1: Version / conf pure helpers + unit tests

**Files:**
- Create: `scripts/lib/desktop-build-version.mjs`
- Create: `tests/desktop-build-version.test.mjs`

**Interfaces:**
- Consumes: none
- Produces:
  - `stripBuildMeta(version: string): string` — drop `+…` suffix
  - `prodStamp(baseVersion: string, date?: Date): string` — `X.Y.Z+MMDDHHmm` local TZ
  - `releaseStamp(baseVersion: string): string` — three-part only (`stripBuildMeta`)
  - `readConfVersion(confText: string): string` — parse `"version": "…"` from tauri.conf JSON text
  - `writeConfVersion(confText: string, version: string): string` — replace only that field; throw if missing

- [ ] **Step 1: Write the failing tests**

Create `tests/desktop-build-version.test.mjs`:

```js
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  stripBuildMeta,
  prodStamp,
  releaseStamp,
  readConfVersion,
  writeConfVersion,
} from '../scripts/lib/desktop-build-version.mjs'

test('stripBuildMeta drops +suffix', () => {
  assert.equal(stripBuildMeta('0.6.5+09261006'), '0.6.5')
  assert.equal(stripBuildMeta('0.6.5'), '0.6.5')
})

test('prodStamp uses local MMDDHHmm', () => {
  const d = new Date(2026, 8, 26, 11, 45, 0) // month 0-based → Sep
  assert.equal(prodStamp('0.6.5', d), '0.6.5+09261145')
  assert.equal(prodStamp('0.6.5+old', d), '0.6.5+09261145')
})

test('releaseStamp is three-part only', () => {
  assert.equal(releaseStamp('0.6.5+09261006'), '0.6.5')
  assert.equal(releaseStamp('0.6.5'), '0.6.5')
})

test('read/writeConfVersion round-trip', () => {
  const sample = `{
  "productName": "x-hub",
  "version": "0.6.5",
  "identifier": "x-hub"
}
`
  assert.equal(readConfVersion(sample), '0.6.5')
  const next = writeConfVersion(sample, '0.6.5+09261145')
  assert.equal(readConfVersion(next), '0.6.5+09261145')
  assert.match(next, /"version": "0\.6\.5\+09261145"/)
  assert.match(next, /"productName": "x-hub"/)
})

test('writeConfVersion throws if version field missing', () => {
  assert.throws(() => writeConfVersion('{ "a": 1 }', '0.6.5'), /version/)
})
```

- [ ] **Step 2: Run tests — expect FAIL (module missing)**

```powershell
cd d:\deployer\x-hub
node --test tests/desktop-build-version.test.mjs
```

Expected: FAIL with ERR_MODULE_NOT_FOUND for `desktop-build-version.mjs`

- [ ] **Step 3: Implement helpers**

Create `scripts/lib/desktop-build-version.mjs`:

```js
/** @param {string} version */
export function stripBuildMeta(version) {
  const i = version.indexOf('+')
  return i === -1 ? version : version.slice(0, i)
}

/** @param {number} n */
function pad2(n) {
  return String(n).padStart(2, '0')
}

/**
 * @param {string} baseVersion
 * @param {Date} [date]
 */
export function prodStamp(baseVersion, date = new Date()) {
  const base = stripBuildMeta(baseVersion)
  const meta = `${pad2(date.getMonth() + 1)}${pad2(date.getDate())}${pad2(date.getHours())}${pad2(date.getMinutes())}`
  return `${base}+${meta}`
}

/** @param {string} baseVersion */
export function releaseStamp(baseVersion) {
  return stripBuildMeta(baseVersion)
}

const VERSION_RE = /("version"\s*:\s*")([^"]*)(")/

/** @param {string} confText */
export function readConfVersion(confText) {
  const m = confText.match(VERSION_RE)
  if (!m) throw new Error('tauri.conf.json: missing "version" field')
  return m[2]
}

/**
 * @param {string} confText
 * @param {string} version
 */
export function writeConfVersion(confText, version) {
  if (!VERSION_RE.test(confText)) {
    throw new Error('tauri.conf.json: missing "version" field')
  }
  return confText.replace(VERSION_RE, `$1${version}$3`)
}
```

- [ ] **Step 4: Run tests — expect PASS**

```powershell
node --test tests/desktop-build-version.test.mjs
```

Expected: all tests pass

- [ ] **Step 5: Commit**

```powershell
git add scripts/lib/desktop-build-version.mjs tests/desktop-build-version.test.mjs
git commit -m "test: add desktop build version helper unit tests"
```

---

### Task 2: CLI orchestrator + package.json + gitignore

**Files:**
- Create: `scripts/desktop-build.mjs`
- Modify: `package.json` (scripts section)
- Modify: `.gitignore` (add `dist-desktop/`)

**Interfaces:**
- Consumes: helpers from Task 1 (`stripBuildMeta` unused at CLI; use `prodStamp` / `releaseStamp` / `readConfVersion` / `writeConfVersion`)
- Produces: CLI invoked as `node scripts/desktop-build.mjs prod|release`; exit 0 on success

- [ ] **Step 1: Implement `scripts/desktop-build.mjs`**

Create the file with this complete contents (single try/finally; no drafts):

```js
/**
 * Desktop build channels
 * Spec: docs/superpowers/specs/2026-09-26-desktop-build-channels-design.md
 *
 * Usage: node scripts/desktop-build.mjs prod|release
 */
import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  prodStamp,
  releaseStamp,
  readConfVersion,
  writeConfVersion,
} from './lib/desktop-build-version.mjs'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const confPath = join(root, 'src-tauri', 'tauri.conf.json')
const releaseExe = join(root, 'src-tauri', 'target', 'release', 'x-hub.exe')
const channel = process.argv[2]

if (channel !== 'prod' && channel !== 'release') {
  console.error('Usage: node scripts/desktop-build.mjs prod|release')
  process.exit(2)
}

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, {
    cwd: root,
    encoding: 'utf8',
    shell: false,
    stdio: opts.stdio ?? 'pipe',
    ...opts,
  })
}

function runOrDie(cmd, args, label) {
  // pnpm/npm are .cmd shims on Windows; shell required for spawnSync
  const needShell = process.platform === 'win32' && (cmd === 'pnpm' || cmd === 'npm')
  const r = run(cmd, args, { stdio: 'inherit', shell: needShell })
  if (r.status !== 0) {
    throw new Error(`${label} failed (exit ${r.status ?? 'null'})`)
  }
}

function gitOk(args) {
  const r = run('git', args)
  if (r.status !== 0) {
    throw new Error(`git ${args.join(' ')} failed: ${r.stderr || r.stdout}`)
  }
  return (r.stdout || '').trim()
}

function assertCleanTree() {
  const out = gitOk(['status', '--porcelain'])
  if (out) {
    console.error(
      'Working tree is dirty. Commit or restore changes before build:prod / build:release.\n' + out,
    )
    process.exit(1)
  }
}

function assertXhubNotRunning() {
  if (process.platform === 'win32') {
    const r = run('tasklist', ['/FI', 'IMAGENAME eq x-hub.exe', '/NH'])
    const text = `${r.stdout || ''}${r.stderr || ''}`
    if (/x-hub\.exe/i.test(text)) {
      console.error('x-hub.exe is running. Quit the app so the release binary can be replaced.')
      process.exit(1)
    }
    return
  }
  const r = run('pgrep', ['-x', 'x-hub'])
  if (r.status === 0) {
    console.error('x-hub is running. Quit the app so the release binary can be replaced.')
    process.exit(1)
  }
}

function currentRef() {
  const branch = run('git', ['branch', '--show-current'])
  if (branch.status === 0 && branch.stdout.trim()) {
    return { type: 'branch', value: branch.stdout.trim() }
  }
  return { type: 'sha', value: gitOk(['rev-parse', 'HEAD']) }
}

function checkoutRef(ref) {
  runOrDie('git', ['checkout', ref.value], `git checkout ${ref.value}`)
}

function ensureUpstream() {
  const remotes = gitOk(['remote'])
  if (!remotes.split(/\r?\n/).includes('upstream')) {
    console.error(
      'Missing remote "upstream". Add it first, e.g.\n  git remote add upstream https://github.com/dckxx/x-hub.git',
    )
    process.exit(1)
  }
}

function switchForChannel() {
  if (channel === 'prod') {
    runOrDie('git', ['checkout', 'master'], 'git checkout master')
    return
  }
  ensureUpstream()
  runOrDie('git', ['fetch', 'upstream'], 'git fetch upstream')
  const probe = run('git', ['rev-parse', '--verify', 'upstream/master'])
  if (probe.status !== 0) {
    console.error('upstream/master not found after fetch.')
    process.exit(1)
  }
  runOrDie(
    'git',
    ['checkout', '-B', 'upstream-master', 'upstream/master'],
    'git checkout -B upstream-master',
  )
}

/** Write stamped version; return stamped string. Caller keeps pre-stamp text for restore. */
function stampVersion(preStampText) {
  const current = readConfVersion(preStampText)
  const next = channel === 'prod' ? prodStamp(current) : releaseStamp(current)
  if (next !== current) {
    writeFileSync(confPath, writeConfVersion(preStampText, next), 'utf8')
  }
  return next
}

function copyArtifact(version) {
  if (!existsSync(releaseExe)) {
    throw new Error(`Missing built exe: ${releaseExe}`)
  }
  const dir = join(root, 'dist-desktop', channel)
  mkdirSync(dir, { recursive: true })
  const dest = join(dir, `x-hub-${version}.exe`)
  copyFileSync(releaseExe, dest)
  return dest
}

assertCleanTree()
assertXhubNotRunning()

const startRef = currentRef()
let preStampConfText = null
let stampedVersion = null
let buildFailed = null

try {
  switchForChannel()
  preStampConfText = readFileSync(confPath, 'utf8')
  stampedVersion = stampVersion(preStampConfText)
  console.log(`[desktop-build] channel=${channel} version=${stampedVersion}`)
  runOrDie('pnpm', ['exec', 'tauri', 'build'], 'tauri build')
  const destPath = copyArtifact(stampedVersion)
  console.log(`[desktop-build] OK ${resolve(destPath)}`)
} catch (e) {
  buildFailed = e
  console.error(`[desktop-build] FAILED: ${e.message || e}`)
} finally {
  try {
    if (preStampConfText != null) {
      writeFileSync(confPath, preStampConfText, 'utf8')
    }
  } catch (e) {
    console.error(`[desktop-build] failed to restore tauri.conf.json: ${e.message || e}`)
  }
  try {
    checkoutRef(startRef)
  } catch (e) {
    console.error(`[desktop-build] failed to restore branch: ${e.message || e}`)
  }
}

if (buildFailed) process.exit(1)
```

- [ ] **Step 2: Wire package.json**

In `package.json` `scripts`, add after `"tauri:build"`:

```json
    "build:prod": "node scripts/desktop-build.mjs prod",
    "build:release": "node scripts/desktop-build.mjs release",
```

Keep existing `"build": "vue-tsc -b && vite build"` unchanged.

- [ ] **Step 3: Ignore output dir**

Append to `.gitignore`:

```
# Local desktop channel builds (build:prod / build:release)
dist-desktop/
```

- [ ] **Step 4: Smoke CLI guards (no full tauri build yet)**

With a **dirty** tree (e.g. uncommitted `tauri.conf.json`):

```powershell
pnpm run build:prod
```

Expected: exit non-zero, message about dirty working tree; no branch change lasting after failure (script exits before checkout if dirty check is first — good).

Then restore/commit dirty files so tree is clean, then:

```powershell
node scripts/desktop-build.mjs
```

Expected: exit 2, Usage message.

- [ ] **Step 5: Commit**

```powershell
git add scripts/desktop-build.mjs package.json .gitignore
git commit -m "feat: add build:prod and build:release desktop channel scripts"
```

---

### Task 3: AGENTS cheat-sheet + full smoke (optional long)

**Files:**
- Modify: `AGENTS.md` (命令速查 block ~lines 254–259)

**Interfaces:**
- Consumes: scripts from Task 2
- Produces: documented commands

- [ ] **Step 1: Update AGENTS.md 命令速查**

Replace the bash block with:

```bash
npm run dev            # Vite 开发服务器（浏览器预览 http://localhost:1420）
npm run tauri:dev      # Tauri 开发窗口（需 Rust 工具链）
npm run build          # vue-tsc 类型检查 + vite build（仅前端）
npm run tauri:build    # 构建桌面应用（当前分支；产物 target/release）
pnpm run build:prod    # 切 master → 版本 X.Y.Z+MMDDHHmm → tauri build → dist-desktop/prod/
pnpm run build:release # fetch upstream/master → 官方三位数 → tauri build → dist-desktop/release/
npm run tauri:test     # Rust 单元测试（Windows 必须走此包装脚本，见注意事项「cargo test」）
```

- [ ] **Step 2: Commit docs**

```powershell
git add AGENTS.md
git commit -m "docs: document build:prod and build:release in AGENTS"
```

- [ ] **Step 3: Full smoke `build:prod` (engineer machine; ~几分钟)**

Prerequisites:

1. Working tree clean (`git status` empty). If `tauri.conf.json` still has `+09261006`, restore to `"0.6.5"` and commit/restore first.
2. Quit any running `x-hub.exe`.
3. `master` branch exists.

```powershell
pnpm run build:prod
```

Expected:

- Log line with `channel=prod version=0.6.5+MMDDHHmm`
- File `dist-desktop/prod/x-hub-0.6.5+*.exe` exists
- After exit: same git branch as before; `tauri.conf.json` `"version"` has **no** `+`
- Optional: run the copied exe; About shows the `+` version

- [ ] **Step 4: Full smoke `build:release` (needs `upstream`)**

```powershell
git remote -v   # must list upstream
pnpm run build:release
```

Expected:

- Fetches upstream; builds with three-part version
- `dist-desktop/release/x-hub-X.Y.Z.exe` exists
- Branch restored to pre-build branch; conf clean

If `upstream` missing, expect clear error (no hang).

---

## Spec coverage check

| Spec requirement | Task |
|------------------|------|
| `build:prod` / `build:release` entries | Task 2 |
| Dirty abort | Task 2 `assertCleanTree` |
| x-hub process abort | Task 2 `assertXhubNotRunning` |
| prod → master; release → fetch + upstream-master | Task 2 `switchForChannel` |
| prod `+MMDDHHmm`; release three-part | Task 1 + Task 2 stamp |
| Copy to `dist-desktop/…` | Task 2 `copyArtifact` |
| Restore conf + branch in finally | Task 2 finally |
| `dist-desktop/` gitignore | Task 2 |
| AGENTS 命令速查 | Task 3 |
| `pnpm run build` unchanged | Task 2 (explicit non-change) |
| No updater / NSIS / push | Global Constraints |

## Self-review notes

- `stampVersion` runs **after** checkout and re-reads conf so release uses upstream’s version, not the fork tip’s leftover.
- Finally writes `preStampConfText` then checks out `startRef`, so porcelain stays clean when the start branch already matched that conf content.
