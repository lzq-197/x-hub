<script setup lang="ts">
import { computed, inject, onMounted, ref, watch } from 'vue'
import { Import, Plus, StickyNote, X } from 'lucide-vue-next'
import type { Note } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { markdownPlainText } from '../utils/markdown'
import { parseTimestamp } from '../utils/time'
import { useFolderChildren, type FolderFilter } from '../composables/useNoteFolders'
import AppSelect, { type AppSelectOption } from './AppSelect.vue'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import NoteFolderTree from './NoteFolderTree.vue'

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

const folderFilter = ref<FolderFilter>('all')
const { flatFolderOptions } = useFolderChildren(computed(() => store.state.folders))

const expandForFolderId = computed(() => {
  if (props.activeId == null) return null
  const note = props.notes.find((n) => n.id === props.activeId)
  return note?.folder_id ?? null
})

function emitCreate() {
  const folderId = typeof folderFilter.value === 'number' ? folderFilter.value : null
  emit('create', folderId)
}

/** 打开笔记时：非「全部」才同步筛选；展开父链由 NoteFolderTree 负责 */
watch(
  () => props.activeId,
  (id) => {
    if (id == null) return
    const note = props.notes.find((n) => n.id === id)
    if (!note) return
    if (folderFilter.value === 'all') return
    folderFilter.value = note.folder_id == null ? 'uncategorized' : note.folder_id
  },
  { immediate: true },
)

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

const menu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })

function openMenu(e: MouseEvent, items: ContextMenuItem[]) {
  setTimeout(() => {
    menu.value = { visible: true, x: e.clientX, y: e.clientY, items }
  }, 0)
}

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
const noteMoveFolderValue = ref('uncategorized')

const noteMoveOptions = computed((): AppSelectOption[] => {
  if (!noteMoveTarget.value) return []
  return [{ value: 'uncategorized', label: '未分类' }, ...flatFolderOptions(null)]
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
      <NoteFolderTree
        v-model="folderFilter"
        :notes="notes"
        :expand-for-folder-id="expandForFolderId"
      />

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
