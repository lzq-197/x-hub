<script setup lang="ts">
import { computed, inject, onBeforeUnmount, ref, toRef, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { FileText, FolderOpen, Loader2, X } from 'lucide-vue-next'
import { isTauri, type ImportResult } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useFocusTrap } from '../composables/useFocusTrap'

const props = defineProps<{ visible: boolean }>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'done', result: ImportResult): void
}>()

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})

const cardRef = ref<HTMLElement | null>(null)
useFocusTrap(toRef(props, 'visible'), cardRef)

const selectedPath = ref('')
const busy = ref(false)
const result = ref<ImportResult | null>(null)
const runError = ref('')
const errorsOpen = ref(false)

const canImport = computed(() => !!selectedPath.value && !busy.value && !result.value)
const errorPreview = computed(() => (result.value?.errors ?? []).slice(0, 20))

function reset() {
  selectedPath.value = ''
  busy.value = false
  result.value = null
  runError.value = ''
  errorsOpen.value = false
}

watch(
  () => props.visible,
  (v) => {
    if (v) reset()
    if (typeof window === 'undefined') return
    if (v) window.addEventListener('keydown', onKeydown)
    else window.removeEventListener('keydown', onKeydown)
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  if (typeof window !== 'undefined') window.removeEventListener('keydown', onKeydown)
})

function onKeydown(e: KeyboardEvent) {
  if (!props.visible) return
  if (e.key === 'Escape' && !busy.value) {
    e.preventDefault()
    requestClose()
  }
}

function requestClose() {
  if (busy.value) return
  emit('close')
}

async function pickDirectory() {
  if (!isTauri() || busy.value) {
    if (!isTauri()) showToast('请在桌面应用中导入')
    return
  }
  const dir = await open({ multiple: false, directory: true })
  if (typeof dir !== 'string') return
  selectedPath.value = dir
  result.value = null
  runError.value = ''
}

async function pickFile() {
  if (!isTauri() || busy.value) {
    if (!isTauri()) showToast('请在桌面应用中导入')
    return
  }
  const file = await open({
    multiple: false,
    directory: false,
    filters: [{ name: 'Markdown', extensions: ['md', 'markdown'] }],
  })
  if (typeof file !== 'string') return
  selectedPath.value = file
  result.value = null
  runError.value = ''
}

function formatSummary(r: ImportResult): string {
  return `导入 ${r.imported} · 更新 ${r.updated} · 跳过 ${r.skipped} · 失败 ${r.failed}`
}

async function startImport() {
  if (!canImport.value || !isTauri()) return
  busy.value = true
  runError.value = ''
  result.value = null
  try {
    const r = await store.importMarkdown(selectedPath.value)
    result.value = r
    const msg = formatSummary(r)
    showToast(msg)
    emit('done', r)
  } catch (e) {
    runError.value = String(e)
    showToast(`导入失败：${String(e)}`)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask" @click.self="requestClose">
        <div
          ref="cardRef"
          class="modal-card import-card"
          role="dialog"
          aria-label="导入 Markdown"
          aria-modal="true"
        >
          <header class="import-head">
            <h2 class="dialog-title">导入 Markdown</h2>
            <button
              class="icon-btn"
              type="button"
              title="关闭"
              aria-label="关闭"
              :disabled="busy"
              @click="requestClose"
            >
              <X :size="14" :stroke-width="2" />
            </button>
          </header>

          <p class="import-hint">将按目录结构导入 Markdown 文件；已导入过的路径会覆盖更新。</p>

          <div class="import-pick">
            <button
              class="pill-btn import-pick-btn"
              type="button"
              :disabled="busy"
              @click="pickDirectory"
            >
              <FolderOpen :size="15" :stroke-width="2" />
              选择文件夹
            </button>
            <button
              class="ghost-btn import-pick-btn"
              type="button"
              :disabled="busy"
              @click="pickFile"
            >
              <FileText :size="15" :stroke-width="2" />
              导入文件
            </button>
          </div>

          <div v-if="selectedPath" class="import-path" :title="selectedPath">
            <span class="import-path-label">已选路径</span>
            <code class="import-path-value">{{ selectedPath }}</code>
          </div>

          <div v-if="busy" class="import-busy">
            <Loader2 :size="22" :stroke-width="1.8" class="spin" />
            <span>正在导入…</span>
          </div>

          <p v-else-if="runError" class="import-error">{{ runError }}</p>

          <div v-else-if="result" class="import-result">
            <p class="import-summary">{{ formatSummary(result) }}</p>
            <p class="import-total">共处理 {{ result.total }} 个文件</p>
            <details
              v-if="errorPreview.length"
              class="import-errors"
              :open="errorsOpen"
              @toggle="errorsOpen = ($event.target as HTMLDetailsElement).open"
            >
              <summary>错误详情（{{ result.errors.length }}）</summary>
              <ul>
                <li v-for="(err, i) in errorPreview" :key="i">{{ err }}</li>
              </ul>
            </details>
          </div>

          <footer class="import-foot">
            <button class="ghost-btn" type="button" :disabled="busy" @click="requestClose">
              {{ result ? '关闭' : '取消' }}
            </button>
            <button
              v-if="!result"
              class="pill-btn"
              type="button"
              :disabled="!canImport"
              @click="startImport"
            >
              开始导入
            </button>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.import-card {
  width: 440px;
  padding: 20px 22px;
}
.import-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}
.dialog-title {
  margin: 0;
  font-size: 1rem;
  font-weight: 600;
  color: var(--text-1);
}
.import-hint {
  margin: 0 0 14px;
  font-size: 0.78rem;
  line-height: 1.55;
  color: var(--text-3);
}
.import-pick {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 12px;
}
.import-pick-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.import-path {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 14px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
}
.import-path-label {
  font-size: 0.7rem;
  font-weight: 600;
  color: var(--text-4);
}
.import-path-value {
  font-size: 0.75rem;
  line-height: 1.45;
  color: var(--text-2);
  word-break: break-all;
  white-space: pre-wrap;
  font-family: inherit;
}
.import-busy {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 14px;
  font-size: 0.8rem;
  color: var(--text-2);
}
.spin {
  animation: import-spin 0.9s linear infinite;
}
@keyframes import-spin {
  to {
    transform: rotate(360deg);
  }
}
.import-error {
  margin: 0 0 14px;
  font-size: 0.78rem;
  line-height: 1.5;
  color: var(--c-red-ink);
}
.import-result {
  margin-bottom: 14px;
}
.import-summary {
  margin: 0;
  font-size: 0.85rem;
  font-weight: 600;
  color: var(--text-1);
}
.import-total {
  margin: 4px 0 0;
  font-size: 0.72rem;
  color: var(--text-4);
}
.import-errors {
  margin-top: 10px;
  font-size: 0.72rem;
  color: var(--text-3);
}
.import-errors summary {
  cursor: pointer;
  color: var(--text-2);
  user-select: none;
}
.import-errors ul {
  margin: 8px 0 0;
  padding-left: 1.1em;
  max-height: 140px;
  overflow: auto;
  line-height: 1.5;
}
.import-errors li {
  margin-bottom: 4px;
  word-break: break-word;
}
.import-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.icon-btn:disabled,
.ghost-btn:disabled,
.pill-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
