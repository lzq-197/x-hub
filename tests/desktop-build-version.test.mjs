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
