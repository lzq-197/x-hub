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
    throw new Error(
      'Missing remote "upstream". Add it first, e.g.\n  git remote add upstream https://github.com/dckxx/x-hub.git',
    )
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
    throw new Error('upstream/master not found after fetch.')
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
let restoreFailed = false

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
    restoreFailed = true
    console.error(`[desktop-build] failed to restore tauri.conf.json: ${e.message || e}`)
  }
  try {
    checkoutRef(startRef)
  } catch (e) {
    restoreFailed = true
    console.error(`[desktop-build] failed to restore branch: ${e.message || e}`)
  }
}

if (buildFailed || restoreFailed) process.exit(1)
