import type { Citation } from '../api/tauri'

export type CiteGroup = {
  note_id: number
  note_title: string
  folder_path: string
  items: Citation[]
}

const CLARIFY_PREFIX = '【澄清】'

/** First non-empty line starting with 【澄清】 → banner; body without that line (+ one following blank). */
export function stripClarification(answer: string): { banner: string | null; body: string } {
  const lines = answer.split('\n')
  let i = 0
  while (i < lines.length && lines[i].trim() === '') i++
  if (i >= lines.length) return { banner: null, body: answer }
  const line = lines[i]
  const trimmed = line.trimStart()
  if (!trimmed.startsWith(CLARIFY_PREFIX)) return { banner: null, body: answer }
  const banner = trimmed.slice(CLARIFY_PREFIX.length).trim()
  i++
  if (i < lines.length && lines[i].trim() === '') i++
  const body = lines.slice(i).join('\n')
  return { banner: banner || null, body }
}

const PAIRS: [string, string][] = [['定位器', '定时器']]

export function confusionHint(
  question: string,
  citations: { note_title: string; heading: string }[],
): string | null {
  if (!question.trim() || citations.length === 0) return null
  const corpus = citations.map((c) => `${c.note_title}\n${c.heading}`).join('\n')
  for (const [a, b] of PAIRS) {
    if (question.includes(a) && corpus.includes(b)) {
      return `检索结果更接近「${b}」相关笔记。`
    }
    if (question.includes(b) && corpus.includes(a)) {
      return `检索结果更接近「${a}」相关笔记。`
    }
  }
  return null
}

export function groupCitationsByNote(cites: Citation[]): CiteGroup[] {
  const order: number[] = []
  const map = new Map<number, CiteGroup>()
  for (const c of cites) {
    let g = map.get(c.note_id)
    if (!g) {
      g = {
        note_id: c.note_id,
        note_title: c.note_title,
        folder_path: c.folder_path,
        items: [],
      }
      map.set(c.note_id, g)
      order.push(c.note_id)
    }
    g.items.push(c)
  }
  return order.map((id) => map.get(id)!)
}

/** Replace [n] in text nodes only (skip pre/code). Buttons: class kb-ref, data-ref=n. */
export function linkifyCiteRefs(html: string, validIndexes: Set<number> | number[]): string {
  const valid = validIndexes instanceof Set ? validIndexes : new Set(validIndexes)
  if (typeof DOMParser === 'undefined') {
    // Node smoke: skip DOM path
    return html
  }
  const doc = new DOMParser().parseFromString(`<div class="kb-linkify-root">${html}</div>`, 'text/html')
  const root = doc.body.firstElementChild
  if (!root) return html
  // Skip pre/code (fences) and a (avoid button-inside-link)
  const skip = new Set(['PRE', 'CODE', 'A'])
  const walk = (node: Node) => {
    if (node.nodeType === Node.ELEMENT_NODE) {
      const el = node as Element
      if (skip.has(el.tagName)) return
      Array.from(el.childNodes).forEach(walk)
      return
    }
    if (node.nodeType !== Node.TEXT_NODE) return
    const text = node.textContent ?? ''
    const re = /\[(\d+)\]/g
    if (!re.test(text)) return
    re.lastIndex = 0
    const frag = doc.createDocumentFragment()
    let last = 0
    let m: RegExpExecArray | null
    while ((m = re.exec(text))) {
      const n = Number(m[1])
      frag.appendChild(doc.createTextNode(text.slice(last, m.index)))
      if (valid.has(n)) {
        const btn = doc.createElement('button')
        btn.type = 'button'
        btn.className = 'kb-ref'
        btn.setAttribute('data-ref', String(n))
        btn.textContent = `[${n}]`
        frag.appendChild(btn)
      } else {
        frag.appendChild(doc.createTextNode(m[0]))
      }
      last = m.index + m[0].length
    }
    frag.appendChild(doc.createTextNode(text.slice(last)))
    node.parentNode?.replaceChild(frag, node)
  }
  walk(root)
  return root.innerHTML
}
