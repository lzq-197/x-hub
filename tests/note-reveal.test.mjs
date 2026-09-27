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

/**
 * Minimal ProseMirror-like doc: one text node per block under distinct parents
 * so buildSearchIndex inserts a space between blocks.
 */
function makeDoc(blocks) {
  return {
    descendants(f) {
      let pos = 1
      for (const text of blocks) {
        const parent = { type: { name: 'paragraph' } }
        const node = {
          isText: true,
          text,
          type: { name: 'text' },
          textContent: text,
        }
        f(node, pos, parent)
        pos += text.length + 2
      }
    },
  }
}

function makeDocWithHeadings(sections) {
  // sections: [{ heading, body }]
  return {
    descendants(f) {
      let pos = 1
      for (const sec of sections) {
        const hParent = { type: { name: 'doc' } }
        const hNode = {
          isText: false,
          text: null,
          type: { name: 'heading' },
          textContent: sec.heading,
        }
        f(hNode, pos, hParent)
        pos += 2
        const pParent = { type: { name: 'paragraph' } }
        const tNode = {
          isText: true,
          text: sec.body,
          type: { name: 'text' },
          textContent: sec.body,
        }
        f(tNode, pos, pParent)
        pos += sec.body.length + 2
      }
    },
  }
}

const { findTextRangeInDoc, findHeadingPosInDoc, headingLeaf, buildSearchIndex } = loadModule(
  'src/utils/noteReveal.ts',
)

test('buildSearchIndex inserts space between blocks', () => {
  const { plain } = buildSearchIndex(makeDoc(['段甲', '段乙']))
  assert.equal(plain, '段甲 段乙')
})

test('findTextRangeInDoc matches cross-block needle from plainForMatch', () => {
  const doc = makeDoc(['定时器计数', '比较寄存器'])
  const range = findTextRangeInDoc(doc, '定时器计数 比较寄存器')
  assert.ok(range)
  assert.equal(typeof range.from, 'number')
  assert.equal(typeof range.to, 'number')
  assert.ok(range.to > range.from)
})

test('findTextRangeInDoc prefers match near heading pos', () => {
  const doc = makeDoc(['重复句', '重复句'])
  // First block text starts at pos 1; second at 1+2+2=5 in makeDoc? 
  // pos starts 1, after first: 1+2+2=5... text len 3 + 2 = 5, second at 5
  const first = findTextRangeInDoc(doc, '重复句')
  assert.ok(first)
  const nearSecond = findTextRangeInDoc(doc, '重复句', first.from + 10)
  assert.ok(nearSecond)
  assert.ok(nearSecond.from >= first.from)
  // With preferNear past first match, should pick second
  assert.ok(nearSecond.from > first.from)
})

test('headingLeaf takes last segment', () => {
  assert.equal(headingLeaf('A / B / C'), 'C')
  assert.equal(headingLeaf(''), '')
})

test('findHeadingPosInDoc finds heading node', () => {
  const doc = makeDocWithHeadings([{ heading: '架构', body: '正文' }])
  const pos = findHeadingPosInDoc(doc, '项目 / 架构')
  assert.equal(pos, 2) // pos+1 from descendants start at 1
})
