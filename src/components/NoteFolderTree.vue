<script setup lang="ts">
import { computed, inject, ref, watch } from 'vue'
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  FolderPlus,
  StickyNote,
} from 'lucide-vue-next'
import type { Note, NoteFolder } from '../api/tauri'
import { useStore } from '../stores/workbench'
import {
  useFolderChildren,
  type FolderFilter,
} from '../composables/useNoteFolders'
import AppSelect, { type AppSelectOption } from './AppSelect.vue'
import ConfirmDialog from './ConfirmDialog.vue'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'

const props = defineProps<{
  notes: readonly Note[]
  modelValue: FolderFilter
  /** 打开笔记时展开其父链（不改 modelValue） */
  expandForFolderId?: number | null
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', v: FolderFilter): void
}>()

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})

const { childrenOf, flatFolderOptions, descendantIds, ancestorIds } = useFolderChildren(
  computed(() => store.state.folders),
)

const expanded = ref<Set<number>>(new Set())

watch(
  () => store.state.folders.map((f) => f.id).join(','),
  () => {
    const next = new Set(expanded.value)
    for (const f of store.state.folders) next.add(f.id)
    expanded.value = next
  },
  { immediate: true },
)

watch(
  () => props.expandForFolderId,
  (fid) => {
    if (fid == null) return
    const next = new Set(expanded.value)
    for (const id of ancestorIds(fid)) next.add(id)
    expanded.value = next
  },
  { immediate: true },
)

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
  return props.modelValue === filterKey(row)
}

function selectFolder(row: TreeRow) {
  emit('update:modelValue', filterKey(row))
}

function toggleExpand(id: number, e: Event) {
  e.stopPropagation()
  const next = new Set(expanded.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  expanded.value = next
}

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
    openMenu(e, [{ label: '新建文件夹', onClick: () => void promptCreateFolder(null) }])
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
    if (props.modelValue === f.id) emit('update:modelValue', 'all')
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
        <StickyNote v-else class="tree-icon" :size="13" :stroke-width="1.8" />
        <span class="tree-name" :title="row.name">{{ row.name }}</span>
        <span class="tree-badge">{{ row.count }}</span>
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
