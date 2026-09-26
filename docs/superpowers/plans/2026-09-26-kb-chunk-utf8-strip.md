# KB Chunk UTF-8 Strip Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix Chinese mojibake in `kb_chunks` caused by byte-wise Markdown strippers, add regression tests, and hint users to rebuild the index once.

**Architecture:** Keep `chunk_markdown` / `strip_markdown` pipeline. Rewrite only `strip_inline_links` and `strip_emphasis` to walk UTF-8 by character (byte offsets from `parse_link_text` / `str::find` stay consistent). No schema change; existing `kb_rebuild_index` clears and re-embeds. UI: one short hint near「重建索引」.

**Tech Stack:** Rust (`repo/knowledge.rs` unit tests via `npm run tauri:test`), Vue 3 (`KnowledgeView.vue`)

## Global Constraints

- Spec: `docs/superpowers/specs/2026-09-26-kb-chunk-utf8-strip-design.md`
- **Do not** auto-rebuild on startup; **do not** add Markdown crates
- Windows tests: `npm run tauri:test` only (never bare `cargo test`)
- Commit only when the user asks (or when executing a plan step that says commit **and** the user already approved plan execution that includes commits)

## File map

| Path | Responsibility |
|------|----------------|
| `src-tauri/src/repo/knowledge.rs` | Fix `strip_inline_links` / `strip_emphasis`; add Chinese chunk tests |
| `src/components/KnowledgeView.vue` | Hint copy near rebuild button + minimal style |

---

### Task 1: Failing Chinese strip tests + char-safe strippers

**Files:**
- Modify: `src-tauri/src/repo/knowledge.rs` (`strip_inline_links`, `strip_emphasis`, `mod tests`)

**Interfaces:**
- Consumes: `chunk_markdown`, `parse_link_text` (byte-offset `next` via `char_indices`)
- Produces: UTF-8-safe strip functions; tests `chunk_markdown_preserves_chinese` / `chunk_markdown_strips_chinese_inline_markdown`

- [ ] **Step 1: Add failing tests** (place after `chunk_markdown_strips_inline_markdown`)

```rust
    #[test]
    fn chunk_markdown_preserves_chinese() {
        let md = "定时器计数器与比较寄存器匹配时触发。\n";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].content.contains("定时器"),
            "got: {:?}",
            chunks[0].content
        );
        assert!(
            !chunks[0].content.chars().any(|c| (c as u32) > 0x7f && c.is_ascii_control()),
            "unexpected control chars: {:?}",
            chunks[0].content
        );
        // Mojibake from `bytes[i] as char` often looks like Latin-1 for UTF-8 lead/cont bytes
        assert!(
            !chunks[0].content.contains('\u{00e5}'),
            "looks like byte-as-char mojibake: {:?}",
            chunks[0].content
        );
    }

    #[test]
    fn chunk_markdown_strips_chinese_inline_markdown() {
        let md = "见 **输出比较** 与 [PWM 模式](http://example.com/pwm)\n";
        let chunks = chunk_markdown(md);
        assert_eq!(chunks.len(), 1);
        let c = &chunks[0].content;
        assert!(c.contains("输出比较"), "got: {c:?}");
        assert!(c.contains("PWM 模式"), "got: {c:?}");
        assert!(!c.contains("**"));
        assert!(!c.contains("http://"));
        assert!(!c.contains('\u{00e8}'), "mojibake: {c:?}");
    }
```

- [ ] **Step 2: Run tests — expect FAIL (mojibake / missing 定时器)**

```powershell
npm run tauri:test -- knowledge::tests::chunk_markdown_preserves_chinese knowledge::tests::chunk_markdown_strips_chinese_inline_markdown
```

Expected: FAIL — content missing `定时器` / `输出比较` or containing Latin-1 junk (current `bytes[i] as char` behavior).

If the filter syntax differs on this repo’s `cargo-test.ps1`, run:

```powershell
npm run tauri:test -- --lib chunk_markdown_preserves_chinese
```

- [ ] **Step 3: Replace `strip_inline_links` and `strip_emphasis`**

Replace the two functions in `src-tauri/src/repo/knowledge.rs` with:

```rust
fn strip_inline_links(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if rest.starts_with("![") {
            if let Some((alt, next)) = parse_link_text(&s[i + 2..]) {
                out.push_str(&alt);
                i += 2 + next;
                continue;
            }
        }
        if rest.starts_with('[') {
            if let Some((label, next)) = parse_link_text(&s[i + 1..]) {
                out.push_str(&label);
                i += 1 + next;
                continue;
            }
        }
        let ch = rest.chars().next().expect("i on char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn strip_emphasis(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if rest.starts_with("**") {
            if let Some(end) = s[i + 2..].find("**") {
                out.push_str(&s[i + 2..i + 2 + end]);
                i += 4 + end;
                continue;
            }
        }
        if rest.starts_with("~~") {
            if let Some(end) = s[i + 2..].find("~~") {
                out.push_str(&s[i + 2..i + 2 + end]);
                i += 4 + end;
                continue;
            }
        }
        let ch = rest.chars().next().expect("i on char boundary");
        if ch == '*' || ch == '_' {
            let mark_len = ch.len_utf8();
            if let Some(end) = s[i + mark_len..].find(ch) {
                out.push_str(&s[i + mark_len..i + mark_len + end]);
                i += mark_len * 2 + end;
                continue;
            }
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}
```

Notes for implementer:
- `parse_link_text` returns **byte** length in `next` (from `char_indices`); advancing `i` by `2 + next` stays on a char boundary.
- `str::find` also returns byte indices — keep using them.
- Do **not** leave any `out.push(bytes[i] as char)`.

- [ ] **Step 4: Run related chunk tests — expect PASS**

```powershell
npm run tauri:test -- --lib chunk_markdown
```

Expected: all `chunk_markdown_*` tests PASS, including the two new ones and `chunk_markdown_strips_inline_markdown`.

- [ ] **Step 5: Commit** (only if user asked to commit during execution)

```bash
git add src-tauri/src/repo/knowledge.rs
git commit -m "$(cat <<'EOF'
fix(kb): strip markdown by Unicode scalar, not UTF-8 bytes

Byte-wise push turned Chinese into Latin-1 junk in kb_chunks while notes stayed fine.
EOF
)"
```

---

### Task 2: Rebuild-index hint in KnowledgeView

**Files:**
- Modify: `src/components/KnowledgeView.vue` (header actions + scoped CSS)

**Interfaces:**
- Consumes: existing `.kb-bar-actions` / `.ghost-btn`
- Produces: visible one-line hint; no new API

- [ ] **Step 1: Add hint markup**

Inside `.kb-bar-actions`, **before** the rebuild button, add:

```html
          <span class="kb-rebuild-hint" title="修复后需重建一次，旧片段才会按正确中文重切并重新嵌入">
            来源乱码时请重建索引
          </span>
```

Also set the rebuild button `title` for hover:

```html
          <button
            class="ghost-btn"
            type="button"
            title="清空并重切全部笔记片段；来源曾乱码时点一次即可修复"
            :disabled="rebuilding || statusKind === 'indexing'"
            @click="onRebuild"
          >
```

- [ ] **Step 2: Add styles** (near existing `.kb-bar-actions` rules)

```css
.kb-rebuild-hint {
  font-size: 11px;
  color: var(--text-3);
  max-width: 11em;
  line-height: 1.3;
  margin-right: 4px;
}
```

Do not introduce hard-coded colors; use design tokens only.

- [ ] **Step 3: Smoke-check**

- Open knowledge view in app (or `npm run build` for typecheck if not launching Tauri).
- Confirm hint sits near「重建索引」and does not break bar layout on narrow width (hint may wrap; that is OK).

- [ ] **Step 4: Commit** (only if user asked)

```bash
git add src/components/KnowledgeView.vue
git commit -m "$(cat <<'EOF'
fix(kb): hint rebuild when citation text was garbled
EOF
)"
```

---

### Task 3: Manual acceptance (no code)

**Files:** none

- [ ] **Step 1:** After Tasks 1–2 land in a running build: Knowledge view →「重建索引」→ wait done.
- [ ] **Step 2:** Ask a question that cites a Chinese imported note (e.g. STM32 / PWM).
- [ ] **Step 3:** Confirm「来源」snippet shows readable Chinese (not Latin junk). Opening the note in 速记 still fine.

---

## Spec coverage (self-review)

| Spec item | Task |
|-----------|------|
| Char-safe `strip_inline_links` / `strip_emphasis` | Task 1 |
| Chinese + inline Markdown unit tests | Task 1 |
| No auto full re-embed | (constraint; no task adds it) |
| Rebuild hint copy | Task 2 |
| Manual acceptance after rebuild | Task 3 |

Placeholder scan: none. Types: no new public APIs.
