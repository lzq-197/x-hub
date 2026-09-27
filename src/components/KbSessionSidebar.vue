<script setup lang="ts">
import { computed, inject, nextTick, onUnmounted, ref } from 'vue'
import { Folder, FolderPlus, MessageSquarePlus, Pin, Plus } from 'lucide-vue-next'
import type {
  KbDragSourceZone,
  KbPlaceTarget,
  KbProject,
  KbSession,
} from '../api/tauri'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import { sameIdOrder } from '../utils/noteTreeHit'

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
  place: [id: number, sourceZone: KbDragSourceZone, target: KbPlaceTarget]
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

/** 拖完松手后吞掉随后的 click，避免误选会话 */
let suppressSelect = false

function onSelect(id: number) {
  if (suppressSelect) return
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

// ---- Pointer DnD（禁止 HTML5 DnD；命中见设计 §5）----
type DragState = {
  id: number
  sourceZone: KbDragSourceZone
  /** 源在项目区时的 project_id；置顶/最近为 null */
  sourceProjectId: number | null
  sourceEl: HTMLElement
  startY: number
  startX: number
  active: boolean
}

type HotState = {
  kind: 'container' | 'gap'
  key: string
  beforeId: number | null
  place: KbPlaceTarget
  edgeY: number | null
}

const sideBodyRef = ref<HTMLElement | null>(null)
const drag = ref<DragState | null>(null)
const hot = ref<HotState | null>(null)
const lineTop = ref<number | null>(null)

let hoverExpandTimer: ReturnType<typeof setTimeout> | null = null
let hoverExpandTarget: number | null = null
let dragCancelBound = false

function clearHoverExpand() {
  if (hoverExpandTimer != null) {
    clearTimeout(hoverExpandTimer)
    hoverExpandTimer = null
  }
  hoverExpandTarget = null
}

function scheduleHoverExpand(projectId: number) {
  if (isExpanded(projectId)) return
  if (hoverExpandTarget === projectId) return
  clearHoverExpand()
  hoverExpandTarget = projectId
  hoverExpandTimer = setTimeout(() => {
    hoverExpandTimer = null
    if (!isExpanded(projectId)) {
      const next = new Set(collapsed.value)
      next.delete(projectId)
      collapsed.value = next
    }
  }, 400)
}

function clearDragChrome() {
  clearHoverExpand()
  document.body.classList.remove('kb-session-dragging')
  drag.value = null
  hot.value = null
  lineTop.value = null
  if (dragCancelBound) {
    window.removeEventListener('keydown', onDragKeydown)
    window.removeEventListener('blur', onDragWindowBlur)
    dragCancelBound = false
  }
}

function cancelDrag() {
  if (!drag.value) return
  clearDragChrome()
}

function onDragKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    cancelDrag()
  }
}

function onDragWindowBlur() {
  cancelDrag()
}

function bindDragCancel() {
  if (dragCancelBound) return
  dragCancelBound = true
  window.addEventListener('keydown', onDragKeydown)
  window.addEventListener('blur', onDragWindowBlur)
}

function sessionsInZone(zone: KbDragSourceZone, projectId: number | null): KbSession[] {
  if (zone === 'pinned') return pinned.value
  if (zone === 'recent') return recent.value
  if (projectId == null) return []
  return props.sessions.filter((s) => s.project_id === projectId).slice().sort(bySortAsc)
}

function isHotContainer(key: string) {
  return hot.value?.kind === 'container' && hot.value.key === key
}

/** 指针仍在源行矩形内 → 取消落点（微拖不误 append） */
function pointerInSourceRow(clientX: number, clientY: number): boolean {
  const live = drag.value
  if (!live?.sourceEl) return false
  const r = live.sourceEl.getBoundingClientRect()
  return clientX >= r.left && clientX <= r.right && clientY >= r.top && clientY <= r.bottom
}

/**
 * 从 elementsFromPoint 栈里找第一个可用 data-kb-drop：
 * 跳过 .is-dragging / 源会话行（pointer-events:none 时栈会穿透到区头 section）。
 */
function findDropEl(clientX: number, clientY: number, body: HTMLElement): HTMLElement | null {
  const live = drag.value
  const stack = document.elementsFromPoint(clientX, clientY)
  for (const node of stack) {
    if (!(node instanceof Element) || !body.contains(node)) continue
    if (node.closest('.is-dragging')) continue
    const drop = node.closest('[data-kb-drop]') as HTMLElement | null
    if (!drop || !body.contains(drop)) continue
    if (
      drop.dataset.kbDrop === 'session' &&
      live &&
      Number(drop.dataset.sessionId) === live.id
    ) {
      continue
    }
    return drop
  }
  return null
}

function hitFromDropEl(drop: HTMLElement, clientY: number): HotState | null {
  const kind = drop.dataset.kbDrop
  if (kind === 'pinned-head') {
    return {
      kind: 'container',
      key: 'pinned',
      beforeId: null,
      place: { zone: 'pinned', beforeId: null },
      edgeY: null,
    }
  }
  if (kind === 'recent-head') {
    return {
      kind: 'container',
      key: 'recent',
      beforeId: null,
      place: { zone: 'recent', beforeId: null },
      edgeY: null,
    }
  }
  if (kind === 'project-row' || kind === 'project-empty') {
    const projectId = Number(drop.dataset.projectId)
    if (!Number.isFinite(projectId)) return null
    return {
      kind: 'container',
      key: `project-${projectId}`,
      beforeId: null,
      place: { zone: 'project', projectId, beforeId: null },
      edgeY: null,
    }
  }
  if (kind === 'session') {
    const id = Number(drop.dataset.sessionId)
    const zone = drop.dataset.kbZone as KbDragSourceZone | undefined
    if (!Number.isFinite(id) || !zone) return null
    const projectId =
      zone === 'project' ? Number(drop.dataset.projectId) : null
    if (zone === 'project' && !Number.isFinite(projectId)) return null

    const rect = drop.getBoundingClientRect()
    const upper = clientY < rect.top + rect.height / 2
    const list = sessionsInZone(zone, projectId)
    const idx = list.findIndex((s) => s.id === id)

    if (upper) {
      const place: KbPlaceTarget =
        zone === 'project'
          ? { zone: 'project', projectId: projectId!, beforeId: id }
          : { zone, beforeId: id }
      return {
        kind: 'gap',
        key: `${zone}-before-${id}`,
        beforeId: id,
        place,
        edgeY: rect.top,
      }
    }

    const nextId = idx >= 0 && idx < list.length - 1 ? list[idx + 1]!.id : null
    const place: KbPlaceTarget =
      zone === 'project'
        ? { zone: 'project', projectId: projectId!, beforeId: nextId }
        : { zone, beforeId: nextId }
    return {
      kind: 'gap',
      key: `${zone}-after-${id}`,
      beforeId: nextId,
      place,
      edgeY: rect.bottom,
    }
  }
  return null
}

function resolveHit(clientX: number, clientY: number): HotState | null {
  const body = sideBodyRef.value
  if (!body) return null
  if (pointerInSourceRow(clientX, clientY)) return null
  const drop = findDropEl(clientX, clientY, body)
  if (!drop) return null
  return hitFromDropEl(drop, clientY)
}

/** 落点与当前区归属 + 顺序一致 → 不调 API（对齐 NoteFolderTree sameIdOrder） */
function isPlaceNoop(live: DragState, place: KbPlaceTarget): boolean {
  if (place.zone === 'pinned') {
    if (live.sourceZone !== 'pinned') return false
  } else if (place.zone === 'recent') {
    if (live.sourceZone !== 'recent') return false
  } else {
    if (live.sourceZone !== 'project') return false
    if (live.sourceProjectId !== place.projectId) return false
  }

  const zone: KbDragSourceZone = place.zone
  const projectId = place.zone === 'project' ? (place.projectId ?? null) : null
  const ids = sessionsInZone(zone, projectId).map((s) => s.id)
  const without = ids.filter((id) => id !== live.id)
  let insertAt = without.length
  if (place.beforeId != null) {
    const bi = without.indexOf(place.beforeId)
    if (bi >= 0) insertAt = bi
  }
  const next = [...without.slice(0, insertAt), live.id, ...without.slice(insertAt)]
  return sameIdOrder(ids, next)
}

function updateDragAim(clientX: number, clientY: number) {
  const live = drag.value
  const body = sideBodyRef.value
  if (!live?.active || !body) return

  const hit = resolveHit(clientX, clientY)
  hot.value = hit

  if (!hit) {
    lineTop.value = null
    clearHoverExpand()
    return
  }

  if (hit.kind === 'container' && hit.place.zone === 'project' && hit.place.projectId != null) {
    scheduleHoverExpand(hit.place.projectId)
  } else {
    clearHoverExpand()
  }

  if (hit.kind === 'gap' && hit.edgeY != null) {
    const bodyRect = body.getBoundingClientRect()
    lineTop.value = hit.edgeY - bodyRect.top + body.scrollTop
  } else {
    lineTop.value = null
  }
}

function finishDrag(clientX?: number, clientY?: number) {
  const live = drag.value
  const aim = hot.value
  suppressSelect = true
  setTimeout(() => {
    suppressSelect = false
  }, 0)
  const inSource =
    live != null &&
    clientX != null &&
    clientY != null &&
    pointerInSourceRow(clientX, clientY)
  clearDragChrome()
  if (!live || !aim || inSource) return
  if (isPlaceNoop(live, aim.place)) return
  emit('place', live.id, live.sourceZone, aim.place)
}

function onSessionPointerDown(s: KbSession, zone: KbDragSourceZone, e: PointerEvent) {
  if (e.button !== 0 || props.disabled || editing.value) return
  const target = e.target as HTMLElement | null
  if (target?.closest('.kb-more, input, .kb-edit, [data-no-drag]')) return

  const el = e.currentTarget as HTMLElement
  const startX = e.clientX
  const startY = e.clientY
  let active = false
  let dead = false

  drag.value = {
    id: s.id,
    sourceZone: zone,
    sourceProjectId: zone === 'project' ? s.project_id : null,
    sourceEl: el,
    startX,
    startY,
    active: false,
  }

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
    if (!drag.value) return false
    drag.value.active = true
    document.body.classList.add('kb-session-dragging')
    document.getSelection()?.removeAllRanges()
    bindDragCancel()
    return true
  }

  function onMove(ev: PointerEvent) {
    if (dead) return
    if (!active) {
      if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < 4) return
      if (!begin()) {
        dead = true
        return
      }
      active = true
    }
    updateDragAim(ev.clientX, ev.clientY)
  }

  function onUp(ev?: PointerEvent) {
    el.removeEventListener('pointermove', onMove)
    el.removeEventListener('pointerup', onUp)
    el.removeEventListener('pointercancel', onUp)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
    if (!active) {
      drag.value = null
      return
    }
    active = false
    finishDrag(ev?.clientX, ev?.clientY)
  }
}

onUnmounted(() => {
  clearDragChrome()
})

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

    <div ref="sideBodyRef" class="kb-side-body">
      <div
        v-if="lineTop != null"
        class="kb-insert-line"
        :style="{ top: `${lineTop}px` }"
        aria-hidden="true"
      />

      <!-- 置顶（区头常驻，供拖放命中；空列表时 body 为空） -->
      <section class="kb-sec" data-kb-drop="pinned-head">
        <div
          class="kb-sec-head"
          data-kb-drop="pinned-head"
          :class="{ 'drop-target': isHotContainer('pinned') }"
        >
          <Pin :size="11" :stroke-width="2" aria-hidden="true" />
          <span>置顶</span>
        </div>
        <div
          v-for="s in pinned"
          :key="'pin-' + s.id"
          class="kb-sess"
          data-kb-drop="session"
          data-kb-zone="pinned"
          :data-session-id="s.id"
          :class="{
            on: s.id === activeId,
            disabled,
            'is-dragging': drag?.active && drag.id === s.id && drag.sourceZone === 'pinned',
          }"
          @pointerdown="onSessionPointerDown(s, 'pinned', $event)"
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
            data-kb-drop="project-row"
            :data-project-id="block.project.id"
            :class="{ 'drop-target': isHotContainer(`project-${block.project.id}`) }"
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
              data-kb-drop="session"
              data-kb-zone="project"
              :data-session-id="s.id"
              :data-project-id="block.project.id"
              :class="{
                on: s.id === activeId,
                disabled,
                'is-dragging':
                  drag?.active && drag.id === s.id && drag.sourceZone === 'project',
              }"
              @pointerdown="onSessionPointerDown(s, 'project', $event)"
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
              :class="{ 'is-hot': isHotContainer(`project-${block.project.id}`) }"
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
      <section class="kb-sec" data-kb-drop="recent-head">
        <div
          class="kb-sec-head"
          data-kb-drop="recent-head"
          :class="{ 'drop-target': isHotContainer('recent') }"
        >
          <span>最近</span>
        </div>
        <div
          v-for="s in recent"
          :key="'recent-' + s.id"
          class="kb-sess"
          data-kb-drop="session"
          data-kb-zone="recent"
          :data-session-id="s.id"
          :class="{
            on: s.id === activeId,
            disabled,
            'is-dragging': drag?.active && drag.id === s.id && drag.sourceZone === 'recent',
          }"
          @pointerdown="onSessionPointerDown(s, 'recent', $event)"
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
  position: relative;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 6px 8px;
}

.kb-insert-line {
  position: absolute;
  left: 6px;
  right: 6px;
  height: 2px;
  background: var(--brand-500);
  border-radius: 1px;
  pointer-events: none;
  z-index: 2;
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
/* 拖中行半透明；指针穿透整行（含子孙），否则 elementFromPoint 永远命中自身 */
.kb-sess.is-dragging {
  opacity: 0.45;
  pointer-events: none;
}
.kb-sess.is-dragging * {
  pointer-events: none;
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
.kb-proj-row.drop-target,
.kb-sec-head.drop-target {
  background: color-mix(in srgb, var(--brand-500) 18%, transparent);
  outline: 1px solid color-mix(in srgb, var(--brand-500) 55%, transparent);
  outline-offset: -1px;
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

/* 拖拽期间全局禁选 + 抓手光标（body 在组件外，用 :global 整条包进同一括号） */
:global(body.kb-session-dragging) {
  cursor: grabbing;
  user-select: none;
  -webkit-user-select: none;
}
</style>
