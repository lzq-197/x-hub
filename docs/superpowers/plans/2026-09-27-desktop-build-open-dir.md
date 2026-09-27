# Desktop Build Open Artifact Dir Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After a successful `build:prod` / `build:release`, open `dist-desktop/<channel>/` in the OS file manager.

**Architecture:** Extract pure helpers (`shouldOpenArtifactDir`, `artifactOpenCommand`) into `scripts/lib/` so they can be unit-tested without running tauri build. Wire them at the end of `desktop-build.mjs` after restore succeeds. Opening uses `spawnSync` and never fails the build exit code.

**Tech Stack:** Node.js (`spawnSync`), existing `node:test` suite pattern in `tests/desktop-build-version.test.mjs`

## Global Constraints

- Spec: `docs/superpowers/specs/2026-09-27-desktop-build-open-dir-design.md`
- Open **only** on full success: `!buildFailed && !restoreFailed` and non-null `destPath`
- Path = `dirname(destPath)` → `dist-desktop/prod` or `dist-desktop/release`
- Open folder only — **no** `/select`, do **not** launch the exe
- Windows: `explorer <dir>`; Darwin: `open <dir>`; else: `xdg-open <dir>`
- Open failure → `console.warn` only; process exit unchanged
- Do **not** change bare `tauri:build` or frontend `npm run build`
- No env-var kill switch (YAGNI)

## File map

| Path | Responsibility |
|------|----------------|
| `scripts/lib/desktop-build-open.mjs` | Pure helpers: when to open + which command/args |
| `tests/desktop-build-open.test.mjs` | Unit tests for helpers |
| `scripts/desktop-build.mjs` | Capture `destPath`; call open after success |

---

### Task 1: Helpers + unit tests

**Files:**
- Create: `scripts/lib/desktop-build-open.mjs`
- Create: `tests/desktop-build-open.test.mjs`

**Interfaces:**
- Produces:
  - `shouldOpenArtifactDir({ buildFailed, restoreFailed, destPath }): boolean`
  - `artifactOpenCommand(platform: string, dir: string): { cmd: string, args: string[] }`
- Consumes: none

- [ ] **Step 1: Write failing tests**

```js
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  shouldOpenArtifactDir,
  artifactOpenCommand,
} from '../scripts/lib/desktop-build-open.mjs'

test('shouldOpenArtifactDir only when both flags clear and dest set', () => {
  assert.equal(
    shouldOpenArtifactDir({
      buildFailed: null,
      restoreFailed: false,
      destPath: 'D:/x/dist-desktop/prod/x-hub-0.6.5.exe',
    }),
    true,
  )
  assert.equal(
    shouldOpenArtifactDir({
      buildFailed: new Error('x'),
      restoreFailed: false,
      destPath: 'D:/x/dist-desktop/prod/x-hub-0.6.5.exe',
    }),
    false,
  )
  assert.equal(
    shouldOpenArtifactDir({
      buildFailed: null,
      restoreFailed: true,
      destPath: 'D:/x/dist-desktop/prod/x-hub-0.6.5.exe',
    }),
    false,
  )
  assert.equal(
    shouldOpenArtifactDir({
      buildFailed: null,
      restoreFailed: false,
      destPath: null,
    }),
    false,
  )
})

test('artifactOpenCommand by platform', () => {
  assert.deepEqual(artifactOpenCommand('win32', 'D:\\dist-desktop\\prod'), {
    cmd: 'explorer',
    args: ['D:\\dist-desktop\\prod'],
  })
  assert.deepEqual(artifactOpenCommand('darwin', '/tmp/dist-desktop/prod'), {
    cmd: 'open',
    args: ['/tmp/dist-desktop/prod'],
  })
  assert.deepEqual(artifactOpenCommand('linux', '/tmp/dist-desktop/prod'), {
    cmd: 'xdg-open',
    args: ['/tmp/dist-desktop/prod'],
  })
})
```

- [ ] **Step 2: Run RED**

Run: `node --test tests/desktop-build-open.test.mjs`  
Expected: FAIL (module missing / exports missing)

- [ ] **Step 3: Implement helpers**

```js
/** @param {{ buildFailed: unknown, restoreFailed: boolean, destPath: string | null | undefined }} p */
export function shouldOpenArtifactDir({ buildFailed, restoreFailed, destPath }) {
  return !buildFailed && !restoreFailed && Boolean(destPath)
}

/** @param {string} platform @param {string} dir */
export function artifactOpenCommand(platform, dir) {
  if (platform === 'win32') return { cmd: 'explorer', args: [dir] }
  if (platform === 'darwin') return { cmd: 'open', args: [dir] }
  return { cmd: 'xdg-open', args: [dir] }
}
```

- [ ] **Step 4: Run GREEN**

Run: `node --test tests/desktop-build-open.test.mjs`  
Expected: PASS (2 tests)

- [ ] **Step 5: Commit**

```bash
git add scripts/lib/desktop-build-open.mjs tests/desktop-build-open.test.mjs
git commit -m "feat(build): helpers to open desktop artifact directory"
```

---

### Task 2: Wire `desktop-build.mjs`

**Files:**
- Modify: `scripts/desktop-build.mjs`

**Interfaces:**
- Consumes: `shouldOpenArtifactDir`, `artifactOpenCommand` from Task 1; existing `run`/`spawnSync` pattern
- Produces: successful builds open the folder

- [ ] **Step 1: Import + capture destPath**

Near top:

```js
import { dirname } from 'node:path' // already have join, resolve — add dirname to existing import
import {
  shouldOpenArtifactDir,
  artifactOpenCommand,
} from './lib/desktop-build-open.mjs'
```

Before `try`:

```js
let destPath = null
```

Inside `try`, replace local const with assignment:

```js
  destPath = copyArtifact(stampedVersion)
  console.log(`[desktop-build] OK ${resolve(destPath)}`)
```

- [ ] **Step 2: Add open helper + call before exit**

```js
function openArtifactDir(dir) {
  const { cmd, args } = artifactOpenCommand(process.platform, dir)
  const r = run(cmd, args, { stdio: 'ignore' })
  if (r.status !== 0 && r.error) {
    console.warn(`[desktop-build] could not open folder ${dir}: ${r.error.message || r.error}`)
  } else if (r.status !== 0) {
    console.warn(`[desktop-build] could not open folder ${dir} (exit ${r.status})`)
  }
}

// After finally block, before / instead of bare exit:
if (shouldOpenArtifactDir({ buildFailed, restoreFailed, destPath })) {
  openArtifactDir(dirname(destPath))
}
if (buildFailed || restoreFailed) process.exit(1)
```

Note: on Windows, `explorer` sometimes returns non-zero even when the window opens — treating only `r.error` (spawn failure) as hard warn is fine; if status≠0 without error, still warn lightly or ignore. Prefer: warn only when `r.error` is set (ENOENT); ignore explorer quirky exit codes.

Revised:

```js
function openArtifactDir(dir) {
  const { cmd, args } = artifactOpenCommand(process.platform, dir)
  const r = run(cmd, args, { stdio: 'ignore' })
  if (r.error) {
    console.warn(`[desktop-build] could not open folder ${dir}: ${r.error.message}`)
  }
}
```

- [ ] **Step 3: Sanity-check unit tests still pass**

Run: `node --test tests/desktop-build-open.test.mjs tests/desktop-build-version.test.mjs`  
Expected: all PASS

- [ ] **Step 4: Commit**

```bash
git add scripts/desktop-build.mjs
git commit -m "feat(build): open dist-desktop channel folder after successful build"
```

---

## Spec coverage (self-review)

| Spec item | Task |
|-----------|------|
| Success-only open | 1 + 2 |
| `dirname(destPath)` | 2 |
| Folder only, no select | 2 (`explorer`/`open`/`xdg-open` with dir only) |
| Platform commands | 1 + 2 |
| Warn on open fail, don’t fail build | 2 |
| No tauri:build / env switch | Global constraints |

**Placeholder scan:** none.  
**Type consistency:** `destPath` string | null; `buildFailed` null | Error-like.
