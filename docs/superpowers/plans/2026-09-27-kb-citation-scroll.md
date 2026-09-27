# KB Citation Scroll-to-Source Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clicking a knowledge-base citation opens the note and scrolls/highlights the exact source span using UTF-8 byte offsets stored at index time.

**Architecture:** Extend `chunk_markdown` to emit `md_start`/`md_end` (UTF-8 byte offsets into `notes.content`) via a strip alignment map; persist on `kb_chunks`; thread through `KbChunkHit` → `Citation`; frontend opens the note with a pending reveal that selects text in Crepe after mount.

**Tech Stack:** Rust (`repo/knowledge.rs`, `db.rs`, `models.rs`, `knowledge.rs`), Vue 3 + Milkdown/Crepe (`KnowledgeView`, `index.vue`, `NoteEditor`), Node test harness for UTF-8 slice util (`tests/*.test.mjs`)

## Global Constraints

- Spec: `docs/superpowers/specs/2026-09-27-kb-citation-scroll-design.md`
- Offsets are UTF-8 **byte** indices, half-open `[md_start, md_end)`; invalid = `-1` in DB / `null` in Citation JSON
- Do **not** change chunk length, Top-K, retrieval, or embed models
- Do **not** silent full rebuild on startup
- Windows tests: `npm run tauri:test` only (never bare `cargo test`)
- Commit only when the user asks, or when executing a plan step that says commit **and** the user already approved plan execution that includes commits

## File map

| Path | Responsibility |
|------|----------------|
| `src-tauri/src/db.rs` | `kb_chunks.md_start` / `md_end` in CREATE + ALTER migrate |
| `src-tauri/src/repo/knowledge.rs` | `ChunkText` offsets; strip alignment; insert/replace/load/search |
| `src-tauri/src/models.rs` | `KbChunkHit` / `Citation` optional offsets |
| `src-tauri/src/knowledge.rs` | `hits_to_citations` + `index_note_inner` row tuples |
| `src/api/tauri.ts` | TS `Citation` / `KbChunkHit` fields |
| `src/utils/utf8Slice.ts` | Byte-safe slice (+ optional plain-ish strip for match) |
| `src/utils/noteReveal.ts` | Find text range in ProseMirror doc; heading fallback helpers |
| `tests/utf8-slice.test.mjs` | Unit tests for `utf8Slice` |
| `src/components/NoteEditor.vue` | `reveal` prop → select + scroll + 2s highlight |
| `src/index/index.vue` | `pendingReveal` + pass to NoteEditor |
| `src/components/KnowledgeView.vue` | Emit open payload with offsets; rebuild hint copy |

---

### Task 1: Schema + chunk offsets (TDD)

**Files:**
- Modify: `src-tauri/src/db.rs` (`kb_chunks` CREATE + migrate after batch)
- Modify: `src-tauri/src/repo/knowledge.rs` (`ChunkText`, `chunk_markdown`, strip helpers, tests)

**Interfaces:**
- Consumes: existing `chunk_markdown` / `strip_*` / `parse_segments` / `split_long_text`
- Produces:
  - `ChunkText { heading, content, md_start: i64, md_end: i64 }`
  - Valid chunk: `0 <= md_start < md_end <= md.len() as i64`; else `-1, -1`
  - DB columns `md_start` / `md_end` INTEGER NOT NULL DEFAULT -1

- [ ] **Step 1: Add failing offset tests** (in `repo/knowledge.rs` `mod tests`, after Chinese strip tests)

```rust
    #[test]
    fn chunk_markdown_offsets_cover_raw_markdown() {
        let md = "见 **输出比较** 与 [PWM 模式](http://example.com/pwm)\n";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks.len(), 1);
        let c = &chunks[0];
        assert!(c.md_start >= 0 && c.md_end > c.md_start);
        assert!((c.md_end as usize) <= md.len());
        let slice = &md[c.md_start as usize..c.md_end as usize];
        assert!(slice.contains("**输出比较**") || slice.contains("输出比较"));
        assert!(slice.contains("PWM") || c.content.contains("PWM 模式"));
        assert_eq!(c.content.trim(), "见 输出比较 与 PWM 模式");
    }

    #[test]
    fn chunk_markdown_long_split_offsets_non_overlapping() {
        let para: String = "字".repeat(1200);
        let md = format!("# H\n\n{para}\n");
        let chunks = chunk_markdown(&md);
        assert!(chunks.len() >= 2);
        let mut prev_end = -1i64;
        for c in &chunks {
            assert!(c.md_start >= 0 && c.md_end > c.md_start, "{c:?}");
            assert!((c.md_end as usize) <= md.len());
            assert!(c.md_start >= prev_end, "overlap/disorder: prev_end={prev_end} {:?}", c);
            prev_end = c.md_end;
        }
    }
```

- [ ] **Step 2: Run tests — expect FAIL** (no `md_start` field yet)

```powershell
npm run tauri:test -- knowledge::tests::chunk_markdown_offsets_cover_raw_markdown knowledge::tests::chunk_markdown_long_split_offsets_non_overlapping
```

Expected: compile fail or assertion fail on missing fields / `-1` offsets.

- [ ] **Step 3: Schema — extend CREATE + ALTER**

In `db.rs` `kb_chunks` CREATE TABLE add:

```sql
md_start INTEGER NOT NULL DEFAULT -1,
md_end INTEGER NOT NULL DEFAULT -1,
```

After the `kb_chunks` / `kb_meta` batch (same migrate function), add pragma-style column ensure (match existing todos/notes pattern):

```rust
    // kb_chunks 原文偏移（引用来源跳转）；旧库缺列则补
    {
        let mut stmt = conn.prepare("PRAGMA table_info(kb_chunks)")?;
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(|c| c.ok())
            .collect();
        if !cols.iter().any(|c| c == "md_start") {
            conn.execute(
                "ALTER TABLE kb_chunks ADD COLUMN md_start INTEGER NOT NULL DEFAULT -1",
                [],
            )?;
        }
        if !cols.iter().any(|c| c == "md_end") {
            conn.execute(
                "ALTER TABLE kb_chunks ADD COLUMN md_end INTEGER NOT NULL DEFAULT -1",
                [],
            )?;
        }
    }
```

Extend `kb_chunks_and_meta_schema_exist` (or add sibling test) to assert both columns exist via `pragma_table_info`.

- [ ] **Step 4: Implement offset-aware chunking**

Replace `ChunkText` and pipeline roughly as follows (keep public `chunk_markdown` signature returning `Vec<ChunkText>`):

```rust
pub struct ChunkText {
    pub heading: String,
    pub content: String,
    /// UTF-8 byte offset into source md; -1 = unknown
    pub md_start: i64,
    pub md_end: i64,
}

struct RawSegment {
    heading: String,
    text: String,
    is_code: bool,
    /// byte offsets into the full markdown document
    md_start: usize,
    md_end: usize,
}
```

**`parse_segments`:** do not rely on `md.lines()` alone for offsets. Walk `md` with a cursor (`while let Some(...)` on remaining slice, or iterate `split_inclusive('\n')` and track `offset`). Each paragraph / code segment records `[md_start, md_end)` covering its original bytes (include trailing newlines that belong to the segment consistently; document choice in a one-line comment).

**Aligned strip:** add helper used only for non-code segments:

```rust
/// Strip markdown markers; `origins[i]` = byte offset in `src` of the i-th char of `text`.
fn strip_markdown_aligned(src: &str) -> (String, Vec<usize>) {
    // Implement by adapting strip_markdown_line / strip_inline_links / strip_emphasis
    // to emit chars while recording src.char_indices().offset for each kept char.
    // Empty result → ("".into(), vec![])
}
```

Minimum viable approach if full rewrite of strip is too large in one pass:

1. Keep existing `strip_markdown` for `content` string.
2. Build origins by scanning `src` and `stripped` with a dual pointer that advances both when chars match, and advances only `src` when at a marker consumed by strip (must share the same strip rules — prefer one implementation that both builds text and origins).

**`split_long_text`:** change to return pieces with char ranges, or add `split_long_text_ranges` returning `(String, start_char, end_char)`. Map via `origins`:

```rust
fn piece_offsets(origins: &[usize], start_char: usize, end_char: usize, src_len: usize) -> (i64, i64) {
    if start_char >= origins.len() || end_char == 0 || end_char > origins.len() {
        return (-1, -1);
    }
    let start = origins[start_char] as i64;
    // end = byte after last char: find char_indices of src or use next origin / src_len
    let end = if end_char < origins.len() {
        origins[end_char] as i64
    } else {
        src_len as i64
    };
    if start < end { (start, end) } else { (-1, -1) }
}
```

**Buffer merge:** track `buf_md_start` / `buf_md_end` alongside `buf` / `buf_heading`. On first piece set both; on append extend `buf_md_end`. `flush_buffer` writes them onto `ChunkText` (validate then or `-1,-1`).

**Code blocks:** `md_start`/`md_end` = segment offsets; splits subdivide the segment byte range proportionally by char split (same origins = identity map on raw code text).

**Validate helper:**

```rust
fn clamp_offsets(md_len: usize, start: i64, end: i64) -> (i64, i64) {
    if start >= 0 && end > start && (end as usize) <= md_len {
        (start, end)
    } else {
        (-1, -1)
    }
}
```

Update **all** existing `ChunkText { heading, content }` construction sites in this file to include offsets (tests that only check `content` keep working).

- [ ] **Step 5: Run offset + existing chunk tests — expect PASS**

```powershell
npm run tauri:test -- knowledge::tests::
```

Expected: all `chunk_markdown_*` and new offset tests green.

- [ ] **Step 6: Commit** (only if execution approved with commits)

```bash
git add src-tauri/src/db.rs src-tauri/src/repo/knowledge.rs
git commit -m "feat(kb): store md byte offsets on chunks at index time"
```

---

### Task 2: Persist offsets through ask/citation pipeline

**Files:**
- Modify: `src-tauri/src/repo/knowledge.rs` (`insert_chunk`, `replace_note_chunks`, `ChunkRow`, `load_chunks_with_notes`, `to_hit`)
- Modify: `src-tauri/src/models.rs` (`KbChunkHit`, `Citation`)
- Modify: `src-tauri/src/knowledge.rs` (`index_note_inner` row tuples, `hits_to_citations`)
- Modify: `src/api/tauri.ts` (`Citation`, `KbChunkHit`)

**Interfaces:**
- Consumes: `ChunkText.md_start` / `md_end` from Task 1
- Produces:
  - `insert_chunk(..., md_start: i64, md_end: i64)`
  - `replace_note_chunks` row tuple adds `(md_start, md_end)` as last two fields (or named struct — prefer extending the tuple consistently)
  - `KbChunkHit { ..., md_start: Option<i64>, md_end: Option<i64> }`
  - `Citation { ..., md_start: Option<i64>, md_end: Option<i64> }` with serde `null` when absent
  - TS: `md_start: number | null`, `md_end: number | null`

- [ ] **Step 1: Extend models**

```rust
// KbChunkHit + Citation — add:
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md_start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md_end: Option<i64>,
```

Helper:

```rust
fn opt_offset(v: i64) -> Option<i64> {
    if v >= 0 { Some(v) } else { None }
}
```

- [ ] **Step 2: INSERT / replace / load**

`insert_chunk` SQL columns include `md_start, md_end`.

`replace_note_chunks` rows type becomes 9-tuple (or keep 7 and add 2 — update every call site in `knowledge.rs` `index_note_inner` both Ok/Err branches):

```rust
(
    i as i64,
    c.heading.as_str(),
    c.content.as_str(),
    dim,
    cfg.model.as_str(),
    emb,
    None, // or Some(err)
    c.md_start,
    c.md_end,
)
```

`ChunkRow` + SELECT list add `c.md_start, c.md_end`. `to_hit` maps with `opt_offset`.

Update any unit test that calls `replace_note_chunks` / `insert_chunk` with the new args (grep `replace_note_chunks` in `knowledge.rs` tests).

- [ ] **Step 3: `hits_to_citations`**

```rust
fn hits_to_citations(hits: &[KbChunkHit]) -> Vec<Citation> {
    hits.iter()
        .enumerate()
        .map(|(i, h)| Citation {
            index: (i + 1) as i64,
            note_id: h.note_id,
            note_title: h.note_title.clone(),
            folder_path: h.folder_path.clone(),
            heading: h.heading.clone(),
            snippet: snippet_of(&h.content),
            md_start: h.md_start,
            md_end: h.md_end,
        })
        .collect()
}
```

- [ ] **Step 4: TypeScript types**

```typescript
export interface Citation {
  index: number
  note_id: number
  note_title: string
  folder_path: string
  heading: string
  snippet: string
  md_start?: number | null
  md_end?: number | null
}

export interface KbChunkHit {
  // ...existing...
  md_start?: number | null
  md_end?: number | null
}
```

- [ ] **Step 5: Run Rust tests touching insert/replace**

```powershell
npm run tauri:test -- knowledge::
```

Expected: PASS.

- [ ] **Step 6: Commit** (if approved)

```bash
git add src-tauri/src/repo/knowledge.rs src-tauri/src/models.rs src-tauri/src/knowledge.rs src/api/tauri.ts
git commit -m "feat(kb): thread citation md offsets through ask pipeline"
```

---

### Task 3: UTF-8 slice + ProseMirror reveal helpers

**Files:**
- Create: `src/utils/utf8Slice.ts`
- Create: `src/utils/noteReveal.ts`
- Create: `tests/utf8-slice.test.mjs`
- Modify: `package.json` (`test:unit` script to include the new test file)

**Interfaces:**
- Consumes: note markdown string; byte offsets; ProseMirror `Node`
- Produces:
  - `utf8ByteSlice(s: string, start: number, end: number): string`
  - `offsetsValid(start: number | null | undefined, end: number | null | undefined, byteLen: number): boolean`
  - `plainForMatch(mdSlice: string): string` — light strip for matching (remove `**`/`*`/`~~`/`_`, `[text](url)` → `text`)
  - `findTextRangeInDoc(doc: Node, needle: string): { from: number; to: number } | null`
  - `findHeadingPosInDoc(doc: Node, headingLeaf: string): number | null` — last segment of `"A / B / C"` → search heading text `C`

- [ ] **Step 1: Write `utf8Slice.ts`**

```typescript
/** UTF-8 byte length of a JS string */
export function utf8ByteLength(s: string): number {
  return new TextEncoder().encode(s).length
}

/** Slice `s` by UTF-8 byte offsets [start, end). Clamps; empty if invalid. */
export function utf8ByteSlice(s: string, start: number, end: number): string {
  if (!Number.isFinite(start) || !Number.isFinite(end) || start < 0 || end <= start) return ''
  const bytes = new TextEncoder().encode(s)
  if (start >= bytes.length) return ''
  const lo = Math.min(start, bytes.length)
  const hi = Math.min(end, bytes.length)
  if (lo >= hi) return ''
  return new TextDecoder('utf-8', { fatal: false }).decode(bytes.subarray(lo, hi))
}

export function offsetsValid(
  start: number | null | undefined,
  end: number | null | undefined,
  byteLen: number,
): boolean {
  return (
    typeof start === 'number' &&
    typeof end === 'number' &&
    start >= 0 &&
    end > start &&
    end <= byteLen
  )
}

/** Enough strip for Crepe textContent match; not a full Markdown parser. */
export function plainForMatch(mdSlice: string): string {
  let s = mdSlice
  s = s.replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
  s = s.replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
  s = s.replace(/(\*\*|__|~~)(.*?)\1/g, '$2')
  s = s.replace(/([*_])(.*?)\1/g, '$2')
  s = s.replace(/`([^`]+)`/g, '$1')
  return s.replace(/\s+/g, ' ').trim()
}
```

- [ ] **Step 2: Write failing/passing unit test**

`tests/utf8-slice.test.mjs` — load module via same `loadModule` pattern as `kb-ask-decorate.test.mjs`:

```javascript
import assert from 'node:assert/strict'
import test from 'node:test'
// ... loadModule helper copy or shared ...

const { utf8ByteSlice, utf8ByteLength, offsetsValid, plainForMatch } = loadModule('src/utils/utf8Slice.ts')

test('utf8ByteSlice cuts on Chinese char boundaries', () => {
  const s = '见 **输出**'
  const bytes = new TextEncoder().encode(s)
  // slice covering 「输出」 only — compute via encoder
  const full = new TextEncoder().encode(s)
  const start = full.indexOf(0xe8) // first byte of 输 — fragile; prefer:
  const prefix = new TextEncoder().encode('见 **').length
  const end = prefix + new TextEncoder().encode('输出').length
  assert.equal(utf8ByteSlice(s, prefix, end), '输出')
  assert.ok(utf8ByteLength(s) > s.length) // Chinese → bytes > UTF-16 code units often
})

test('offsetsValid rejects -1 and inverted', () => {
  assert.equal(offsetsValid(-1, 10, 100), false)
  assert.equal(offsetsValid(0, 0, 100), false)
  assert.equal(offsetsValid(0, 5, 5), true)
})

test('plainForMatch strips emphasis and links', () => {
  assert.equal(plainForMatch('见 **输出比较** 与 [PWM](http://x)'), '见 输出比较 与 PWM')
})
```

Add to `package.json`:

```json
"test:utf8-slice": "node --test tests/utf8-slice.test.mjs",
"test:unit": "node --test tests/security.test.mjs tests/kb-ask-decorate.test.mjs tests/utf8-slice.test.mjs",
```

- [ ] **Step 3: Run unit test**

```powershell
npm run test:utf8-slice
```

Expected: PASS.

- [ ] **Step 4: Write `noteReveal.ts`**

```typescript
import type { Node as ProseNode } from '@milkdown/kit/prose/model'

export function findTextRangeInDoc(
  doc: ProseNode,
  needle: string,
): { from: number; to: number } | null {
  const n = needle.trim()
  if (!n) return null
  let from: number | null = null
  let to: number | null = null
  let remaining = n
  // Walk text nodes; support needle spanning nodes by consuming remaining prefix
  doc.descendants((node, pos) => {
    if (from != null) return false
    if (!node.isText || !node.text) return
    const text = node.text
    if (remaining === n) {
      const idx = text.indexOf(n)
      if (idx >= 0) {
        from = pos + idx
        to = from + n.length
        return false
      }
      // try start of multi-node match
      for (let i = 0; i < text.length; i++) {
        if (n.startsWith(text.slice(i))) {
          from = pos + i
          remaining = n.slice(text.length - i)
          return
        }
      }
    } else {
      if (!text.startsWith(remaining) && !remaining.startsWith(text)) {
        from = null
        remaining = n
        return
      }
      if (remaining.startsWith(text)) {
        remaining = remaining.slice(text.length)
        if (!remaining) {
          to = pos + text.length
          return false
        }
      } else if (text.startsWith(remaining)) {
        to = pos + remaining.length
        remaining = ''
        return false
      }
    }
  })
  if (from != null && to != null && remaining === '') return { from, to }
  // Fallback: single-node search only (if multi-node logic incomplete, at least first indexOf on concatenated)
  let acc = ''
  const map: number[] = []
  doc.descendants((node, pos) => {
    if (!node.isText || !node.text) return
    for (let i = 0; i < node.text.length; i++) {
      map.push(pos + i)
      acc += node.text[i]
    }
  })
  const idx = acc.indexOf(n)
  if (idx < 0) return null
  return { from: map[idx], to: map[idx + n.length - 1]! + 1 }
}

export function headingLeaf(headingChain: string): string {
  const parts = headingChain.split('/').map((s) => s.trim()).filter(Boolean)
  return parts[parts.length - 1] ?? ''
}

export function findHeadingPosInDoc(doc: ProseNode, headingChain: string): number | null {
  const leaf = headingLeaf(headingChain)
  if (!leaf) return null
  let found: number | null = null
  doc.descendants((node, pos) => {
    if (found != null) return false
    if (node.type.name.startsWith('heading') && node.textContent.trim() === leaf) {
      found = pos + 1
      return false
    }
  })
  return found
}
```

(If multi-node walk is error-prone in review, keep **only** the concatenate+map fallback — simpler and enough for MVP.)

- [ ] **Step 5: Commit** (if approved)

```bash
git add src/utils/utf8Slice.ts src/utils/noteReveal.ts tests/utf8-slice.test.mjs package.json
git commit -m "feat(kb): utf8 slice and note reveal helpers for citation jump"
```

---

### Task 4: NoteEditor reveal + index / KnowledgeView wiring

**Files:**
- Modify: `src/components/NoteEditor.vue`
- Modify: `src/index/index.vue`
- Modify: `src/components/KnowledgeView.vue`

**Interfaces:**
- Consumes: Task 2 Citation fields; Task 3 helpers
- Produces:
  - `OpenNotePayload = { noteId: number; mdStart?: number | null; mdEnd?: number | null; heading?: string }`
  - `KnowledgeView` emit `'open-note'` with `OpenNotePayload` (breaking vs bare `number` — update sole listener in `index.vue`)
  - `NoteEditor` prop `reveal: { mdStart: number; mdEnd: number; heading?: string } | null`
  - emit `'reveal-done'` when attempt finished (success or silent fail) so parent clears pending

- [ ] **Step 1: KnowledgeView — open helper + emit shape**

```typescript
export type OpenNotePayload = {
  noteId: number
  mdStart?: number | null
  mdEnd?: number | null
  heading?: string
}

const emit = defineEmits<{
  (e: 'open-note', payload: OpenNotePayload): void
}>()

function openCitation(c: Pick<Citation, 'note_id' | 'md_start' | 'md_end' | 'heading'>) {
  emit('open-note', {
    noteId: c.note_id,
    mdStart: c.md_start ?? null,
    mdEnd: c.md_end ?? null,
    heading: c.heading || undefined,
  })
}
```

Replace `@click="emit('open-note', g.note_id)"` / `c.note_id` with `openCitation` (for group header use first citation in group for offsets, or open note-only if group has mixed — **use the clicked snippet's citation**; group title click → first cite in group).

**Answer `[n]` clicks:** change `onAnswerClick` to resolve citation by index from that message and call `openCitation(c)` (still optional flash on cite card). Spec requires `[n]` → open note + locate.

- [ ] **Step 2: index.vue pending reveal**

```typescript
type NoteReveal = {
  noteId: number
  mdStart: number
  mdEnd: number
  heading?: string
}
const pendingReveal = ref<NoteReveal | null>(null)

function onOpenNoteById(payload: number | { noteId: number; mdStart?: number | null; mdEnd?: number | null; heading?: string }) {
  const noteId = typeof payload === 'number' ? payload : payload.noteId
  activeNoteId.value = noteId
  activeView.value = 'notes'
  if (typeof payload !== 'number' && typeof payload.mdStart === 'number' && typeof payload.mdEnd === 'number' && payload.mdStart >= 0 && payload.mdEnd > payload.mdStart) {
    pendingReveal.value = {
      noteId,
      mdStart: payload.mdStart,
      mdEnd: payload.mdEnd,
      heading: payload.heading,
    }
  } else {
    pendingReveal.value = null
  }
}
```

Template:

```vue
<NoteEditor
  :note="activeNote"
  :reveal="pendingReveal && pendingReveal.noteId === activeNote?.id ? pendingReveal : null"
  @save="onSaveNote"
  @delete="onDeleteNote"
  @reveal-done="pendingReveal = null"
/>
```

- [ ] **Step 3: NoteEditor — apply reveal after Crepe ready**

Add prop + emit:

```typescript
const props = defineProps<{
  note: Readonly<Note> | null
  reveal?: { mdStart: number; mdEnd: number; heading?: string } | null
}>()
const emit = defineEmits<{
  (e: 'save', id: number, title: string, content: string): void
  (e: 'delete', id: number): void
  (e: 'reveal-done'): void
}>()
```

After Crepe create / content sync succeeds (same place remount finishes — watch `reveal` + `note.id` with `flush: 'post'`):

```typescript
import { utf8ByteSlice, utf8ByteLength, offsetsValid, plainForMatch } from '../utils/utf8Slice'
import { findTextRangeInDoc, findHeadingPosInDoc } from '../utils/noteReveal'
import { Decoration, DecorationSet } from '@milkdown/kit/prose/view'
// If Decoration import path differs in this Milkdown version, use a CSS class on
// temporary TextSelection only (selection highlight) + scrollIntoView — acceptable MVP.

async function applyReveal() {
  const r = props.reveal
  const note = props.note
  if (!r || !note || !crepe) {
    if (r) emit('reveal-done')
    return
  }
  const md = localContent.value || note.content || ''
  const byteLen = utf8ByteLength(md)
  try {
    await crepe.editor.action((ctx) => {
      const view = ctx.get(editorViewCtx)
      if (offsetsValid(r.mdStart, r.mdEnd, byteLen)) {
        const raw = utf8ByteSlice(md, r.mdStart, r.mdEnd)
        const needle = plainForMatch(raw)
        const range = needle ? findTextRangeInDoc(view.state.doc, needle) : null
        if (range) {
          const tr = view.state.tr.setSelection(TextSelection.create(view.state.doc, range.from, range.to)).scrollIntoView()
          view.dispatch(tr)
          // Optional: add meta + plugin decoration for 2s; else rely on native selection
          return
        }
        const hPos = r.heading ? findHeadingPosInDoc(view.state.doc, r.heading) : null
        if (hPos != null) {
          const sel = TextSelection.findFrom(view.state.doc.resolve(hPos), 1, true)
          if (sel) view.dispatch(view.state.tr.setSelection(sel).scrollIntoView())
        }
      }
    })
  } finally {
    emit('reveal-done')
  }
}

watch(
  () => [props.note?.id, props.reveal?.mdStart, props.reveal?.mdEnd, crepeReadyFlag] as const,
  () => {
    void nextTick(() => void applyReveal())
  },
)
```

Implement `crepeReadyFlag` as a `ref(false)` set `true` at end of successful Crepe mount, `false` on destroy — **do not** call reveal before mount (spec gate).

Highlight ~2s: either keep selection (simplest) or add a one-shot Decoration plugin; if Decoration wiring fights Crepe, **native selection for 2s then collapse to caret at `from`** is enough.

Mode note: if `mode !== 'wysiwyg'`, switch to wysiwyg once for reveal, or scroll source textarea by approximating line from byte offset — spec allows wysiwyg-primary; **force wysiwyg for reveal** then leave mode as user had if you stored previous (optional). Minimal: only reveal when `mode === 'wysiwyg'`; if source/split, switch to wysiwyg for this reveal.

- [ ] **Step 4: Rebuild hint copy**

Merge into rebuild button `title` (and any visible hint near it):

```
清空并重切全部笔记片段。升级后若「来源」曾乱码或要点来源跳到原文位置，请点一次重建（编辑保存也会重切该篇）。
```

- [ ] **Step 5: Typecheck / unit**

```powershell
npm run test:unit
npm run build
```

Expected: `vue-tsc` + vite build OK; unit tests green.

- [ ] **Step 6: Manual checklist**

1. Rebuild index (or edit-save one note).
2. Ask a question that cites a mid-document chunk.
3. Click snippet → notes view opens, viewport near quote, selection visible.
4. Click `[n]` in answer → same.
5. Old session without offsets → opens note, no error.

- [ ] **Step 7: Commit** (if approved)

```bash
git add src/components/NoteEditor.vue src/index/index.vue src/components/KnowledgeView.vue
git commit -m "feat(kb): open note and reveal citation span from offsets"
```

---

## Spec coverage (self-review)

| Spec item | Task |
|-----------|------|
| `md_start`/`md_end` columns + migrate | T1 |
| `chunk_markdown` alignment / merge / code / validate | T1 |
| Persist on index + Citation / ask | T2 |
| UTF-8 byte slice (no JS string byte index) | T3 |
| open-note protocol + NoteEditor reveal + 2s highlight | T4 |
| Fail silent → heading → open only | T4 |
| `[n]` and 来源 both open+locate | T4 |
| Rebuild hint copy | T4 |
| No auto full rebuild / no retrieval changes | Global Constraints |
| Rust + unit tests | T1–T3 |
| Dirty note still reveal | T4 (`localContent`) |

## Placeholder / consistency check

- Tuple arity for `replace_note_chunks` is defined as +2 fields; all call sites listed under T2.
- Emit type changes from `number` to `OpenNotePayload`; `onOpenNoteById` accepts both during transition.
- No TBD left for offset semantics (UTF-8 bytes, half-open, `-1` / `null`).
