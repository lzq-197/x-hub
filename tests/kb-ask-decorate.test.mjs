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
      ...globals,
    },
    { filename: absolute },
  )
  return module.exports
}

/** Minimal DOM enough for linkifyCiteRefs (text replace + skip PRE/CODE/A). */
function installMiniDom(globals) {
  const Node = { ELEMENT_NODE: 1, TEXT_NODE: 3 }
  class MiniNode {
    constructor() {
      this.parentNode = null
      this.childNodes = []
    }
    appendChild(child) {
      child.parentNode = this
      this.childNodes.push(child)
      return child
    }
    replaceChild(next, old) {
      const i = this.childNodes.indexOf(old)
      if (i < 0) throw new Error('replaceChild miss')
      old.parentNode = null
      if (next.childNodes && next._isFrag) {
        const parts = [...next.childNodes]
        this.childNodes.splice(i, 1, ...parts)
        for (const p of parts) p.parentNode = this
      } else {
        this.childNodes[i] = next
        next.parentNode = this
      }
      return old
    }
  }
  class MiniText extends MiniNode {
    constructor(text) {
      super()
      this.nodeType = Node.TEXT_NODE
      this.textContent = text
    }
  }
  class MiniEl extends MiniNode {
    constructor(tag) {
      super()
      this.nodeType = Node.ELEMENT_NODE
      this.tagName = tag.toUpperCase()
      this.attrs = {}
      this._isFrag = false
    }
    setAttribute(k, v) {
      this.attrs[k] = String(v)
    }
    getAttribute(k) {
      return this.attrs[k]
    }
    set type(v) {
      this.attrs.type = v
    }
    get type() {
      return this.attrs.type
    }
    set className(v) {
      this.attrs.class = v
    }
    get className() {
      return this.attrs.class || ''
    }
    set textContent(v) {
      this.childNodes = [new MiniText(String(v))]
      for (const c of this.childNodes) c.parentNode = this
    }
    get textContent() {
      return this.childNodes.map((c) => c.textContent ?? '').join('')
    }
    get innerHTML() {
      return serialize(this)
    }
    set innerHTML(_v) {
      /* unused */
    }
    get firstElementChild() {
      return this.childNodes.find((c) => c.nodeType === Node.ELEMENT_NODE) || null
    }
  }
  function serialize(el) {
    if (el.nodeType === Node.TEXT_NODE) return escapeText(el.textContent)
    const tag = el.tagName.toLowerCase()
    if (tag === '#document-fragment' || el._isFrag) {
      return el.childNodes.map(serialize).join('')
    }
    let attrs = ''
    for (const [k, v] of Object.entries(el.attrs || {})) {
      attrs += ` ${k}="${String(v).replace(/"/g, '&quot;')}"`
    }
    const inner = el.childNodes.map(serialize).join('')
    if (tag === 'button') return `<button${attrs}>${inner}</button>`
    return `<${tag}${attrs}>${inner}</${tag}>`
  }
  function escapeText(t) {
    return String(t).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
  }
  function parseSimple(wrapped) {
    // Only supports <div class="kb-linkify-root">…</div> with <p>/<a>/<code>/<pre> and text.
    const root = new MiniEl('div')
    root.attrs.class = 'kb-linkify-root'
    const inner = wrapped.replace(/^<div class="kb-linkify-root">/, '').replace(/<\/div>$/, '')
    const re = /<(p|a|code|pre)(\s[^>]*)?>([\s\S]*?)<\/\1>|([^<]+)/gi
    let m
    while ((m = re.exec(inner))) {
      if (m[4] != null) {
        const t = new MiniText(m[4])
        t.parentNode = root
        root.childNodes.push(t)
        continue
      }
      const el = new MiniEl(m[1])
      if (m[2] && /href\s*=\s*"([^"]*)"/.test(m[2])) {
        el.attrs.href = RegExp.$1
      }
      const childText = new MiniText(m[3])
      childText.parentNode = el
      el.childNodes.push(childText)
      el.parentNode = root
      root.childNodes.push(el)
    }
    return root
  }
  class DOMParser {
    parseFromString(html) {
      const body = new MiniEl('body')
      const root = parseSimple(html)
      root.parentNode = body
      body.childNodes.push(root)
      return {
        body,
        createDocumentFragment() {
          const f = new MiniEl('fragment')
          f._isFrag = true
          f.tagName = '#DOCUMENT-FRAGMENT'
          return f
        },
        createTextNode(t) {
          return new MiniText(t)
        },
        createElement(tag) {
          return new MiniEl(tag)
        },
      }
    }
  }
  globals.DOMParser = DOMParser
  globals.Node = Node
}

const stub = {
  index: 0,
  note_id: 0,
  note_title: '',
  folder_path: '',
  heading: '',
  snippet: '',
}

const plain = loadModule('src/utils/kbAskDecorate.ts', {})

test('stripClarification peels first 【澄清】 line and one blank', () => {
  const s = plain.stripClarification('【澄清】更像定时器\n\n正文[1]\n')
  assert.equal(s.banner, '更像定时器')
  assert.equal(s.body.trim(), '正文[1]')
})

test('stripClarification ignores mid-body marker', () => {
  const s = plain.stripClarification('正文\n【澄清】不该剥\n')
  assert.equal(s.banner, null)
  assert.match(s.body, /【澄清】/)
})

test('confusionHint 定位器↔定时器', () => {
  const h = plain.confusionHint('STM32 定位器有哪些类型?', [
    { note_title: 'STM32 定时器与 PWM', heading: '' },
  ])
  assert.equal(h, '检索结果更接近「定时器」相关笔记。')
})

test('confusionHint returns null without pair', () => {
  assert.equal(
    plain.confusionHint('天气怎么样', [{ note_title: 'STM32 定时器', heading: '' }]),
    null,
  )
})

test('groupCitationsByNote preserves order and indexes', () => {
  const g = plain.groupCitationsByNote([
    { ...stub, index: 1, note_id: 9, note_title: 'A', snippet: 'x' },
    { ...stub, index: 2, note_id: 9, note_title: 'A', snippet: 'y' },
    { ...stub, index: 3, note_id: 2, note_title: 'B', snippet: 'z' },
  ])
  assert.equal(g.length, 2)
  assert.equal(g[0].note_id, 9)
  assert.equal(g[0].items.length, 2)
  assert.equal(g[0].items[0].index, 1)
  assert.equal(g[1].note_id, 2)
})

test('linkifyCiteRefs without DOMParser is identity', () => {
  const html = '<p>见[1]</p>'
  assert.equal(plain.linkifyCiteRefs(html, [1]), html)
})

test('linkifyCiteRefs with DOM skips A/CODE and linkifies plain [n]', () => {
  const globals = {}
  installMiniDom(globals)
  const mod = loadModule('src/utils/kbAskDecorate.ts', {}, globals, new Map())
  const html = '<p>见[1]</p><a href="x">[1]</a><code>[1]</code>'
  const out = mod.linkifyCiteRefs(html, [1])
  assert.match(out, /<button[^>]*class="kb-ref"[^>]*data-ref="1"[^>]*>\[1\]<\/button>/)
  assert.match(out, /<a href="x">\[1\]<\/a>/)
  assert.match(out, /<code>\[1\]<\/code>/)
})
