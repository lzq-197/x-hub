# KB Ask UI Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Polish KnowledgeView Q&A: group citations by note with collapsed snippets, clearer answer Markdown, clickable `[n]` → source highlight, and a clarification banner (model `【澄清】` line + confusion-pair heuristic fallback).

**Architecture:** Keep flat `Citation[]` / `[n]` numbering from `kb_ask`. Add pure helpers in `src/utils/kbAskDecorate.ts` (clarification strip, heuristic, cite linkify skipping `pre`/`code`, group-by-note). Wire them in `KnowledgeView.vue` after Done. Extend `build_rag_system_prompt` with one clarification rule + Rust unit test.

**Tech Stack:** Vue 3, existing `renderMarkdown` (marked + DOMPurify), Rust prompt string, no new crates/npm deps

## Global Constraints

- Spec: `docs/superpowers/specs/2026-09-27-kb-ask-ui-polish-design.md`
- Do **not** change `Citation` / `KbAskEvent` shapes
- Do **not** renumber citations when grouping
- Clarification marker literal: `【澄清】` (full-width brackets)
- Confusion pair v1: `定位器` ↔ `定时器` only
- Design tokens only (no hard-coded hex); follow `:global()` lightningcss rule (AGENTS 43)
- Windows Rust tests: `npm run tauri:test` (or manifest-embed + `cargo test --lib` equivalent)
- Commit only when executing plan with user approval for commits (or user asked to execute)

## File map

| Path | Responsibility |
|------|----------------|
| `src/utils/kbAskDecorate.ts` | Pure helpers: strip clarification, heuristic, group citations, linkify `[n]` in HTML |
| `src/components/KnowledgeView.vue` | Grouped sources UI, banner, Done-time decorate, answer styles, click handlers |
| `src-tauri/src/knowledge.rs` | Prompt rule + test that prompt contains `【澄清】` |

---

### Task 1: `kbAskDecorate` pure helpers (+ node smoke asserts)

**Files:**
- Create: `src/utils/kbAskDecorate.ts`
- Test via: `npx --yes tsx` one-liners / short script (no vitest in repo)

**Interfaces:**
- Consumes: `Citation` type from `src/api/tauri.ts` (`index`, `note_id`, `note_title`, `folder_path`, `heading`, `snippet`)
- Produces:
  - `stripClarification(answer: string): { banner: string | null; body: string }`
  - `confusionHint(question: string, citations: { note_title: string; heading: string }[]): string | null`
  - `groupCitationsByNote<T extends { note_id: number; note_title: string; folder_path: string; index: number }>(cites: T[]): { note_id: number; note_title: string; folder_path: string; items: T[] }[]`
  - `linkifyCiteRefs(html: string, validIndexes: Set<number> | number[]): string`

- [ ] **Step 1: Create `src/utils/kbAskDecorate.ts` with stubs that fail asserts**

```ts
import type { Citation } from '../api/tauri'

export type CiteGroup = {
  note_id: number
  note_title: string
  folder_path: string
  items: Citation[]
}

/** First non-empty line starting with 【澄清】 → banner; body without that line (+ one following blank). */
export function stripClarification(answer: string): { banner: string | null; body: string } {
  void answer
  return { banner: null, body: '' }
}

const PAIRS: [string, string][] = [['定位器', '定时器']]

export function confusionHint(
  question: string,
  citations: { note_title: string; heading: string }[],
): string | null {
  void question
  void citations
  return null
}

export function groupCitationsByNote(cites: Citation[]): CiteGroup[] {
  void cites
  return []
}

/** Replace [n] in text nodes only (skip pre/code). Buttons: class kb-ref, data-ref=n. */
export function linkifyCiteRefs(html: string, validIndexes: number[]): string {
  void validIndexes
  return html
}
```

- [ ] **Step 2: Run failing smoke (RED)**

```powershell
npx --yes tsx -e "import { stripClarification, confusionHint, groupCitationsByNote } from './src/utils/kbAskDecorate.ts'; const s=stripClarification('【澄清】更像定时器\n\n正文'); if(s.banner!=='更像定时器'||!s.body.includes('正文')) { console.error('strip fail',s); process.exit(1)}; const h=confusionHint('STM32 定位器?',[{note_title:'STM32 定时器',heading:''}]); if(!h||!h.includes('定时器')){console.error('hint fail',h);process.exit(1)}; const g=groupCitationsByNote([{index:1,note_id:9,note_title:'A',folder_path:'',heading:'',snippet:'x'},{index:2,note_id:9,note_title:'A',folder_path:'',heading:'',snippet:'y'}]); if(g.length!==1||g[0].items.length!==2){console.error('group fail',g);process.exit(1)}; console.log('unexpected pass'); process.exit(1)"
```

Expected: exit 1 / strip fail (stubs wrong).

- [ ] **Step 3: Implement helpers for real**

```ts
import type { Citation } from '../api/tauri'

export type CiteGroup = {
  note_id: number
  note_title: string
  folder_path: string
  items: Citation[]
}

const CLARIFY_PREFIX = '【澄清】'

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

export function linkifyCiteRefs(html: string, validIndexes: number[]): string {
  const valid = new Set(validIndexes)
  if (typeof DOMParser === 'undefined') {
    // Node smoke: skip DOM path
    return html
  }
  const doc = new DOMParser().parseFromString(`<div class="kb-linkify-root">${html}</div>`, 'text/html')
  const root = doc.body.firstElementChild
  if (!root) return html
  const skip = new Set(['PRE', 'CODE'])
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
```

Note: browser has `DOMParser`; `tsx` smoke for strip/hint/group only. Add a separate browser-free linkify fallback for Node tests using regex on a toy string **only if** no tags — or test linkify only in KnowledgeView manually. For Node, export a `linkifyCiteRefsInText(text, valid)` used when html has no tags; prefer DOMParser in app.

Simpler linkify for plan reliability — implement DOMParser path only; Task 1 smoke tests strip/hint/group; Task 3 manually verifies linkify.

- [ ] **Step 4: Re-run strip/hint/group smoke — expect PASS**

```powershell
npx --yes tsx -e "import { stripClarification, confusionHint, groupCitationsByNote } from './src/utils/kbAskDecorate.ts'; const s=stripClarification('【澄清】更像定时器\n\n正文'); if(s.banner!=='更像定时器'||s.body.trim()!=='正文') { console.error(s); process.exit(1)}; const h=confusionHint('STM32 定位器?',[{note_title:'STM32 定时器',heading:''}]); if(h!=='检索结果更接近「定时器」相关笔记。'){console.error(h);process.exit(1)}; console.log('ok')"
```

Expected: `ok`

- [ ] **Step 5: Commit** (when executing)

```bash
git add src/utils/kbAskDecorate.ts
git commit -m "feat(kb): ask decorate helpers for clarify, group, refs"
```

---

### Task 2: Prompt clarification rule (Rust)

**Files:**
- Modify: `src-tauri/src/knowledge.rs` (`build_rag_system_prompt`)

**Interfaces:**
- Consumes: existing prompt builder
- Produces: rule 7 (or renumber) mentioning `【澄清】`

- [ ] **Step 1: Add failing test** in `knowledge.rs` `mod tests` (near other knowledge tests; if `build_rag_system_prompt` is private, test via constructing empty hits or `pub(crate)` the fn for tests — prefer keep private and test with empty hits by calling through a `#[cfg(test)]` wrapper):

```rust
    #[test]
    fn rag_system_prompt_includes_clarification_rule() {
        let p = super::build_rag_system_prompt(&[]);
        assert!(p.contains("【澄清】"), "prompt missing clarify marker: {p}");
    }
```

If `build_rag_system_prompt` is private in parent, either move test to parent `#[cfg(test)] mod` that already exists in same file, or make the function `pub(super)`.

- [ ] **Step 2: Run RED**

```powershell
# after mt embed if needed
cargo test --manifest-path src-tauri/Cargo.toml --lib rag_system_prompt_includes_clarification_rule
```

Expected: FAIL assertion.

- [ ] **Step 3: Append rule to prompt string** (after rule 6, before `## 知识库片段`):

```text
7. 若用户用词与片段主题明显不符（笔误、近形近义等），答案第一行必须是「【澄清】」+ 一句说明（例如：知识库内容更接近「定时器」，而不是「定位器」），然后空一行再写正文；无需澄清时不要输出【澄清】行。
```

- [ ] **Step 4: Run GREEN** — same test PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/knowledge.rs
git commit -m "feat(kb): prompt rule for 【澄清】 first line"
```

---

### Task 3: KnowledgeView wire-up (sources group + banner + refs + CSS)

**Files:**
- Modify: `src/components/KnowledgeView.vue`

**Interfaces:**
- Consumes: helpers from Task 1; existing `citations`, `streamText`, `streamHtml`, `question`, `renderMarkdown`
- Produces: grouped UI; `clarifyBanner` ref; Done path decorate

- [ ] **Step 1: Import helpers + state**

```ts
import {
  stripClarification,
  confusionHint,
  groupCitationsByNote,
  linkifyCiteRefs,
  type CiteGroup,
} from '../utils/kbAskDecorate'
```

Add:

```ts
const clarifyBanner = ref<string | null>(null)
const expandedSnips = ref<Set<number>>(new Set()) // citation index
const flashCite = ref<number | null>(null)
let flashTimer: ReturnType<typeof setTimeout> | null = null

const citeGroups = computed(() => groupCitationsByNote(citations.value))

function toggleSnip(index: number) {
  const next = new Set(expandedSnips.value)
  if (next.has(index)) next.delete(index)
  else next.add(index)
  expandedSnips.value = next
}

function onAnswerClick(e: MouseEvent) {
  const t = e.target as HTMLElement | null
  const btn = t?.closest?.('button.kb-ref') as HTMLElement | null
  if (!btn) return
  const n = Number(btn.getAttribute('data-ref'))
  if (!Number.isFinite(n)) return
  const el = document.querySelector(`[data-cite-index="${n}"]`) as HTMLElement | null
  el?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  flashCite.value = n
  if (flashTimer) clearTimeout(flashTimer)
  flashTimer = setTimeout(() => {
    flashCite.value = null
  }, 1200)
}

function decorateAnswer(raw: string) {
  const { banner, body } = stripClarification(raw)
  let msg = banner
  if (!msg) {
    msg = confusionHint(
      question.value,
      citations.value.map((c) => ({ note_title: c.note_title, heading: c.heading })),
    )
  }
  clarifyBanner.value = msg
  const html = renderMarkdown(body)
  const idxs = citations.value.map((c) => c.index)
  streamHtml.value = linkifyCiteRefs(html, idxs)
}
```

In `submitAsk` reset: `clarifyBanner.value = null`, `expandedSnips.value = new Set()`, `flashCite.value = null`.

On chunk: keep `streamHtml.value = renderMarkdown(streamText.value)` (no linkify).

On Done: `streamText.value = e.answer` (if set), `citations.value = e.citations`, then `decorateAnswer(e.answer)`.

On Error with partial: optional `decorateAnswer(e.partial)` without requiring cites.

- [ ] **Step 2: Template — answer + banner + grouped sources**

Replace answer / citations block approximately:

```html
          <p v-if="clarifyBanner" class="kb-clarify" role="status">{{ clarifyBanner }}</p>

          <div
            v-if="streamHtml"
            class="kb-answer md-body"
            @click="onAnswerClick"
            v-html="streamHtml"
          />

          <div v-if="citations.length" class="kb-citations">
            <h3 class="kb-cite-heading">来源</h3>
            <div v-for="g in citeGroups" :key="g.note_id" class="kb-cite-group">
              <button
                type="button"
                class="kb-cite-group-head"
                @click="emit('open-note', g.note_id)"
              >
                <span class="kb-cite-title">{{ g.note_title || '无标题笔记' }}</span>
                <span class="kb-cite-path">
                  <template v-if="g.folder_path">{{ g.folder_path }}</template>
                  <template v-if="g.folder_path"> · </template>
                  {{ g.items.length }} 个片段
                </span>
              </button>
              <div
                v-for="c in g.items"
                :key="c.index"
                class="kb-cite"
                :data-cite-index="c.index"
                :data-flash="flashCite === c.index ? '1' : undefined"
              >
                <button type="button" class="kb-cite-row" @click="toggleSnip(c.index)">
                  <span class="kb-cite-idx">[{{ c.index }}]</span>
                  <span class="kb-cite-heading-text">{{ c.heading || '片段' }}</span>
                  <span class="kb-cite-chevron">{{ expandedSnips.has(c.index) ? '▾' : '▸' }}</span>
                </button>
                <p v-if="expandedSnips.has(c.index) && c.snippet" class="kb-cite-snip">{{ c.snippet }}</p>
              </div>
            </div>
          </div>
```

Keep `noHit` banner as today.

- [ ] **Step 3: Styles** (token-based; wrap `:global` properly if needed)

```css
.kb-clarify {
  margin: 0 0 10px;
  padding: 8px 12px;
  border-radius: 8px;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-1);
  background: color-mix(in srgb, var(--c-orange) 16%, transparent);
  border: 1px solid color-mix(in srgb, var(--c-orange) 35%, transparent);
}
.kb-answer :deep(h1),
.kb-answer :deep(h2),
.kb-answer :deep(h3) {
  font-weight: 650;
  margin: 0.85em 0 0.35em;
}
.kb-answer :deep(h1) { font-size: 16px; }
.kb-answer :deep(h2) { font-size: 15px; }
.kb-answer :deep(h3) { font-size: 14px; }
.kb-answer :deep(p) { margin: 0.4em 0; font-size: 13px; line-height: 1.55; }
.kb-answer :deep(ul),
.kb-answer :deep(ol) { margin: 0.4em 0; padding-left: 1.35em; font-size: 13px; }
.kb-answer :deep(code) {
  font-size: 12px;
  padding: 0.1em 0.35em;
  border-radius: 4px;
  background: var(--bg-card-soft);
}
.kb-answer :deep(button.kb-ref) {
  display: inline;
  padding: 0 2px;
  margin: 0;
  border: none;
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  color: var(--brand-600);
  border-radius: 4px;
  cursor: pointer;
  font: inherit;
  font-size: 0.92em;
}
.kb-cite-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 10px;
}
.kb-cite-group-head {
  text-align: left;
  border: none;
  background: transparent;
  cursor: pointer;
  padding: 4px 0;
}
.kb-cite[data-flash='1'] {
  outline: 2px solid color-mix(in srgb, var(--accent) 55%, transparent);
  border-radius: 8px;
}
.kb-cite-row {
  display: flex;
  gap: 8px;
  align-items: baseline;
  width: 100%;
  border: none;
  background: transparent;
  cursor: pointer;
  text-align: left;
  padding: 4px 6px;
  color: inherit;
  font: inherit;
}
.kb-cite-chevron { color: var(--text-3); font-size: 11px; margin-left: auto; }
```

Remove obsolete single-button `.kb-cite` hover rules that conflict, or adapt.

- [ ] **Step 4: `npm run build`** — expect exit 0 (vue-tsc + vite).

- [ ] **Step 5: Commit**

```bash
git add src/components/KnowledgeView.vue src/utils/kbAskDecorate.ts
git commit -m "feat(kb): group sources, cite jump, clarification banner"
```

---

### Task 4: Manual acceptance

- [ ] Rebuild/run app; ask `STM32 定位器有哪些类型?` with Top-K 6.
- [ ] Sources: one group for the timer note; snippets collapsed until expand.
- [ ] Click `[1]` in answer → source row flashes.
- [ ] Clarification: either `【澄清】` banner from model or heuristic 「定时器」 line.
- [ ] Code fences in answer (if any) do not turn `[n]` into buttons.

---

## Spec coverage

| Spec § | Task |
|--------|------|
| Sources group + collapsed snippet | 3 (+ group helper 1) |
| Answer Markdown polish | 3 |
| Clickable `[n]` + highlight | 1 linkify + 3 |
| `【澄清】` prompt + parse | 2 + 1 + 3 |
| Heuristic fallback | 1 + 3 |
| No protocol change | all |
| Manual acceptance | 4 |
