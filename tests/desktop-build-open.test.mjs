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
