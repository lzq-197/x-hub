# Task 10 Report: Verification gate

**Branch:** `feature/kb-notes-structure`  
**Tip:** `29a100d` (`feat(kb): show note folder path and sync list filter on open`)  
**Date:** 2026-09-23

## Status: PASS (automated) / needs human (manual UI)

Automated gates green; GUI acceptance checklist left for human.

---

## Step 1: Rust tests — PASS

`npm run tauri:test` failed immediately: script invokes `pwsh`, which is not on PATH on this machine (`'pwsh' 不是内部或外部命令`).

**Actual commands used (Windows PowerShell + mt.exe embed, same pattern as `scripts/cargo-test.ps1`):**

```powershell
# 1) Locate mt.exe
# C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\mt.exe

# 2) Build test artifacts
cargo test --manifest-path src-tauri/Cargo.toml --no-run --message-format=json

# 3) Embed Common-Controls v6 manifest into test exes
mt.exe -nologo -manifest src-tauri\windows\app.manifest -outputresource:"<exe>;#1"
# Embedded 2 exes:
#   app_lib-1218e3ece8cd2af0.exe
#   app-28d18daa1c95679e.exe

# 4) Run tests
cargo test --manifest-path src-tauri/Cargo.toml
```

**Result:** `259 passed; 0 failed; 0 ignored` (lib unit tests), ~0.85s.  
Includes KB-related coverage: `db::note_folders_schema_and_notes_columns`, `repo::folder::*` (cycle + promote-on-delete), `knowledge::import_*`, `index_note_stub_ok`, etc.

---

## Step 2: Frontend build — PASS

```powershell
npm run build
```

**Result:** exit 0  
- `vue-tsc -b` OK  
- `vite build` OK (~1.32s, 3632 modules)  
- Only existing Vite warnings (`INEFFECTIVE_DYNAMIC_IMPORT`); no errors.

---

## Step 3: Automated greps (no GUI) — PASS

| Check | Result |
|-------|--------|
| `src/index/index.vue` nav ids | Only `dashboard` / `todos` / `notes` / `suda` / `chat` — **no** `knowledge` / `kb` |
| Grep `知识库` / `KnowledgeView` under `src/` | no matches |
| Grep `kb_chunks` in `src-tauri/src/db.rs` | no matches |
| Repo-wide `kb_chunks` | only a comment in `knowledge.rs` (“本段无 kb_chunks”) — not a table |

---

## Step 4: Push — PASS

```powershell
git push origin feature/kb-notes-structure
```

**Result:** `378174f..29a100d  feature/kb-notes-structure -> feature/kb-notes-structure`  
Post-push: `origin/feature/kb-notes-structure` synced (`0	0` ahead/behind). Tip remains `29a100d`.

---

## Manual checklist (needs human)

| Item | Status |
|------|--------|
| Create nested folders, rename, move (no cycle) | **needs human** |
| Delete folder: children promote; notes uncategorized; notes still openable | **needs human** |
| Import a small md tree twice (second = updates) | **needs human** |
| Tags + global search still work | **needs human** |
| No sidebar 「知识库」; no Ollama required | **needs human** (sidebar absence also confirmed by grep) |

Agent cannot drive the Tauri GUI; do not treat these as passed.

---

## Spec coverage (this gate)

| Spec item | Verified how |
|-----------|--------------|
| Acceptance / tests | Rust suite PASS (Tasks 3–5 unit coverage exercised) |
| No kb_chunks / KnowledgeView | grep PASS |
| Branch feature/kb-notes-structure | tip + push PASS |
| Frontend build | `npm run build` PASS |

Out of scope (unchanged): RAG, embeddings, KnowledgeView, `kb_chunks`, real `kb_index_note`.

---

## Follow-up fix (2026-09-23): note move without HTML5 drag

**Finding:** Note→folder HTML5 drag (`draggable` + `drop` on tree rows) is likely broken under Tauri `dragDropEnabled` (native file-drop intercept vs WebView HTML5 DnD, same class of issue as AGENTS.md 约定 14/38). There was no alternate UI calling `setNoteFolder` for hand-written notes.

**Fix commit:** `fix(kb): add note move-to-folder actions without relying on HTML5 drag`

| Change | Detail |
|--------|--------|
| `NoteList.vue` | Note row context menu (Suda `setTimeout` open pattern + shared `ContextMenu`): **移到未分类** → `store.setNoteFolder(id, null)`; **移动到文件夹…** → AppSelect dialog; ≤12 folders also listed inline for one-click move |
| `repo/folder.rs` | `delete_promote` wrapped in `unchecked_transaction` (atomic promote + unclassify + delete) |
| Build | `npm run build` PASS |

No KnowledgeView added; HTML5 drag handlers left in place as optional, not required.
