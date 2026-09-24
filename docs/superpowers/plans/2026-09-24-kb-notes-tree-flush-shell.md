# Notes Tree Flush Inner Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the nested card chrome from `.nl-tree` so the folder tree sits flush on the outer 速记 `.card`.

**Architecture:** One scoped CSS edit in `NoteFolderTree.vue`. No template, store, or API changes.

**Tech Stack:** Vue 3 SFC scoped CSS, existing design tokens

## Global Constraints

- Branch: `feature/kb-notes-structure` only
- Spec: `docs/superpowers/specs/2026-09-24-kb-notes-tree-flush-shell-design.md`
- Do **not** remove outer `.card.note-list`
- Do **not** remove「文件夹」label or FolderPlus
- Do **not** change DnD / selection / import behavior
- No new dependencies

## File map

| Path | Responsibility |
|------|----------------|
| `src/components/NoteFolderTree.vue` | `.nl-tree` styles only |

---

### Task 1: Flush `.nl-tree` shell styles

**Files:**
- Modify: `src/components/NoteFolderTree.vue` (`.nl-tree` rule in `<style scoped>`)

**Interfaces:**
- Consumes: none
- Produces: transparent, borderless `.nl-tree` keeping flex layout

- [x] **Step 1: Locate current rule**

In `NoteFolderTree.vue`, find:

```css
.nl-tree {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  overflow: hidden;
}
```

- [x] **Step 2: Apply flush styles**

Replace with:

```css
.nl-tree {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  /* 无内层卡片壳：border / radius / bg 已去掉，铺在外层 .card.note-list 上 */
}
```

- [x] **Step 3: Visual check**

Open 速记视图：左栏仅外层玻璃卡有描边；内层树无第二层底色/圆角框；「文件夹」+ FolderPlus 仍在；选中行高亮仍清晰。

- [x] **Step 4: Commit**

```powershell
git add src/components/NoteFolderTree.vue
git commit -m "fix(kb): flush notes folder tree inner card shell"
```

---

## Spec coverage

| Spec item | Task |
|-----------|------|
| Remove `.nl-tree` border/radius/bg | T1 |
| Keep folder head + interactions | T1 (untouched) |
| Keep outer `.card` | Global / no change |

## Placeholder scan

None.

## Type consistency

N/A (CSS only).
