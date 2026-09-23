<script setup lang="ts">
import { computed, inject, nextTick, ref, watch } from 'vue'
import {
  Check,
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  FolderPlus,
  StickyNote,
} from 'lucide-vue-next'
import type { Note, NoteFolder } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useFolderChildren } from '../composables/useNoteFolders'
import { parseTimestamp } from '../utils/time'
import AppSelect, { type AppSelectOption } from './AppSelect.vue'
import ConfirmDialog from './ConfirmDialog.vue'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'

const props = defineProps<{
  notes: readonly Note[]
  activeNoteId: number | null
  selectedFolderId?: number | null
}>()

const emit = defineEmits<{
  (e: 'select-note', id: number): void
  (e: 'select-folder', id: number | null): void
}>()

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})

const { childrenOf, flatFolderOptions, descendantIds, ancestorIds } = useFolderChildren(
  computed(() => store.state.folders),
)

const expanded = ref<Set<number>>(new Set())
const treeBodyRef = ref<HTMLElement | null>(null)

watch(
  () => props.activeNoteId,
  (id) => {
    if (id == null) return
    const note = props.notes.find((n) => n.id === id)
    if (note?.folder_id == null) return
    const next = new Set(expanded.value)
    for (const aid of ancestorIds(note.folder_id)) next.add(aid)
    expanded.value = next
  },
  { immediate: true },
)

type TreeRow =
  | { kind: 'folder'; folder: NoteFolder; depth: number; hasChildren: boolean }
  | { kind: 'note'; note: Note; depth: number }

function notesInFolder(folderId: number | null): Note[] {
  return props.notes
    .filter((n) => (n.folder_id ?? null) === folderId)
    .sort((a, b) => parseTimestamp(b.updated_at) - parseTimestamp(a.updated_at))
}

function folderHasChildren(folderId: number): boolean {
  return (
    (childrenOf.value.get(folderId) ?? []).length > 0 ||
    props.notes.some((n) => n.folder_id === folderId)
  )
}

const treeRows = computed((): TreeRow[] => {
  const rows: TreeRow[] = []
  const walk = (parentId: number | null, depth: number) => {
    const kids = childrenOf.value.get(parentId) ?? []
    for (const f of kids) {
      rows.push({
        kind: 'folder',
        folder: f,
        depth,
        hasChildren: folderHasChildren(f.id),
      })
      if (expanded.value.has(f.id)) {
        walk(f.id, depth + 1)
        for (const n of notesInFolder(f.id)) {
          rows.push({ kind: 'note', note: n, depth: depth + 1 })
        }
      }
    }
  }
  walk(null, 0)
  for (const n of notesInFolder(null)) {
    rows.push({ kind: 'note', note: n, depth: 0 })
  }
  return rows
})

function isFolderSelected(id: number): boolean {
  return props.selectedFolderId === id
}

function toggleExpand(id: number) {
  const next = new Set(expanded.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  expanded.value = next
}

function onFolderClick(folder: NoteFolder, e?: Event) {
  if (suppressFolderClick) {
    suppressFolderClick = false
    return
  }
  if (e && (e.target as HTMLElement | null)?.closest('.tree-chevron, .edit-row, [data-no-drag]')) {
    return
  }
  toggleExpand(folder.id)
  emit('select-folder', folder.id)
}

function onChevronClick(id: number, e: Event) {
  e.stopPropagation()
  toggleExpand(id)
}

function onNoteClick(note: Note) {
  emit('select-note', note.id)
  if (note.folder_id != null) emit('select-folder', note.folder_id)
  else emit('select-folder', null)
}

// ---- Inline create / rename ----
type EditState =
  | { mode: 'create'; parentId: number | null }
  | { mode: 'rename'; folder: NoteFolder }

const editing = ref<EditState | null>(null)
const editName = ref('')
const editSort = ref(1)
const editInputRef = ref<HTMLInputElement | null>(null)

function bindEditInput(el: unknown) {
  editInputRef.value = (el as HTMLInputElement | null) ?? null
}

async function focusEditInput() {
  await nextTick()
  editInputRef.value?.focus()
  editInputRef.value?.select()
}

function startCreate(parentId: number | null = null) {
  editing.value = { mode: 'create', parentId }
  editName.value = ''
  const siblings = childrenOf.value.get(parentId) ?? []
  editSort.value = siblings.length + 1
  if (parentId != null) {
    const next = new Set(expanded.value)
    next.add(parentId)
    expanded.value = next
  }
  void focusEditInput()
}

function startRename(folder: NoteFolder) {
  editing.value = { mode: 'rename', folder }
  editName.value = folder.name
  editSort.value = folder.sort_order
  void focusEditInput()
}

function cancelEdit() {
  editing.value = null
  editName.value = ''
}

function bumpSort(delta: number) {
  editSort.value = Math.max(1, (editSort.value || 1) + delta)
}

function onSortStepPointer(e: MouseEvent) {
  const el = e.currentTarget as HTMLElement
  const mid = el.getBoundingClientRect().height / 2
  bumpSort(e.offsetY < mid ? 1 : -1)
}

async function applyRootSort(folderId: number, desiredSort: number) {
  const roots = [...(childrenOf.value.get(null) ?? [])]
  const others = roots.filter((f) => f.id !== folderId).map((f) => f.id)
  const pos = Math.max(0, Math.min(others.length, Math.floor(desiredSort) - 1))
  others.splice(pos, 0, folderId)
  await store.reorderFolders(others)
}

async function commitEdit() {
  const name = editName.value.trim()
  if (!name) {
    showToast('文件夹名称不能为空')
    return
  }
  const state = editing.value
  if (!state) return
  try {
    if (state.mode === 'create') {
      const folder = await store.createFolder(state.parentId, name, editSort.value)
      if (state.parentId == null) {
        await applyRootSort(folder.id, editSort.value)
      }
      emit('select-folder', folder.id)
    } else {
      const folder = state.folder
      if (name !== folder.name) {
        await store.renameFolder(folder.id, name)
      }
      if (folder.parent_id == null && editSort.value !== folder.sort_order) {
        await applyRootSort(folder.id, editSort.value)
      }
    }
    cancelEdit()
  } catch (err) {
    showToast(String(err))
  }
}

function onFolderDblClick(folder: NoteFolder, e: MouseEvent) {
  e.preventDefault()
  e.stopPropagation()
  startRename(folder)
}

// ---- Pointer reorder (root folders only) ----
const dragId = ref<number | null>(null)
const lineTop = ref<number | null>(null)
/** 拖拽松手后吞掉随后的 click，避免误切展开态 */
let suppressFolderClick = false

let dragState: {
  id: number
  fromIndex: number
  insert: number | null
} | null = null

function rootFolderEls(): HTMLElement[] {
  const body = treeBodyRef.value
  if (!body) return []
  return Array.from(body.querySelectorAll<HTMLElement>('[data-root-folder]'))
}

function onRootFolderPointerDown(folder: NoteFolder, e: PointerEvent) {
  if (e.button !== 0 || folder.parent_id != null || editing.value) return
  const target = e.target as HTMLElement | null
  if (target?.closest('button, input, .tree-chevron, [data-no-drag]')) return

  const el = e.currentTarget as HTMLElement
  const startX = e.clientX
  const startY = e.clientY
  let active = false
  let dead = false

  el.addEventListener('pointermove', onMove)
  el.addEventListener('pointerup', onUp)
  el.addEventListener('pointercancel', onUp)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)

  function begin(): boolean {
    try {
      el.setPointerCapture(e.pointerId)
    } catch {
      /* ignore */
    }
    const roots = childrenOf.value.get(null) ?? []
    const fromIndex = roots.findIndex((f) => f.id === folder.id)
    if (fromIndex < 0) return false
    dragState = { id: folder.id, fromIndex, insert: null }
    dragId.value = folder.id
    document.body.classList.add('note-folder-dragging')
    document.getSelection()?.removeAllRanges()
    return true
  }

  function onMove(ev: PointerEvent) {
    if (dead) return
    if (!active) {
      if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < 5) return
      if (!begin()) {
        dead = true
        return
      }
      active = true
    }
    updateDragLine(ev.clientY)
  }

  function onUp() {
    el.removeEventListener('pointermove', onMove)
    el.removeEventListener('pointerup', onUp)
    el.removeEventListener('pointercancel', onUp)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
    if (!active) return
    active = false
    void finishDrag()
  }
}

function updateDragLine(clientY: number) {
  const ds = dragState
  const body = treeBodyRef.value
  if (!ds || !body) return
  const rows = rootFolderEls()
  if (!rows.length) {
    ds.insert = null
    lineTop.value = null
    return
  }
  const bodyRect = body.getBoundingClientRect()
  let insert = rows.length
  let edgeY = rows[rows.length - 1]!.getBoundingClientRect().bottom
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i]!.getBoundingClientRect()
    if (clientY < r.top + r.height / 2) {
      edgeY = r.top
      insert = i
      break
    }
    edgeY = r.bottom
    insert = i + 1
  }
  ds.insert = insert
  lineTop.value = edgeY - bodyRect.top + body.scrollTop
}

async function finishDrag() {
  const ds = dragState
  suppressFolderClick = true
  document.body.classList.remove('note-folder-dragging')
  dragId.value = null
  dragState = null
  lineTop.value = null
  if (!ds || ds.insert == null || ds.fromIndex < 0) return
  const roots = childrenOf.value.get(null) ?? []
  const ids = roots.map((f) => f.id)
  const final = ds.insert > ds.fromIndex ? ds.insert - 1 : ds.insert
  if (final === ds.fromIndex) return
  const without = ids.filter((_, i) => i !== ds.fromIndex)
  without.splice(final, 0, ds.id)
  try {
    await store.reorderFolders(without)
  } catch (err) {
    showToast(String(err))
  }
}

// ---- Context menus ----
const menu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })

function openMenu(e: MouseEvent, items: ContextMenuItem[]) {
  setTimeout(() => {
    menu.value = { visible: true, x: e.clientX, y: e.clientY, items }
  }, 0)
}

function onFolderContext(e: MouseEvent, folder: NoteFolder) {
  e.preventDefault()
  e.stopPropagation()
  openMenu(e, [
    { label: '新建子文件夹', onClick: () => startCreate(folder.id) },
    { label: '重命名', onClick: () => startRename(folder) },
    { label: '移动到…', onClick: () => openMoveDialog(folder) },
    {
      label: '删除',
      danger: true,
      dividerBefore: true,
      onClick: () => askDeleteFolder(folder),
    },
  ])
}

function onNoteContext(e: MouseEvent, note: Note) {
  e.preventDefault()
  e.stopPropagation()
  const items: ContextMenuItem[] = [
    {
      label: '移到顶级',
      onClick: () => void moveNoteToFolder(note.id, null),
    },
    {
      label: '移动到文件夹…',
      dividerBefore: true,
      onClick: () => openNoteMoveDialog(note),
    },
  ]
  const flat = flatFolderOptions(note.folder_id ?? null)
  if (flat.length > 0 && flat.length <= 12) {
    for (let i = 0; i < flat.length; i++) {
      const opt = flat[i]!
      items.push({
        label: opt.label,
        dividerBefore: i === 0,
        onClick: () => void moveNoteToFolder(note.id, Number(opt.value)),
      })
    }
  }
  openMenu(e, items)
}

async function moveNoteToFolder(noteId: number, folderId: number | null) {
  const note = props.notes.find((n) => n.id === noteId)
  if (note && (note.folder_id ?? null) === folderId) return
  try {
    await store.setNoteFolder(noteId, folderId)
  } catch (err) {
    showToast(String(err))
  }
}

const noteMoveTarget = ref<Note | null>(null)
const noteMoveFolderValue = ref('top')

const noteMoveOptions = computed((): AppSelectOption[] => {
  if (!noteMoveTarget.value) return []
  return [{ value: 'top', label: '顶级' }, ...flatFolderOptions(null)]
})

function openNoteMoveDialog(note: Note) {
  noteMoveTarget.value = note
  noteMoveFolderValue.value = note.folder_id == null ? 'top' : String(note.folder_id)
}

async function confirmNoteMove() {
  const note = noteMoveTarget.value
  if (!note) return
  const folderId =
    noteMoveFolderValue.value === 'top' ? null : Number(noteMoveFolderValue.value)
  noteMoveTarget.value = null
  await moveNoteToFolder(note.id, folderId)
}

const deleteTarget = ref<NoteFolder | null>(null)
const deleteMessage = computed(() => {
  const f = deleteTarget.value
  if (!f) return ''
  return `将删除文件夹「${f.name}」。其下子文件夹会移到上一级；该文件夹内的 ${f.notes_count} 篇笔记将变为顶级笔记。笔记不会被删除。`
})

function askDeleteFolder(folder: NoteFolder) {
  deleteTarget.value = folder
}

async function confirmDeleteFolder() {
  const f = deleteTarget.value
  deleteTarget.value = null
  if (!f) return
  try {
    await store.deleteFolder(f.id)
    if (props.selectedFolderId === f.id) emit('select-folder', null)
  } catch (err) {
    showToast(String(err))
  }
}

const moveTarget = ref<NoteFolder | null>(null)
const moveParentValue = ref('root')

const moveOptions = computed((): AppSelectOption[] => {
  const f = moveTarget.value
  if (!f) return []
  const blocked = descendantIds(f.id)
  blocked.add(f.id)
  return [{ value: 'root', label: '根目录' }, ...flatFolderOptions(null, blocked)]
})

function openMoveDialog(folder: NoteFolder) {
  moveTarget.value = folder
  moveParentValue.value = folder.parent_id == null ? 'root' : String(folder.parent_id)
}

async function confirmMoveFolder() {
  const f = moveTarget.value
  if (!f) return
  const newParent =
    moveParentValue.value === 'root' ? null : Number(moveParentValue.value)
  moveTarget.value = null
  if (newParent === f.parent_id) return
  try {
    await store.moveFolder(f.id, newParent)
  } catch (err) {
    showToast(String(err))
  }
}

function showCreateEditAtTop(): boolean {
  return editing.value?.mode === 'create' && editing.value.parentId == null
}

function showCreateEditUnder(folderId: number): boolean {
  return editing.value?.mode === 'create' && editing.value.parentId === folderId
}

function showRenameEdit(folderId: number): boolean {
  return editing.value?.mode === 'rename' && editing.value.folder.id === folderId
}

defineExpose({ flatFolderOptions })
</script>

<template>
  <aside class="nl-tree" aria-label="文件夹">
    <div class="nl-tree-head">
      <span class="nl-tree-label">文件夹</span>
      <button
        class="icon-btn nl-tree-add"
        type="button"
        title="新建文件夹"
        @click="startCreate(null)"
      >
        <FolderPlus :size="14" :stroke-width="2" />
      </button>
    </div>
    <div ref="treeBodyRef" class="nl-tree-body">
      <div
        v-if="lineTop != null"
        class="tree-drop-line"
        :style="{ top: `${lineTop}px` }"
        aria-hidden="true"
      />

      <div v-if="showCreateEditAtTop()" class="edit-row" data-no-drag>
          <input
          :ref="bindEditInput"
          v-model="editName"
          type="text"
          class="edit-name"
          placeholder="文件夹名称"
          @keydown.enter.prevent="commitEdit"
          @keydown.escape.prevent="cancelEdit"
        />
        <div class="sort-box" title="排序">
          <input v-model.number="editSort" type="number" min="1" />
          <button
            type="button"
            class="sort-step"
            aria-label="调整排序"
            @mousedown.prevent="onSortStepPointer"
          >
            <span>▲</span>
            <span>▼</span>
          </button>
        </div>
        <button type="button" class="btn-ok" title="保存" @click="commitEdit">
          <Check :size="14" :stroke-width="2.5" />
        </button>
      </div>

      <template v-for="(row, idx) in treeRows" :key="`${row.kind}-${row.kind === 'folder' ? row.folder.id : row.note.id}-${idx}`">
        <div
          v-if="row.kind === 'folder' && showRenameEdit(row.folder.id)"
          class="edit-row"
          data-no-drag
          :style="{ paddingLeft: `${8 + row.depth * 12}px` }"
        >
          <input
            :ref="bindEditInput"
            v-model="editName"
            type="text"
            class="edit-name"
            @keydown.enter.prevent="commitEdit"
            @keydown.escape.prevent="cancelEdit"
          />
          <div v-if="row.folder.parent_id == null" class="sort-box" title="排序">
            <input v-model.number="editSort" type="number" min="1" />
            <button
              type="button"
              class="sort-step"
              aria-label="调整排序"
              @mousedown.prevent="onSortStepPointer"
            >
              <span>▲</span>
              <span>▼</span>
            </button>
          </div>
          <button type="button" class="btn-ok" title="保存" @click="commitEdit">
            <Check :size="14" :stroke-width="2.5" />
          </button>
        </div>

        <div
          v-else-if="row.kind === 'folder'"
          class="tree-row"
          :class="{
            active: isFolderSelected(row.folder.id),
            dragging: dragId === row.folder.id,
          }"
          :data-root-folder="row.depth === 0 ? String(row.folder.id) : undefined"
          :style="{ paddingLeft: `${8 + row.depth * 12}px` }"
          role="button"
          tabindex="0"
          @click="onFolderClick(row.folder, $event)"
          @dblclick="onFolderDblClick(row.folder, $event)"
          @keydown.enter="onFolderClick(row.folder)"
          @contextmenu="onFolderContext($event, row.folder)"
          @pointerdown="onRootFolderPointerDown(row.folder, $event)"
        >
          <button
            v-if="row.hasChildren"
            class="tree-chevron"
            type="button"
            :aria-label="expanded.has(row.folder.id) ? '收起' : '展开'"
            @click="onChevronClick(row.folder.id, $event)"
          >
            <ChevronDown v-if="expanded.has(row.folder.id)" :size="12" :stroke-width="2.2" />
            <ChevronRight v-else :size="12" :stroke-width="2.2" />
          </button>
          <span v-else class="tree-chevron-spacer" />
          <FolderOpen
            v-if="expanded.has(row.folder.id)"
            class="tree-icon"
            :size="13"
            :stroke-width="1.8"
          />
          <Folder v-else class="tree-icon" :size="13" :stroke-width="1.8" />
          <span class="tree-name" :title="row.folder.name">{{ row.folder.name }}</span>
          <span class="tree-badge">{{ row.folder.notes_count }}</span>
        </div>

        <div
          v-if="row.kind === 'folder' && showCreateEditUnder(row.folder.id)"
          class="edit-row"
          data-no-drag
          :style="{ paddingLeft: `${8 + (row.depth + 1) * 12}px` }"
        >
          <input
            :ref="bindEditInput"
            v-model="editName"
            type="text"
            class="edit-name"
            placeholder="子文件夹名称"
            @keydown.enter.prevent="commitEdit"
            @keydown.escape.prevent="cancelEdit"
          />
          <button type="button" class="btn-ok" title="保存" @click="commitEdit">
            <Check :size="14" :stroke-width="2.5" />
          </button>
        </div>

        <div
          v-else-if="row.kind === 'note'"
          class="tree-row tree-row--note"
          :class="{ active: row.note.id === activeNoteId }"
          :style="{ paddingLeft: `${8 + row.depth * 12}px` }"
          role="button"
          tabindex="0"
          @click="onNoteClick(row.note)"
          @keydown.enter="onNoteClick(row.note)"
          @contextmenu="onNoteContext($event, row.note)"
        >
          <span class="tree-chevron-spacer" />
          <StickyNote class="tree-icon" :size="13" :stroke-width="1.8" />
          <span class="tree-name" :title="row.note.title">{{ row.note.title }}</span>
        </div>
      </template>
    </div>

    <ContextMenu
      :visible="menu.visible"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      @close="menu.visible = false"
    />

    <ConfirmDialog
      :visible="deleteTarget != null"
      title="删除文件夹"
      :message="deleteMessage"
      confirm-text="删除"
      tone="danger"
      @confirm="confirmDeleteFolder"
      @cancel="deleteTarget = null"
    />

    <Teleport to="body">
      <div v-if="moveTarget" class="modal-mask" @click.self="moveTarget = null">
        <div class="modal-card nl-move-card" role="dialog" aria-modal="true">
          <h2 class="nl-move-title">移动「{{ moveTarget.name }}」</h2>
          <p class="nl-move-hint">选择新的父文件夹</p>
          <AppSelect
            v-model="moveParentValue"
            :options="moveOptions"
            aria-label="目标父文件夹"
          />
          <div class="nl-move-foot">
            <button type="button" class="nl-move-btn" @click="moveTarget = null">取消</button>
            <button type="button" class="nl-move-btn nl-move-btn--primary" @click="confirmMoveFolder">
              移动
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <Teleport to="body">
      <div v-if="noteMoveTarget" class="modal-mask" @click.self="noteMoveTarget = null">
        <div class="modal-card nl-move-card" role="dialog" aria-modal="true">
          <h2 class="nl-move-title">移动笔记「{{ noteMoveTarget.title }}」</h2>
          <p class="nl-move-hint">选择目标文件夹</p>
          <AppSelect
            v-model="noteMoveFolderValue"
            :options="noteMoveOptions"
            aria-label="目标文件夹"
          />
          <div class="nl-move-foot">
            <button type="button" class="nl-move-btn" @click="noteMoveTarget = null">取消</button>
            <button type="button" class="nl-move-btn nl-move-btn--primary" @click="confirmNoteMove">
              移动
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </aside>
</template>

<style scoped>
.nl-tree {
  flex: 0 0 auto;
  max-height: 42%;
  min-height: 88px;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  overflow: hidden;
}
.nl-tree-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px 4px;
}
.nl-tree-label {
  font-size: 0.6875em;
  font-weight: 600;
  color: var(--text-3);
  letter-spacing: 0.02em;
}
.nl-tree-add {
  width: 24px;
  height: 24px;
}
.nl-tree-body {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 2px 4px 6px;
}
.tree-row {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 5px 8px 5px 0;
  border-radius: var(--radius-sm);
  cursor: pointer;
  color: var(--text-2);
  transition: background 0.12s, color 0.12s;
  user-select: none;
}
.tree-row:hover {
  background: color-mix(in srgb, var(--brand-50) 70%, transparent);
  color: var(--text-1);
}
.tree-row.active {
  background: var(--brand-50);
  color: var(--brand-500);
  font-weight: 600;
}
.tree-row.dragging {
  opacity: 0.4;
}
.tree-row--note {
  font-weight: 400;
}
.tree-chevron,
.tree-chevron-spacer {
  flex-shrink: 0;
  width: 16px;
  height: 16px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  padding: 0;
  color: inherit;
  cursor: pointer;
  border-radius: 4px;
}
.tree-chevron:hover {
  background: var(--bg-card);
}
.tree-icon {
  flex-shrink: 0;
  opacity: 0.85;
}
.tree-name {
  flex: 1;
  min-width: 0;
  font-size: 0.75em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tree-badge {
  flex-shrink: 0;
  font-size: 0.625em;
  font-weight: 600;
  color: var(--text-3);
  min-width: 1.2em;
  text-align: right;
}
.tree-row.active .tree-badge {
  color: var(--brand-500);
}
.tree-drop-line {
  position: absolute;
  left: 6px;
  right: 6px;
  height: 2px;
  background: var(--brand-500);
  border-radius: 1px;
  pointer-events: none;
  z-index: 2;
}

.edit-row {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 6px;
  border-radius: var(--radius-sm);
  background: var(--brand-50);
  margin: 1px 0;
}
.edit-name {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--brand-500);
  border-radius: var(--radius-sm);
  padding: 4px 8px;
  font-size: 0.75em;
  outline: none;
  background: var(--bg-card);
  color: var(--text-1);
}
.sort-box {
  display: flex;
  align-items: center;
  height: 26px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: var(--bg-card);
  overflow: hidden;
  flex-shrink: 0;
}
.sort-box input {
  width: 36px;
  border: none;
  text-align: center;
  font-size: 0.6875em;
  outline: none;
  background: transparent;
  color: var(--text-1);
  -moz-appearance: textfield;
}
.sort-box input::-webkit-outer-spin-button,
.sort-box input::-webkit-inner-spin-button {
  -webkit-appearance: none;
}
.sort-step {
  width: 18px;
  height: 26px;
  border: none;
  border-left: 1px solid var(--border-soft);
  background: transparent;
  cursor: pointer;
  color: var(--text-3);
  font-size: 7px;
  line-height: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1px;
  padding: 0;
}
.sort-step:hover {
  color: var(--brand-500);
  background: var(--bg-card-soft);
}
.btn-ok {
  width: 26px;
  height: 26px;
  border: none;
  border-radius: var(--radius-sm);
  background: var(--brand-500);
  color: var(--text-on-accent);
  cursor: pointer;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.btn-ok:hover {
  filter: brightness(1.06);
}

.nl-move-card {
  width: min(360px, 92vw);
  padding: 20px 22px;
}
.nl-move-title {
  font-size: 1rem;
  font-weight: 650;
  color: var(--text-1);
  margin: 0 0 6px;
}
.nl-move-hint {
  font-size: 0.8125rem;
  color: var(--text-3);
  margin: 0 0 12px;
}
.nl-move-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}
.nl-move-btn {
  padding: 6px 14px;
  font-size: 0.78rem;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  color: var(--text-2);
  cursor: pointer;
  transition: background 0.18s, color 0.18s, border-color 0.18s;
}
.nl-move-btn:hover {
  color: var(--text-1);
  border-color: var(--border-strong);
}
.nl-move-btn--primary {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: var(--text-on-accent);
  font-weight: 600;
}
.nl-move-btn--primary:hover {
  color: var(--text-on-accent);
  filter: brightness(1.06);
}
</style>
