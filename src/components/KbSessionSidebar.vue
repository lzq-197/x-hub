<script setup lang="ts">
import { computed, inject, nextTick, ref } from 'vue'
import { Folder, FolderPlus, MessageSquarePlus, Pin, Plus } from 'lucide-vue-next'
import type { KbProject, KbSession } from '../api/tauri'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'

const props = defineProps<{
  sessions: readonly KbSession[]
  projects: readonly KbProject[]
  activeId: number | null
  disabled: boolean
}>()

const emit = defineEmits<{
  new: []
  select: [id: number]
  rename: [id: number, title: string]
  pin: [id: number, pinned: boolean]
  move: [id: number, projectId: number | null]
  delete: [id: number]
  'clear-all': []
  'create-project': [name: string]
  'rename-project': [id: number, name: string]
  'delete-project': [id: number]
}>()

const showToast = inject<(msg: string) => void>('showToast', () => {})

function bySortAsc(a: KbSession, b: KbSession) {
  if (a.sort_order !== b.sort_order) return a.sort_order - b.sort_order
  return b.updated_at.localeCompare(a.updated_at)
}

function byPinSortAsc(a: KbSession, b: KbSession) {
  if (a.pin_sort_order !== b.pin_sort_order) return a.pin_sort_order - b.pin_sort_order
  return b.updated_at.localeCompare(a.updated_at)
}

const pinned = computed(() =>
  props.sessions.filter((s) => s.pinned).slice().sort(byPinSortAsc),
)

const recent = computed(() =>
  props.sessions
    .filter((s) => !s.pinned && s.project_id == null)
    .slice()
    .sort(bySortAsc),
)

const projectBlocks = computed(() =>
  props.projects.map((p) => ({
    project: p,
    sessions: props.sessions.filter((s) => s.project_id === p.id).slice().sort(bySortAsc),
  })),
)

/** 折叠状态：默认全部展开；本版不跨重启持久化 */
const collapsed = ref<Set<number>>(new Set())

function isExpanded(id: number) {
  return !collapsed.value.has(id)
}

function toggleProject(id: number) {
  const next = new Set(collapsed.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  collapsed.value = next
}

function onSelect(id: number) {
  if (props.disabled) {
    showToast('生成中，请稍候')
    return
  }
  if (id === props.activeId) return
  emit('select', id)
}

function onNew() {
  if (props.disabled) {
    showToast('生成中，请稍候')
    return
  }
  emit('new')
}

// ---- 行内重命名 / 新建项目 ----
type EditState =
  | { kind: 'session'; id: number }
  | { kind: 'project'; id: number }
  | { kind: 'create-project' }

const editing = ref<EditState | null>(null)
const editText = ref('')
const editInput = ref<HTMLInputElement | null>(null)

function bindEditInput(el: unknown) {
  editInput.value = el instanceof HTMLInputElement ? el : null
}

async function focusEdit() {
  await nextTick()
  editInput.value?.focus()
  editInput.value?.select()
}

function startRenameSession(s: KbSession) {
  editing.value = { kind: 'session', id: s.id }
  editText.value = s.title
  void focusEdit()
}

function startRenameProject(p: KbProject) {
  editing.value = { kind: 'project', id: p.id }
  editText.value = p.name
  void focusEdit()
}

function startCreateProject() {
  if (props.disabled) return
  editing.value = { kind: 'create-project' }
  editText.value = ''
  void focusEdit()
}

function cancelEdit() {
  editing.value = null
  editText.value = ''
}

function commitEdit() {
  const state = editing.value
  if (!state) return
  const name = editText.value.trim()
  if (state.kind === 'create-project') {
    if (!name) {
      cancelEdit()
      return
    }
    emit('create-project', name)
    cancelEdit()
    return
  }
  if (state.kind === 'session') {
    if (!name) {
      cancelEdit()
      return
    }
    const cur = props.sessions.find((s) => s.id === state.id)
    if (cur && cur.title === name) {
      cancelEdit()
      return
    }
    emit('rename', state.id, name)
    cancelEdit()
    return
  }
  if (!name) {
    cancelEdit()
    return
  }
  const cur = props.projects.find((p) => p.id === state.id)
  if (cur && cur.name === name) {
    cancelEdit()
    return
  }
  emit('rename-project', state.id, name)
  cancelEdit()
}

/** 置顶会话也出现在项目下时，只在置顶区展示编辑框，避免双输入 */
function showSessionEdit(s: KbSession, zone: 'pinned' | 'project' | 'recent') {
  if (!(editing.value?.kind === 'session' && editing.value.id === s.id)) return false
  if (zone === 'project' && s.pinned) return false
  return true
}

function showProjectEdit(id: number) {
  return editing.value?.kind === 'project' && editing.value.id === id
}

// ---- Context menu ----
const menu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })

function openMenu(e: MouseEvent, items: ContextMenuItem[]) {
  setTimeout(() => {
    menu.value = { visible: true, x: e.clientX, y: e.clientY, items }
  }, 0)
}

function sessionMenuItems(s: KbSession): ContextMenuItem[] {
  const items: ContextMenuItem[] = [
    { label: '重命名', onClick: () => startRenameSession(s) },
    {
      label: s.pinned ? '取消置顶' : '置顶',
      onClick: () => emit('pin', s.id, !s.pinned),
    },
  ]
  if (s.project_id != null) {
    items.push({
      label: '移出项目',
      dividerBefore: true,
      onClick: () => emit('move', s.id, null),
    })
  }
  const others = props.projects.filter((p) => p.id !== s.project_id)
  for (let i = 0; i < others.length; i++) {
    const p = others[i]!
    items.push({
      label: `移到「${p.name}」`,
      dividerBefore: i === 0 && s.project_id == null,
      onClick: () => emit('move', s.id, p.id),
    })
  }
  items.push({
    label: '删除',
    danger: true,
    dividerBefore: true,
    onClick: () => emit('delete', s.id),
  })
  return items
}

function onSessionContext(e: MouseEvent, s: KbSession) {
  e.preventDefault()
  e.stopPropagation()
  if (props.disabled) return
  openMenu(e, sessionMenuItems(s))
}

function onProjectContext(e: MouseEvent, p: KbProject) {
  e.preventDefault()
  e.stopPropagation()
  if (props.disabled) return
  openMenu(e, [
    { label: '重命名', onClick: () => startRenameProject(p) },
    {
      label: '删除项目',
      danger: true,
      dividerBefore: true,
      onClick: () => emit('delete-project', p.id),
    },
  ])
}

function onMoreClick(e: MouseEvent, s: KbSession) {
  e.preventDefault()
  e.stopPropagation()
  if (props.disabled) return
  openMenu(e, sessionMenuItems(s))
}
</script>

<template>
  <aside class="kb-side" aria-label="知识库会话">
    <div class="kb-side-top">
      <button type="button" class="kb-new" :disabled="disabled" @click="onNew">
        <MessageSquarePlus :size="15" :stroke-width="2" aria-hidden="true" />
        新对话
      </button>
    </div>

    <div class="kb-side-body">
      <!-- 置顶（区头常驻，供拖放命中；空列表时 body 为空） -->
      <section class="kb-sec">
        <div class="kb-sec-head">
          <Pin :size="11" :stroke-width="2" aria-hidden="true" />
          <span>置顶</span>
        </div>
        <div
          v-for="s in pinned"
          :key="'pin-' + s.id"
          class="kb-sess"
          :class="{ on: s.id === activeId, disabled }"
          @contextmenu="onSessionContext($event, s)"
        >
          <input
            v-if="showSessionEdit(s, 'pinned')"
            :ref="bindEditInput"
            v-model="editText"
            class="kb-edit"
            type="text"
            @click.stop
            @keydown.enter.prevent="commitEdit"
            @keydown.escape.prevent="cancelEdit"
            @blur="commitEdit"
          />
          <template v-else>
            <button
              type="button"
              class="kb-sess-main"
              :disabled="disabled"
              :title="s.title"
              @click="onSelect(s.id)"
            >
              <span class="kb-sess-title">{{ s.title }}</span>
            </button>
            <button
              type="button"
              class="kb-more"
              title="更多"
              :disabled="disabled"
              @click="onMoreClick($event, s)"
            >
              ···
            </button>
          </template>
        </div>
      </section>

      <!-- 项目 -->
      <section class="kb-sec">
        <div class="kb-sec-head">
          <Folder :size="11" :stroke-width="2" aria-hidden="true" />
          <span>项目</span>
          <button
            type="button"
            class="kb-sec-add"
            title="新建项目"
            :disabled="disabled"
            @click="startCreateProject"
          >
            <FolderPlus :size="13" :stroke-width="2" aria-hidden="true" />
          </button>
        </div>

        <div v-if="editing?.kind === 'create-project'" class="kb-create-row">
          <input
            :ref="bindEditInput"
            v-model="editText"
            class="kb-edit"
            type="text"
            placeholder="项目名称"
            @keydown.enter.prevent="commitEdit"
            @keydown.escape.prevent="cancelEdit"
            @blur="commitEdit"
          />
        </div>

        <div v-for="block in projectBlocks" :key="block.project.id" class="kb-proj">
          <div v-if="showProjectEdit(block.project.id)" class="kb-proj-row">
            <input
              :ref="bindEditInput"
              v-model="editText"
              class="kb-edit"
              type="text"
              @keydown.enter.prevent="commitEdit"
              @keydown.escape.prevent="cancelEdit"
              @blur="commitEdit"
            />
          </div>
          <button
            v-else
            type="button"
            class="kb-proj-row"
            :disabled="disabled"
            @click="toggleProject(block.project.id)"
            @contextmenu="onProjectContext($event, block.project)"
          >
            <span class="kb-chevron" :data-open="isExpanded(block.project.id) ? '1' : '0'">▸</span>
            <Folder :size="13" :stroke-width="1.8" aria-hidden="true" />
            <span class="kb-proj-name" :title="block.project.name">{{ block.project.name }}</span>
            <span class="kb-proj-count">{{ block.sessions.length }}</span>
          </button>
          <template v-if="isExpanded(block.project.id)">
            <div
              v-for="s in block.sessions"
              :key="'p' + block.project.id + '-' + s.id"
              class="kb-sess kb-sess--nested"
              :class="{ on: s.id === activeId, disabled }"
              @contextmenu="onSessionContext($event, s)"
            >
              <input
                v-if="showSessionEdit(s, 'project')"
                :ref="bindEditInput"
                v-model="editText"
                class="kb-edit"
                type="text"
                @click.stop
                @keydown.enter.prevent="commitEdit"
                @keydown.escape.prevent="cancelEdit"
                @blur="commitEdit"
              />
              <template v-else>
                <button
                  type="button"
                  class="kb-sess-main"
                  :disabled="disabled"
                  :title="s.title"
                  @click="onSelect(s.id)"
                >
                  <span class="kb-sess-title">
                    <Pin
                      v-if="s.pinned"
                      class="kb-sess-pin"
                      :size="10"
                      :stroke-width="2.2"
                      aria-hidden="true"
                    />
                    {{ s.title }}
                  </span>
                </button>
                <button
                  type="button"
                  class="kb-more"
                  title="更多"
                  :disabled="disabled"
                  @click="onMoreClick($event, s)"
                >
                  ···
                </button>
              </template>
            </div>
            <div
              v-if="block.sessions.length === 0 && isExpanded(block.project.id)"
              class="kb-drop-strip"
              data-kb-drop="project-empty"
              :data-project-id="block.project.id"
              aria-hidden="true"
            />
          </template>
        </div>

        <p v-if="!projectBlocks.length && editing?.kind !== 'create-project'" class="kb-empty-hint">
          暂无项目
          <button type="button" class="kb-link" :disabled="disabled" @click="startCreateProject">
            <Plus :size="11" :stroke-width="2.2" aria-hidden="true" />
            新建
          </button>
        </p>
      </section>

      <!-- 最近 -->
      <section class="kb-sec">
        <div class="kb-sec-head">
          <span>最近</span>
        </div>
        <div
          v-for="s in recent"
          :key="'recent-' + s.id"
          class="kb-sess"
          :class="{ on: s.id === activeId, disabled }"
          @contextmenu="onSessionContext($event, s)"
        >
          <input
            v-if="showSessionEdit(s, 'recent')"
            :ref="bindEditInput"
            v-model="editText"
            class="kb-edit"
            type="text"
            @click.stop
            @keydown.enter.prevent="commitEdit"
            @keydown.escape.prevent="cancelEdit"
            @blur="commitEdit"
          />
          <template v-else>
            <button
              type="button"
              class="kb-sess-main"
              :disabled="disabled"
              :title="s.title"
              @click="onSelect(s.id)"
            >
              <span class="kb-sess-title">{{ s.title }}</span>
            </button>
            <button
              type="button"
              class="kb-more"
              title="更多"
              :disabled="disabled"
              @click="onMoreClick($event, s)"
            >
              ···
            </button>
          </template>
        </div>
        <p v-if="!recent.length" class="kb-empty-hint">暂无最近会话</p>
      </section>
    </div>

    <div class="kb-side-foot">
      <button
        type="button"
        class="kb-clear"
        :disabled="disabled || sessions.length === 0"
        @click="emit('clear-all')"
      >
        清空全部
      </button>
    </div>

    <ContextMenu
      :visible="menu.visible"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      @close="menu.visible = false"
    />
  </aside>
</template>

<style scoped>
.kb-side {
  width: 240px;
  min-width: 240px;
  max-width: 240px;
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-right: 1px solid var(--border-soft);
  background: transparent;
}
.kb-side-top {
  flex-shrink: 0;
  padding: 10px 10px 6px;
}
.kb-new {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  width: 100%;
  height: 32px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  color: var(--text-1);
  font-size: 0.8125rem;
  font-weight: 600;
  font-family: inherit;
  cursor: pointer;
  transition: background 0.12s, border-color 0.12s, color 0.12s, transform 0.1s;
}
.kb-new:hover:not(:disabled) {
  background: var(--brand-50);
  border-color: color-mix(in srgb, var(--brand-500) 35%, var(--border-soft));
  color: var(--brand-500);
}
.kb-new:active:not(:disabled) {
  transform: scale(0.96);
}
.kb-new:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.kb-side-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 6px 8px;
}

.kb-sec {
  margin-bottom: 10px;
}
.kb-sec-head {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px 2px;
  font-size: 0.6875rem;
  font-weight: 600;
  color: var(--text-3);
  letter-spacing: 0.02em;
  user-select: none;
}
.kb-sec-add {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  padding: 0;
}
.kb-sec-add:hover:not(:disabled) {
  background: var(--brand-50);
  color: var(--brand-500);
}
.kb-sec-add:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.kb-sess {
  display: flex;
  align-items: center;
  gap: 4px;
  width: 100%;
  padding: 2px 4px 2px 4px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-2);
  font-size: 0.8125rem;
  font-family: inherit;
  text-align: left;
  box-sizing: border-box;
}
.kb-sess:hover:not(.disabled) {
  background: color-mix(in srgb, var(--brand-50) 70%, transparent);
  color: var(--text-1);
}
.kb-sess.on {
  background: var(--bg-card-soft);
  color: var(--brand-500);
  font-weight: 600;
}
.kb-sess.disabled {
  opacity: 0.55;
}
.kb-sess--nested {
  padding-left: 24px;
}
.kb-sess-main {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 4px;
  margin: 0;
  padding: 4px 4px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: inherit;
  font: inherit;
  font-weight: inherit;
  text-align: left;
  cursor: pointer;
}
.kb-sess-main:disabled {
  cursor: not-allowed;
}
.kb-sess-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.kb-sess-pin {
  flex-shrink: 0;
  color: var(--brand-500);
  opacity: 0.85;
}
.kb-more {
  flex-shrink: 0;
  opacity: 0;
  border: none;
  background: transparent;
  color: var(--text-3);
  font-size: 0.75rem;
  line-height: 1;
  padding: 2px 4px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-family: inherit;
}
.kb-sess:hover .kb-more,
.kb-sess.on .kb-more {
  opacity: 1;
}
.kb-more:hover:not(:disabled) {
  background: var(--bg-card);
  color: var(--text-1);
}

.kb-proj {
  margin-bottom: 2px;
}
.kb-proj-row {
  display: flex;
  align-items: center;
  gap: 4px;
  width: 100%;
  padding: 5px 8px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-2);
  font-size: 0.8125rem;
  font-family: inherit;
  text-align: left;
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
  box-sizing: border-box;
}
.kb-proj-row:hover:not(:disabled) {
  background: color-mix(in srgb, var(--brand-50) 70%, transparent);
  color: var(--text-1);
}
.kb-chevron {
  width: 12px;
  flex-shrink: 0;
  font-size: 0.625rem;
  color: var(--text-3);
  transition: transform 0.12s;
  display: inline-flex;
  justify-content: center;
}
.kb-chevron[data-open='1'] {
  transform: rotate(90deg);
}
.kb-proj-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
}
.kb-proj-count {
  flex-shrink: 0;
  font-size: 0.6875rem;
  color: var(--text-4);
}

.kb-create-row {
  padding: 2px 8px 4px;
}
.kb-edit {
  flex: 1;
  min-width: 0;
  width: 100%;
  height: 26px;
  padding: 0 8px;
  border: 1px solid color-mix(in srgb, var(--brand-500) 40%, var(--border-soft));
  border-radius: var(--radius-sm);
  background: var(--bg-card);
  color: var(--text-1);
  font-size: 0.8125rem;
  font-family: inherit;
  outline: none;
  box-sizing: border-box;
}
.kb-edit:focus {
  border-color: var(--brand-500);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--brand-500) 18%, transparent);
}

.kb-empty-hint {
  margin: 2px 8px 4px;
  font-size: 0.6875rem;
  color: var(--text-4);
  display: flex;
  align-items: center;
  gap: 6px;
}
.kb-drop-strip {
  height: 10px;
  margin: 2px 8px 6px;
  border-radius: 6px;
  border: 1px dashed transparent;
}
.kb-drop-strip.is-hot {
  border-color: color-mix(in srgb, var(--brand-500) 55%, transparent);
  background: color-mix(in srgb, var(--brand-500) 12%, transparent);
}
.kb-link {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  border: none;
  background: transparent;
  color: var(--brand-500);
  font-size: inherit;
  font-family: inherit;
  cursor: pointer;
  padding: 0;
}
.kb-link:hover:not(:disabled) {
  text-decoration: underline;
}
.kb-link:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.kb-side-foot {
  flex-shrink: 0;
  padding: 6px 10px 10px;
  border-top: 1px solid var(--border-soft);
}
.kb-clear {
  width: 100%;
  height: 28px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  font-size: 0.75rem;
  font-family: inherit;
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}
.kb-clear:hover:not(:disabled) {
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
  color: var(--c-red);
}
.kb-clear:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
</style>
