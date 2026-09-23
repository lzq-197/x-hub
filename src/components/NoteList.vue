<script setup lang="ts">
import { computed, inject, onMounted, ref, watch } from 'vue'
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  FolderPlus,
  Import,
  Plus,
  StickyNote,
  X,
} from 'lucide-vue-next'
import type { Note, NoteFolder } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { markdownPlainText } from '../utils/markdown'
import { parseTimestamp } from '../utils/time'
import AppSelect, { type AppSelectOption } from './AppSelect.vue'
import ConfirmDialog from './ConfirmDialog.vue'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'

type FolderFilter = 'all' | 'uncategorized' | number

const props = defineProps<{
  notes: readonly Note[]
  activeId: number | null
}>()

const emit = defineEmits<{
  (e: 'select', id: number): void
  /** folderId：当前树选中的真实文件夹；未分类/全部则为 null */
  (e: 'create', folderId: number | null): void
  (e: 'delete', id: number): void
  (e: 'import-request'): void
}>()

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})

// ---- 标签筛选 ----
const activeTagId = ref<number | null>(null)
const tagMap = ref<Map<number, number[]>>(new Map())

onMounted(async () => {
  const rows = await store.loadNoteTagsMap()
  const map = new Map<number, number[]>()
  for (const row of rows) {
    const list = map.get(row.note_id) ?? []
    list.push(row.tag_id)
    map.set(row.note_id, list)
  }
  tagMap.value = map
})

// ---- 文件夹树 ----
const folderFilter = ref<FolderFilter>('all')
/** 默认全部展开 */
const expanded = ref<Set<number>>(new Set())

function emitCreate() {
  const folderId = typeof folderFilter.value === 'number' ? folderFilter.value : null
  emit('create', folderId)
}

watch(
  () => store.state.folders.map((f) => f.id).join(','),
  () => {
    const next = new Set(expanded.value)
    for (const f of store.state.folders) next.add(f.id)
    expanded.value = next
  },
  { immediate: true },
)

const childrenOf = computed(() => {
  const map = new Map<number | null, NoteFolder[]>()
  for (const f of store.state.folders) {
    const key = f.parent_id
    const list = map.get(key) ?? []
    list.push(f)
    map.set(key, list)
  }
  for (const list of map.values()) {
    list.sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name, 'zh'))
  }
  return map
})

const uncategorizedCount = computed(
  () => props.notes.filter((n) => n.folder_id == null).length,
)

const allNotesCount = computed(() => props.notes.length)

interface TreeRow {
  kind: 'all' | 'uncategorized' | 'folder'
  id: number | null
  name: string
  depth: number
  count: number
  folder?: NoteFolder
  hasChildren?: boolean
}

const treeRows = computed((): TreeRow[] => {
  const rows: TreeRow[] = [
    {
      kind: 'all',
      id: null,
      name: '全部笔记',
      depth: 0,
      count: allNotesCount.value,
    },
  ]
  const walk = (parentId: number | null, depth: number) => {
    const kids = childrenOf.value.get(parentId) ?? []
    for (const f of kids) {
      const hasChildren = (childrenOf.value.get(f.id) ?? []).length > 0
      rows.push({
        kind: 'folder',
        id: f.id,
        name: f.name,
        depth,
        count: f.notes_count,
        folder: f,
        hasChildren,
      })
      if (hasChildren && expanded.value.has(f.id)) walk(f.id, depth + 1)
    }
  }
  walk(null, 1)
  rows.push({
    kind: 'uncategorized',
    id: null,
    name: '未分类',
    depth: 1,
    count: uncategorizedCount.value,
  })
  return rows
})

function filterKey(row: TreeRow): FolderFilter {
  if (row.kind === 'all') return 'all'
  if (row.kind === 'uncategorized') return 'uncategorized'
  return row.id as number
}

function isSelected(row: TreeRow): boolean {
  return folderFilter.value === filterKey(row)
}

function selectFolder(row: TreeRow) {
  folderFilter.value = filterKey(row)
}

function toggleExpand(id: number, e: Event) {
  e.stopPropagation()
  const next = new Set(expanded.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  expanded.value = next
}

/** 打开笔记时展开父链；仅在非「全部笔记」时才改筛选（避免从「全部」点开被拽走） */
watch(
  () => props.activeId,
  (id) => {
    if (id == null) return
    const note = props.notes.find((n) => n.id === id)
    if (!note) return
    if (note.folder_id != null) {
      const next = new Set(expanded.value)
      let cur: number | null = note.folder_id
      const byId = new Map(store.state.folders.map((f) => [f.id, f]))
      while (cur != null) {
        next.add(cur)
        cur = byId.get(cur)?.parent_id ?? null
      }
      expanded.value = next
    }
    if (folderFilter.value === 'all') return
    folderFilter.value = note.folder_id == null ? 'uncategorized' : note.folder_id
  },
  { immediate: true },
)

// ---- 列表过滤（文件夹 ∩ 标签）----
const sortedNotes = computed(() => {
  let list = [...props.notes]
  if (folderFilter.value === 'uncategorized') {
    list = list.filter((n) => n.folder_id == null)
  } else if (typeof folderFilter.value === 'number') {
    const fid = folderFilter.value
    list = list.filter((n) => n.folder_id === fid)
  }
  list.sort((a, b) => parseTimestamp(b.updated_at) - parseTimestamp(a.updated_at))
  if (activeTagId.value === null) return list
  return list.filter((n) => tagMap.value.get(n.id)?.includes(activeTagId.value!))
})

function formatTime(iso: string): string {
  const t = new Date(parseTimestamp(iso))
  const now = new Date()
  const diffMs = now.getTime() - t.getTime()
  const diffMin = Math.floor(diffMs / 60000)
  if (diffMin < 1) return '刚刚'
  if (diffMin < 60) return `${diffMin} 分钟前`
  const diffHour = Math.floor(diffMin / 60)
  if (diffHour < 24 && sameDay(now, t)) return `${diffHour} 小时前`
  if (sameYear(now, t)) return `${t.getMonth() + 1}月${t.getDate()}日`
  return `${t.getFullYear()}年${t.getMonth() + 1}月${t.getDate()}日`
}

function sameDay(a: Date, b: Date) {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate()
}

function sameYear(a: Date, b: Date) {
  return a.getFullYear() === b.getFullYear()
}

function summary(n: Note): string {
  return markdownPlainText(n.content, 60) || '空白笔记'
}

// ---- 右键菜单 ----
const menu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })

function openMenu(e: MouseEvent, items: ContextMenuItem[]) {
  setTimeout(() => {
    menu.value = { visible: true, x: e.clientX, y: e.clientY, items }
  }, 0)
}

function onTreeContext(e: MouseEvent, row: TreeRow) {
  e.preventDefault()
  e.stopPropagation()
  if (row.kind === 'all') {
    openMenu(e, [
      { label: '新建文件夹', onClick: () => void promptCreateFolder(null) },
    ])
    return
  }
  if (row.kind === 'uncategorized') return
  const folder = row.folder!
  openMenu(e, [
    { label: '新建子文件夹', onClick: () => void promptCreateFolder(folder.id) },
    { label: '重命名', onClick: () => void promptRenameFolder(folder) },
    { label: '移动到…', onClick: () => openMoveDialog(folder) },
    {
      label: '删除',
      danger: true,
      dividerBefore: true,
      onClick: () => askDeleteFolder(folder),
    },
  ])
}

/** 笔记右键：移文件夹（不依赖 HTML5 拖拽；Tauri dragDropEnabled 下 DnD 常失效） */
function onNoteContext(e: MouseEvent, note: Note) {
  e.preventDefault()
  e.stopPropagation()
  const items: ContextMenuItem[] = [
    {
      label: '移到未分类',
      onClick: () => void moveNoteToFolder(note.id, null),
    },
    {
      label: '移动到文件夹…',
      dividerBefore: true,
      onClick: () => openNoteMoveDialog(note),
    },
  ]
  // ≤12 个文件夹时额外列出名称，一点即移；更多则走上方弹层
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

function flatFolderOptions(excludeId: number | null): AppSelectOption[] {
  const opts: AppSelectOption[] = []
  const walk = (parentId: number | null, depth: number) => {
    for (const c of childrenOf.value.get(parentId) ?? []) {
      if (excludeId != null && c.id === excludeId) {
        // 当前文件夹不可选，子级上提到本层继续可选
        walk(c.id, depth)
        continue
      }
      opts.push({
        value: String(c.id),
        label: `${'　'.repeat(depth)}${c.name}`,
      })
      walk(c.id, depth + 1)
    }
  }
  walk(null, 0)
  return opts
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

// ---- 移动笔记到文件夹（弹层）----
const noteMoveTarget = ref<Note | null>(null)
const noteMoveFolderValue = ref('uncategorized')

const noteMoveOptions = computed((): AppSelectOption[] => {
  if (!noteMoveTarget.value) return []
  return [
    { value: 'uncategorized', label: '未分类' },
    ...flatFolderOptions(null),
  ]
})

function openNoteMoveDialog(note: Note) {
  noteMoveTarget.value = note
  noteMoveFolderValue.value =
    note.folder_id == null ? 'uncategorized' : String(note.folder_id)
}

async function confirmNoteMove() {
  const note = noteMoveTarget.value
  if (!note) return
  const folderId =
    noteMoveFolderValue.value === 'uncategorized'
      ? null
      : Number(noteMoveFolderValue.value)
  noteMoveTarget.value = null
  await moveNoteToFolder(note.id, folderId)
}

async function promptCreateFolder(parentId: number | null) {
  const name = window.prompt(parentId == null ? '新建文件夹名称' : '新建子文件夹名称', '')
  if (name == null) return
  const trimmed = name.trim()
  if (!trimmed) {
    showToast('文件夹名称不能为空')
    return
  }
  try {
    await store.createFolder(parentId, trimmed)
    if (parentId != null) {
      const next = new Set(expanded.value)
      next.add(parentId)
      expanded.value = next
    }
  } catch (err) {
    showToast(String(err))
  }
}

async function promptRenameFolder(folder: NoteFolder) {
  const name = window.prompt('重命名文件夹', folder.name)
  if (name == null) return
  const trimmed = name.trim()
  if (!trimmed) {
    showToast('文件夹名称不能为空')
    return
  }
  try {
    await store.renameFolder(folder.id, trimmed)
  } catch (err) {
    showToast(String(err))
  }
}

// ---- 删除确认 ----
const deleteTarget = ref<NoteFolder | null>(null)

const deleteMessage = computed(() => {
  const f = deleteTarget.value
  if (!f) return ''
  return `将删除文件夹「${f.name}」。其下子文件夹会移到上一级；该文件夹内的 ${f.notes_count} 篇笔记将变为未分类。笔记不会被删除。`
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
    if (folderFilter.value === f.id) folderFilter.value = 'all'
  } catch (err) {
    showToast(String(err))
  }
}

// ---- 移动文件夹 ----
const moveTarget = ref<NoteFolder | null>(null)
const moveParentValue = ref('root')

function descendantIds(id: number): Set<number> {
  const out = new Set<number>()
  const walk = (pid: number) => {
    for (const c of childrenOf.value.get(pid) ?? []) {
      out.add(c.id)
      walk(c.id)
    }
  }
  walk(id)
  return out
}

const moveOptions = computed((): AppSelectOption[] => {
  const f = moveTarget.value
  if (!f) return []
  const blocked = descendantIds(f.id)
  blocked.add(f.id)
  const opts: AppSelectOption[] = [{ value: 'root', label: '根目录' }]
  const walk = (parentId: number | null, depth: number) => {
    for (const c of childrenOf.value.get(parentId) ?? []) {
      if (blocked.has(c.id)) continue
      opts.push({
        value: String(c.id),
        label: `${'　'.repeat(depth)}${c.name}`,
      })
      walk(c.id, depth + 1)
    }
  }
  walk(null, 0)
  return opts
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

</script>

<template>
  <section class="card note-list">
    <header class="nl-header">
      <h2 class="nl-title">速记</h2>
      <div class="nl-actions">
        <button class="icon-btn" type="button" title="导入 Markdown" @click="emit('import-request')">
          <Import :size="15" :stroke-width="2" />
        </button>
        <button class="icon-btn add" type="button" title="新建笔记" @click="emitCreate">
          <Plus :size="15" :stroke-width="2.2" />
        </button>
      </div>
    </header>

    <!-- 标签筛选（横向滚动） -->
    <nav v-if="store.state.tags.length > 0" class="filter-tabs tag-filter" aria-label="标签筛选">
      <button
        class="filter-tab filter-tab--tag"
        :class="{ active: activeTagId === null }"
        @click="activeTagId = null"
      >
        全部
      </button>
      <button
        v-for="t in store.state.tags"
        :key="t.id"
        class="filter-tab filter-tab--tag"
        :class="{ active: activeTagId === t.id }"
        @click="activeTagId = t.id"
      >
        {{ t.name }}
      </button>
    </nav>

    <div class="nl-split">
      <!-- 文件夹树 -->
      <aside class="nl-tree" aria-label="文件夹">
        <div class="nl-tree-head">
          <span class="nl-tree-label">文件夹</span>
          <button
            class="icon-btn nl-tree-add"
            type="button"
            title="新建文件夹"
            @click="promptCreateFolder(null)"
          >
            <FolderPlus :size="14" :stroke-width="2" />
          </button>
        </div>
        <div class="nl-tree-body">
          <div
            v-for="(row, idx) in treeRows"
            :key="`${row.kind}-${row.id ?? 'x'}-${idx}`"
            class="tree-row"
            :class="{ active: isSelected(row) }"
            :style="{ paddingLeft: `${8 + row.depth * 12}px` }"
            role="button"
            tabindex="0"
            @click="selectFolder(row)"
            @keydown.enter="selectFolder(row)"
            @contextmenu="onTreeContext($event, row)"
          >
            <button
              v-if="row.kind === 'folder' && row.hasChildren"
              class="tree-chevron"
              type="button"
              :aria-label="expanded.has(row.id!) ? '收起' : '展开'"
              @click="toggleExpand(row.id!, $event)"
            >
              <ChevronDown v-if="expanded.has(row.id!)" :size="12" :stroke-width="2.2" />
              <ChevronRight v-else :size="12" :stroke-width="2.2" />
            </button>
            <span v-else class="tree-chevron-spacer" />
            <FolderOpen
              v-if="row.kind === 'folder' && expanded.has(row.id!)"
              class="tree-icon"
              :size="13"
              :stroke-width="1.8"
            />
            <Folder
              v-else-if="row.kind === 'folder'"
              class="tree-icon"
              :size="13"
              :stroke-width="1.8"
            />
            <StickyNote
              v-else
              class="tree-icon"
              :size="13"
              :stroke-width="1.8"
            />
            <span class="tree-name" :title="row.name">{{ row.name }}</span>
            <span class="tree-badge">{{ row.count }}</span>
          </div>
        </div>
      </aside>

      <!-- 笔记列表 -->
      <div class="nl-list">
        <div v-if="sortedNotes.length > 0" class="nl-body">
          <div
            v-for="n in sortedNotes"
            :key="n.id"
            class="note-item"
            :class="{ active: n.id === activeId }"
            role="button"
            tabindex="0"
            @click="emit('select', n.id)"
            @keydown.enter="emit('select', n.id)"
            @keydown.space.prevent="emit('select', n.id)"
            @contextmenu="onNoteContext($event, n)"
          >
            <div class="note-item-main">
              <span class="note-title" :title="n.title">{{ n.title }}</span>
              <span class="note-meta">{{ formatTime(n.updated_at) }}</span>
              <span class="note-summary" :title="summary(n)">{{ summary(n) }}</span>
            </div>
            <button
              class="icon-btn del"
              type="button"
              title="删除笔记"
              aria-label="删除笔记"
              @click.stop="emit('delete', n.id)"
            >
              <X :size="13" :stroke-width="2" />
            </button>
          </div>
        </div>

        <div v-else class="empty-state">
          <StickyNote :size="24" :stroke-width="1.7" aria-hidden="true" />
          <p>{{ folderFilter === 'all' ? '还没有笔记' : '此文件夹暂无笔记' }}</p>
          <button class="pill-btn" type="button" style="margin-top: 6px" @click="emitCreate">
            新建笔记
          </button>
        </div>
      </div>
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
  </section>
</template>

<style scoped>
.note-list {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 16px 12px;
  min-height: 0;
  font-size: calc(1rem * var(--fs-notes, 1));
}
.nl-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 4px;
  margin-bottom: 10px;
}
.nl-title {
  font-size: 1em;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
}
.nl-actions {
  display: flex;
  align-items: center;
  gap: 4px;
}
.icon-btn.add {
  width: 30px;
  height: 30px;
  background: var(--brand-50);
  color: var(--brand-500);
}
.icon-btn.add:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
}

.nl-split {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

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

.nl-list {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.nl-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.tag-filter {
  padding: 0 4px 8px;
  margin-bottom: 2px;
}
.note-item {
  position: relative;
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: background 0.15s;
}
.note-item:hover {
  background: var(--bg-card-soft);
}
.note-item.active {
  background: var(--brand-50);
}
.note-item.active::before {
  content: '';
  position: absolute;
  left: 0;
  top: 10px;
  bottom: 10px;
  width: 3px;
  border-radius: 2px;
  background: var(--brand-500);
}
.note-item-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.note-title {
  font-size: 0.8125em;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.note-meta {
  font-size: 0.6875em;
  color: var(--text-3);
}
.note-summary {
  font-size: 0.75em;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.del {
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  opacity: 0;
  margin-top: -2px;
}
.note-item:hover .del,
.note-item:focus-within .del {
  opacity: 1;
}
.del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
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
