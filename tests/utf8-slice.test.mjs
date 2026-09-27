import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import vm from 'node:vm'
import test from 'node:test'
import ts from 'typescript'

const require = createRequire(import.meta.url)
const root = path.resolve(import.meta.dirname, '..')

function loadModule(file, replacements = {}, globals = {}, cache = new Map()) {
  const absolute = path.resolve(root, file)
  if (cache.has(absolute)) return cache.get(absolute).exports
  const module = { exports: {} }
  cache.set(absolute, module)
  const output = ts.transpileModule(readFileSync(absolute, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText
  const localRequire = (name) => {
    if (name in replacements) return replacements[name]
    if (name.startsWith('.')) {
      const target = path.resolve(path.dirname(absolute), name)
      return loadModule(`${target}.ts`, replacements, globals, cache)
    }
    return require(name)
  }
  vm.runInNewContext(
    output,
    {
      module,
      exports: module.exports,
      require: localRequire,
      console,
      TextEncoder,
      TextDecoder,
      ...globals,
    },
    { filename: absolute },
  )
  return module.exports
}

const { utf8ByteSlice, utf8ByteLength, offsetsValid, plainForMatch } = loadModule(
  'src/utils/utf8Slice.ts',
)

test('utf8ByteSlice cuts on Chinese char boundaries', () => {
  const s = '见 **输出**'
  const prefix = new TextEncoder().encode('见 **').length
  const end = prefix + new TextEncoder().encode('输出').length
  assert.equal(utf8ByteSlice(s, prefix, end), '输出')
  assert.ok(utf8ByteLength(s) > s.length)
})

test('offsetsValid rejects -1 and inverted', () => {
  assert.equal(offsetsValid(-1, 10, 100), false)
  assert.equal(offsetsValid(0, 0, 100), false)
  assert.equal(offsetsValid(0, 5, 5), true)
})

test('plainForMatch strips emphasis and links', () => {
  assert.equal(plainForMatch('见 **输出比较** 与 [PWM](http://x)'), '见 输出比较 与 PWM')
})
