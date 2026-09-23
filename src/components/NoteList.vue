<script setup lang="ts">
import { computed, inject, onMounted, ref } from 'vue'
import { Import, Plus } from 'lucide-vue-next'
import type { Note } from '../api/tauri'
import { useStore } from '../stores/workbench'
import NoteFolderTree from './NoteFolderTree.vue'

const props = defineProps<{
  notes: readonly Note[]
  activeId: number | null
}>()

const emit = defineEmits<{
  (e: 'select', id: number): void
  /** folderId：当前树选中的真实文件夹，或按名称解析/创建后的根级夹；null = 顶级 */
  (e: 'create', folderId: number | null): void
  (e: 'delete', id: number): void
  (e: 'import-request'): void
}>()

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})

const activeTagId = ref<number | null>(null)
const tagMap = ref<Map<number, number[]>>(new Map())
/** 树选中的真实文件夹；null = 未选夹（新建默认顶级，可填文件夹名） */
const selectedFolderId = ref<number | null>(null)

const showCreateBar = ref(false)
const folderNameInput = ref('')
const createBusy = ref(false)

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

/** 标签筛选后交给树；无标签筛选则原样 */
const treeNotes = computed((): readonly Note[] => {
  if (activeTagId.value === null) return props.notes
  const tid = activeTagId.value
  return props.notes.filter((n) => tagMap.value.get(n.id)?.includes(tid))
})

function onSelectFolder(id: number | null) {
  selectedFolderId.value = id
  if (id != null) {
    showCreateBar.value = false
    folderNameInput.value = ''
  }
}

function onCreateClick() {
  if (selectedFolderId.value != null) {
    emit('create', selectedFolderId.value)
    return
  }
  showCreateBar.value = true
}

function cancelCreateBar() {
  showCreateBar.value = false
  folderNameInput.value = ''
}

async function confirmCreate() {
  if (createBusy.value) return
  const name = folderNameInput.value.trim()
  if (!name) {
    emit('create', null)
    cancelCreateBar()
    return
  }
  createBusy.value = true
  try {
    // 避免启动快照过期：按名称解析前先刷新文件夹列表
    await store.refreshFolders()
    const id = await store.resolveOrCreateRootFolder(name)
    emit('create', id)
    cancelCreateBar()
  } catch (err) {
    showToast(String(err))
  } finally {
    createBusy.value = false
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
        <button class="icon-btn add" type="button" title="新建笔记" @click="onCreateClick">
          <Plus :size="15" :stroke-width="2.2" />
        </button>
      </div>
    </header>

    <div v-if="showCreateBar" class="nl-create-bar">
      <label class="nl-create-label" for="nl-folder-name">文件夹</label>
      <input
        id="nl-folder-name"
        v-model="folderNameInput"
        type="text"
        class="nl-create-input"
        placeholder="留空=顶级"
        :disabled="createBusy"
        @keydown.enter.prevent="confirmCreate"
        @keydown.escape.prevent="cancelCreateBar"
      />
      <button
        type="button"
        class="nl-create-confirm"
        :disabled="createBusy"
        @click="confirmCreate"
      >
        确认
      </button>
      <button type="button" class="nl-create-cancel" :disabled="createBusy" @click="cancelCreateBar">
        取消
      </button>
    </div>

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

    <NoteFolderTree
      class="nl-tree-fill"
      :notes="treeNotes"
      :active-note-id="activeId"
      :selected-folder-id="selectedFolderId"
      @select-note="emit('select', $event)"
      @select-folder="onSelectFolder"
    />
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

.nl-create-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 4px 10px;
  flex-shrink: 0;
}
.nl-create-label {
  font-size: 0.75em;
  color: var(--text-3);
  flex-shrink: 0;
}
.nl-create-input {
  flex: 1;
  min-width: 0;
  height: 28px;
  padding: 0 8px;
  font-size: 0.8125em;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  color: var(--text-1);
  outline: none;
}
.nl-create-input:focus {
  border-color: var(--brand-500);
}
.nl-create-confirm,
.nl-create-cancel {
  height: 28px;
  padding: 0 10px;
  font-size: 0.75em;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-soft);
  background: var(--bg-card-soft);
  color: var(--text-2);
  cursor: pointer;
  flex-shrink: 0;
}
.nl-create-confirm {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: var(--text-on-accent);
  font-weight: 600;
}
.nl-create-confirm:hover:not(:disabled) {
  filter: brightness(1.06);
}
.nl-create-cancel:hover:not(:disabled) {
  color: var(--text-1);
  border-color: var(--border-strong);
}
.nl-create-confirm:disabled,
.nl-create-cancel:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.tag-filter {
  padding: 0 4px 8px;
  margin-bottom: 2px;
  flex-shrink: 0;
}

.nl-tree-fill {
  flex: 1;
  min-height: 0;
}
</style>
