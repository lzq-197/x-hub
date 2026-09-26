<script setup lang="ts">
import { computed, inject, onMounted, onUnmounted, ref, watch } from 'vue'
import { BrainCircuit, RefreshCw, Settings2, Send, X } from 'lucide-vue-next'
import AppSelect from './AppSelect.vue'
import { useFocusTrap } from '../composables/useFocusTrap'
import {
  PLATFORM_ENTRY_NAME,
  isPlatformModel,
  isTauri,
  tauriApi,
  type ChatModelConfig,
  type Citation,
  type KbAskEvent,
  type KbEmbedConfigView,
  type KbStatus,
} from '../api/tauri'
import { renderMarkdown } from '../utils/markdownHtml'

const emit = defineEmits<{
  (e: 'open-note', noteId: number): void
}>()

const showToast = inject<(msg: string) => void>('showToast', () => {})

// ---- 索引状态轮询 ----
const status = ref<KbStatus | null>(null)
const rebuilding = ref(false)
let pollTimer: ReturnType<typeof setTimeout> | null = null

async function refreshStatus() {
  if (!isTauri()) return
  try {
    status.value = await tauriApi.kbGetStatus()
  } catch {
    /* 静默；下次轮询再试 */
  }
}

function schedulePoll() {
  if (pollTimer) clearTimeout(pollTimer)
  const ms = status.value?.status === 'indexing' ? 1000 : 30000
  pollTimer = setTimeout(async () => {
    await refreshStatus()
    schedulePoll()
  }, ms)
}

const statusKind = computed(() => {
  const s = status.value?.status ?? 'idle'
  if (s === 'indexing') return 'indexing'
  if (s === 'error') return 'error'
  if (s === 'done') return 'done'
  return 'idle'
})

const statusLabel = computed(() => {
  const s = status.value
  if (!s) return '加载中…'
  if (s.status === 'indexing') {
    // status.progress 已是 0–100 百分比（后端 set_meta_progress），勿再除 total_notes
    const pct = Math.min(100, Math.max(0, Math.round(Number(s.progress) || 0)))
    return `索引中 ${pct}%`
  }
  if (s.status === 'error') return s.error?.trim() || '索引出错'
  if (s.status === 'done') return '已就绪'
  return '空闲'
})

const lastIndexedLabel = computed(() => {
  const raw = status.value?.last_indexed_at
  if (!raw) return '尚未索引'
  const d = new Date(raw)
  if (Number.isNaN(d.getTime())) return raw
  return d.toLocaleString('zh-CN', {
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
})

async function onRebuild() {
  if (!isTauri() || rebuilding.value) return
  rebuilding.value = true
  try {
    await tauriApi.kbRebuildIndex((e) => {
      if (status.value) {
        const progress =
          e.total > 0 ? Math.min(99, Math.round((e.done * 100) / e.total)) : 100
        status.value = {
          ...status.value,
          status: 'indexing',
          progress,
          total_notes: e.total,
        }
      }
    })
    await refreshStatus()
    showToast('索引重建完成')
  } catch (e) {
    await refreshStatus()
    showToast(`重建失败：${String(e)}`)
  } finally {
    rebuilding.value = false
    schedulePoll()
  }
}

// ---- 嵌入设置弹窗 ----
const embedOpen = ref(false)
const embedCardRef = ref<HTMLElement | null>(null)
useFocusTrap(embedOpen, embedCardRef)

const embedBaseUrl = ref('http://127.0.0.1:11434/v1')
const embedModel = ref('bge-m3')
const embedApiKey = ref('')
const embedHasKey = ref(false)
const embedTopK = ref(6)
const embedSaving = ref(false)
const embedTesting = ref(false)
const embedTestMsg = ref('')
const embedTestOk = ref<boolean | null>(null)

async function openEmbedSettings() {
  embedTestMsg.value = ''
  embedTestOk.value = null
  embedApiKey.value = ''
  if (isTauri()) {
    try {
      const cfg: KbEmbedConfigView = await tauriApi.getKbEmbedConfig()
      embedBaseUrl.value = cfg.base_url || 'http://127.0.0.1:11434/v1'
      embedModel.value = cfg.model || 'bge-m3'
      embedHasKey.value = cfg.has_api_key
      embedTopK.value = Math.min(10, Math.max(3, cfg.top_k || 6))
    } catch (e) {
      showToast(`读取嵌入配置失败：${String(e)}`)
    }
  }
  embedOpen.value = true
}

async function saveEmbed() {
  if (!isTauri() || embedSaving.value) return
  const topK = Math.min(10, Math.max(3, Math.round(embedTopK.value) || 6))
  embedSaving.value = true
  try {
    const cfg = await tauriApi.saveKbEmbedConfig(
      embedBaseUrl.value.trim(),
      embedModel.value.trim(),
      embedApiKey.value,
      topK,
    )
    embedHasKey.value = cfg.has_api_key
    embedApiKey.value = ''
    embedTopK.value = cfg.top_k
    askTopK.value = cfg.top_k
    showToast('嵌入设置已保存')
    embedOpen.value = false
    await refreshStatus()
  } catch (e) {
    showToast(`保存失败：${String(e)}`)
  } finally {
    embedSaving.value = false
  }
}

async function testEmbed() {
  if (!isTauri() || embedTesting.value) return
  embedTesting.value = true
  embedTestMsg.value = ''
  embedTestOk.value = null
  try {
    const r = await tauriApi.kbTestEmbed(
      embedBaseUrl.value.trim(),
      embedModel.value.trim(),
      embedApiKey.value,
    )
    embedTestOk.value = r.ok
    embedTestMsg.value = r.message
  } catch (e) {
    embedTestOk.value = false
    embedTestMsg.value = String(e)
  } finally {
    embedTesting.value = false
  }
}

// ---- 问答 ----
const models = ref<ChatModelConfig[]>([])
const accountLoggedIn = ref(false)
const selectedModel = ref('')
const askTopK = ref(6)
const question = ref('')
const asking = ref(false)
const streamText = ref('')
const streamHtml = ref('')
const answerError = ref('')
const citations = ref<Citation[]>([])
const noHit = ref(false)
const askedOnce = ref(false)

const platformEnabled = computed(() => models.value.some((m) => isPlatformModel(m)))

const modelOptions = computed(() => {
  const opts: { value: string; label: string; group: string }[] = []
  if (platformEnabled.value) {
    opts.push({ value: PLATFORM_ENTRY_NAME, label: PLATFORM_ENTRY_NAME, group: '' })
  }
  for (const m of models.value) {
    if (isPlatformModel(m)) continue
    const label = (m.model ?? '').trim() || m.name
    const group = (m.provider_name ?? '').trim() || (m.base_url ?? '').trim() || '其他'
    opts.push({ value: m.name, label, group })
  }
  return opts
})

function displayModelName(name: string): string {
  const m = models.value.find((x) => x.name === name)
  return m && isPlatformModel(m) ? PLATFORM_ENTRY_NAME : name
}

function defaultModelName(): string {
  const def = models.value.find((m) => m.is_default)
  if (def) return displayModelName(def.name)
  return modelOptions.value[0]?.value ?? ''
}

const topKOptions = computed(() =>
  Array.from({ length: 8 }, (_, i) => {
    const v = String(i + 3)
    return { value: v, label: `Top-K ${v}`, group: '' }
  }),
)

const topKValue = computed({
  get: () => String(askTopK.value),
  set: (v: string) => {
    askTopK.value = Math.min(10, Math.max(3, Number(v) || 6))
  },
})

async function loadModels() {
  if (!isTauri()) return
  try {
    models.value = await tauriApi.getChatModels()
    accountLoggedIn.value = (await tauriApi.accountStatus()).loggedIn
  } catch {
    models.value = []
  }
  const cur = selectedModel.value
  selectedModel.value = modelOptions.value.some((o) => o.value === cur)
    ? cur
    : defaultModelName()
}

function onAskKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    void submitAsk()
  }
}

async function submitAsk() {
  const q = question.value.trim()
  if (!q || asking.value || !isTauri()) return
  if (!selectedModel.value || modelOptions.value.length === 0) {
    showToast('请先在设置 → AI 助手中配置对话模型')
    return
  }
  if (selectedModel.value === PLATFORM_ENTRY_NAME) {
    if (!platformEnabled.value) {
      showToast('平台额度未开启（设置 → AI 助手）')
      return
    }
    if (!accountLoggedIn.value) {
      showToast('平台额度需登录账号（设置 → 账号）')
      return
    }
  } else {
    const m = models.value.find((x) => x.name === selectedModel.value)
    if (!m?.has_api_key) {
      showToast('当前模型未配置 API Key（设置 → AI 助手）')
      return
    }
  }

  asking.value = true
  askedOnce.value = true
  streamText.value = ''
  streamHtml.value = ''
  answerError.value = ''
  citations.value = []
  noHit.value = false

  try {
    await tauriApi.kbAsk(q, selectedModel.value, askTopK.value, (e: KbAskEvent) => {
      if (e.type === 'chunk') {
        streamText.value += e.content
        streamHtml.value = renderMarkdown(streamText.value)
      } else if (e.type === 'done') {
        streamText.value = e.answer
        streamHtml.value = renderMarkdown(e.answer)
        citations.value = e.citations
        noHit.value = e.citations.length === 0
        if (e.kbStatus) status.value = e.kbStatus
      } else if (e.type === 'error') {
        answerError.value = e.message
        if (e.partial) {
          streamText.value = e.partial
          streamHtml.value = renderMarkdown(e.partial)
        }
      }
    })
  } catch (e) {
    answerError.value = String(e)
  } finally {
    asking.value = false
  }
}

onMounted(async () => {
  await Promise.all([refreshStatus(), loadModels()])
  if (isTauri()) {
    try {
      const cfg = await tauriApi.getKbEmbedConfig()
      askTopK.value = Math.min(10, Math.max(3, cfg.top_k || 6))
    } catch {
      /* 默认 6 */
    }
  }
  schedulePoll()
})

onUnmounted(() => {
  if (pollTimer) clearTimeout(pollTimer)
})

watch(embedOpen, (open) => {
  if (!open) {
    embedApiKey.value = ''
    embedTestMsg.value = ''
    embedTestOk.value = null
  }
})
</script>

<template>
  <section class="view view-kb" tabindex="-1" aria-label="知识库">
    <div class="kb-root card">
      <!-- 顶栏：索引状态 -->
      <header class="kb-bar">
        <div class="kb-bar-left">
          <BrainCircuit class="kb-bar-icon" :size="16" :stroke-width="2" aria-hidden="true" />
          <span class="kb-bar-title">知识库</span>
          <span class="kb-dot" :data-kind="statusKind" :title="statusLabel" aria-hidden="true" />
          <span class="kb-status-text">{{ statusLabel }}</span>
        </div>
        <div class="kb-bar-meta">
          <span class="kb-meta-item" :title="status?.model || ''">
            模型 {{ status?.model || '—' }}
          </span>
          <span class="kb-meta-item">片段 {{ status?.chunk_count ?? 0 }}</span>
          <span class="kb-meta-item">已索引 {{ status?.indexed_notes ?? 0 }}</span>
          <span class="kb-meta-item">{{ lastIndexedLabel }}</span>
        </div>
        <div class="kb-bar-actions">
          <button
            class="ghost-btn"
            type="button"
            title="清空并重切全部笔记片段。若「来源」曾乱码：升级本修复后请点一次重建，旧片段才会按正确中文重切"
            :disabled="rebuilding || statusKind === 'indexing'"
            @click="onRebuild"
          >
            <RefreshCw :size="14" :stroke-width="2" :class="{ spin: rebuilding }" aria-hidden="true" />
            重建索引
          </button>
          <button class="ghost-btn" type="button" @click="openEmbedSettings">
            <Settings2 :size="14" :stroke-width="2" aria-hidden="true" />
            嵌入设置
          </button>
        </div>
      </header>

      <p v-if="statusKind === 'error' && status?.error" class="kb-error-banner">
        {{ status.error }}
        <button class="kb-link" type="button" @click="openEmbedSettings">打开嵌入设置</button>
      </p>

      <!-- 问答区（全宽，无地图/标签云） -->
      <div class="kb-ask">
        <div class="kb-ask-toolbar">
          <AppSelect
            v-model="selectedModel"
            class="kb-model-select"
            :options="modelOptions"
            :disabled="asking || modelOptions.length === 0"
            aria-label="对话模型"
            compact
          />
          <AppSelect
            v-model="topKValue"
            class="kb-topk-select"
            :options="topKOptions"
            :disabled="asking"
            aria-label="检索条数"
            compact
          />
        </div>

        <div class="kb-composer">
          <textarea
            v-model="question"
            class="kb-input"
            rows="3"
            placeholder="基于你的笔记提问…（Enter 发送，Shift+Enter 换行）"
            :disabled="asking"
            @keydown="onAskKeydown"
          />
          <button
            class="pill-btn kb-send"
            type="button"
            :disabled="asking || !question.trim() || modelOptions.length === 0"
            @click="submitAsk"
          >
            <Send :size="14" :stroke-width="2" aria-hidden="true" />
            {{ asking ? '生成中…' : '提问' }}
          </button>
        </div>

        <div v-if="askedOnce" class="kb-answer card">
          <div v-if="asking && !streamText" class="kb-answer-pending">正在检索并生成…</div>
          <div v-else-if="streamHtml" class="kb-md" v-html="streamHtml" />
          <div v-else-if="!answerError" class="kb-answer-pending">暂无内容</div>

          <p v-if="answerError" class="kb-answer-err">
            {{ answerError }}
            <button class="kb-link" type="button" @click="openEmbedSettings">检查嵌入设置</button>
          </p>

          <p v-if="noHit && !asking" class="kb-nohit">
            知识库中未找到直接相关内容，以下为模型通用回答。
          </p>

          <div v-if="citations.length" class="kb-citations">
            <h3 class="kb-cite-heading">来源</h3>
            <button
              v-for="c in citations"
              :key="`${c.index}-${c.note_id}`"
              type="button"
              class="kb-cite"
              @click="emit('open-note', c.note_id)"
            >
              <span class="kb-cite-idx">[{{ c.index }}]</span>
              <span class="kb-cite-body">
                <span class="kb-cite-title">{{ c.note_title || '无标题笔记' }}</span>
                <span class="kb-cite-path">
                  <template v-if="c.folder_path">{{ c.folder_path }}</template>
                  <template v-if="c.folder_path && c.heading"> · </template>
                  <template v-if="c.heading">{{ c.heading }}</template>
                </span>
                <span v-if="c.snippet" class="kb-cite-snip">{{ c.snippet }}</span>
              </span>
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- 嵌入设置弹窗 -->
    <Teleport to="body">
      <Transition name="mask">
        <div v-if="embedOpen" class="modal-mask" @click.self="embedOpen = false">
          <div
            ref="embedCardRef"
            class="modal-card kb-embed-card"
            role="dialog"
            aria-label="嵌入设置"
            aria-modal="true"
          >
            <header class="kb-embed-head">
              <h2 class="dialog-title">嵌入设置</h2>
              <button
                class="icon-btn"
                type="button"
                title="关闭"
                aria-label="关闭"
                @click="embedOpen = false"
              >
                <X :size="14" :stroke-width="2" aria-hidden="true" />
              </button>
            </header>
            <div class="kb-embed-body">
              <label class="kb-field">
                <span class="kb-field-label">Base URL</span>
                <input
                  v-model="embedBaseUrl"
                  class="kb-field-input"
                  type="text"
                  spellcheck="false"
                  placeholder="http://127.0.0.1:11434/v1"
                />
              </label>
              <label class="kb-field">
                <span class="kb-field-label">模型</span>
                <input
                  v-model="embedModel"
                  class="kb-field-input"
                  type="text"
                  spellcheck="false"
                  placeholder="bge-m3"
                />
              </label>
              <label class="kb-field">
                <span class="kb-field-label">API Key</span>
                <input
                  v-model="embedApiKey"
                  class="kb-field-input"
                  type="password"
                  autocomplete="off"
                  spellcheck="false"
                  :placeholder="embedHasKey ? '已保存（留空保留）' : '可选，本地 Ollama 通常不需要'"
                />
              </label>
              <label class="kb-field">
                <span class="kb-field-label">默认 Top-K（3–10）</span>
                <input
                  v-model.number="embedTopK"
                  class="kb-field-input"
                  type="number"
                  min="3"
                  max="10"
                  step="1"
                />
              </label>
              <p
                v-if="embedTestMsg"
                class="kb-test-msg"
                :data-ok="embedTestOk === true ? '1' : embedTestOk === false ? '0' : undefined"
              >
                {{ embedTestMsg }}
              </p>
            </div>
            <footer class="kb-embed-foot">
              <button
                class="ghost-btn"
                type="button"
                :disabled="embedTesting || embedSaving"
                @click="testEmbed"
              >
                {{ embedTesting ? '测试中…' : '测试连接' }}
              </button>
              <div class="kb-embed-foot-spacer" />
              <button class="ghost-btn" type="button" :disabled="embedSaving" @click="embedOpen = false">
                取消
              </button>
              <button class="pill-btn" type="button" :disabled="embedSaving" @click="saveEmbed">
                {{ embedSaving ? '保存中…' : '保存' }}
              </button>
            </footer>
          </div>
        </div>
      </Transition>
    </Teleport>
  </section>
</template>

<style scoped>
.view-kb {
  height: 100%;
  min-height: 0;
  padding: 0 20px 20px 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.kb-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 0;
  padding: 0;
  overflow: hidden;
}

.kb-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px 16px;
  padding: 14px 18px;
  border-bottom: 1px solid var(--border-soft);
}

.kb-bar-left {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.kb-bar-icon {
  color: var(--brand-500);
  flex-shrink: 0;
}

.kb-bar-title {
  font-size: 0.9375rem;
  font-weight: 650;
  color: var(--text-1);
}

.kb-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
  background: var(--text-4);
}
.kb-dot[data-kind='indexing'] {
  background: var(--c-blue);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--c-blue) 25%, transparent);
}
.kb-dot[data-kind='done'] {
  background: var(--c-green);
}
.kb-dot[data-kind='error'] {
  background: var(--c-red);
}

.kb-status-text {
  font-size: 0.75rem;
  color: var(--text-2);
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.kb-bar-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 12px;
  flex: 1;
  min-width: 0;
}

.kb-meta-item {
  font-size: 0.6875rem;
  color: var(--text-3);
  white-space: nowrap;
}

.kb-bar-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-left: auto;
}

.spin {
  animation: kb-spin 0.9s linear infinite;
}
@keyframes kb-spin {
  to {
    transform: rotate(360deg);
  }
}

.kb-error-banner {
  margin: 0;
  padding: 10px 18px;
  font-size: 0.8125rem;
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 8%, transparent);
  border-bottom: 1px solid var(--border-soft);
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.kb-link {
  border: none;
  background: none;
  padding: 0;
  color: var(--brand-500);
  font-size: inherit;
  font-weight: 600;
  cursor: pointer;
  text-decoration: underline;
  text-underline-offset: 2px;
}
.kb-link:hover {
  color: var(--brand-600);
}

.kb-ask {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 16px 18px 18px;
  overflow: auto;
}

.kb-ask-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
}

.kb-model-select {
  min-width: 160px;
  max-width: 280px;
}
.kb-topk-select {
  width: 110px;
}

.kb-composer {
  display: flex;
  gap: 10px;
  align-items: flex-end;
}

.kb-input {
  flex: 1;
  min-width: 0;
  resize: vertical;
  min-height: 72px;
  max-height: 200px;
  padding: 10px 12px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  color: var(--text-1);
  font-size: 0.8125rem;
  line-height: 1.5;
  font-family: inherit;
}
.kb-input:focus {
  outline: none;
  border-color: color-mix(in srgb, var(--brand-500) 55%, transparent);
  box-shadow: var(--shadow-focus);
}
.kb-input:disabled {
  opacity: 0.65;
}
.kb-input::placeholder {
  color: var(--text-4);
}

.kb-send {
  flex-shrink: 0;
  align-self: flex-end;
}

.kb-answer {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.kb-answer-pending {
  font-size: 0.8125rem;
  color: var(--text-3);
}

.kb-answer-err {
  margin: 0;
  font-size: 0.8125rem;
  color: var(--c-red);
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
}

.kb-nohit {
  margin: 0;
  padding: 8px 10px;
  font-size: 0.75rem;
  color: var(--text-2);
  background: var(--bg-card-soft);
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-soft);
}

.kb-md {
  font-size: 0.8125rem;
  line-height: 1.65;
  color: var(--text-1);
  word-break: break-word;
}
.kb-md :deep(p) {
  margin: 0 0 0.65em;
}
.kb-md :deep(p:last-child) {
  margin-bottom: 0;
}
.kb-md :deep(ul),
.kb-md :deep(ol) {
  margin: 0 0 0.65em;
  padding-left: 1.4em;
}
.kb-md :deep(code) {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.92em;
  padding: 0.1em 0.35em;
  border-radius: 4px;
  background: var(--bg-card-soft);
}
.kb-md :deep(pre) {
  margin: 0 0 0.65em;
  padding: 10px 12px;
  overflow: auto;
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
}
.kb-md :deep(pre code) {
  padding: 0;
  background: none;
}
.kb-md :deep(a) {
  color: var(--brand-500);
}

.kb-cite-heading {
  margin: 0 0 8px;
  font-size: 0.75rem;
  font-weight: 650;
  color: var(--text-2);
}

.kb-citations {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-top: 4px;
  border-top: 1px solid var(--border-soft);
}

.kb-cite {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  text-align: left;
  padding: 10px 12px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s, transform 0.15s;
}
.kb-cite:hover {
  border-color: color-mix(in srgb, var(--brand-500) 40%, transparent);
  background: color-mix(in srgb, var(--brand-50) 55%, var(--bg-card-soft));
  transform: translateY(-1px);
}
.kb-cite:active {
  transform: scale(0.99);
}

.kb-cite-idx {
  flex-shrink: 0;
  font-size: 0.75rem;
  font-weight: 700;
  color: var(--brand-500);
  font-variant-numeric: tabular-nums;
}

.kb-cite-body {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.kb-cite-title {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
}

.kb-cite-path {
  font-size: 0.6875rem;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.kb-cite-snip {
  font-size: 0.75rem;
  color: var(--text-2);
  line-height: 1.45;
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
}

/* 嵌入设置：清零 .modal-card 默认 24px，与扩展弹窗口径一致 */
.kb-embed-card {
  width: 440px;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0;
}
.kb-embed-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 16px 18px 0;
}
.kb-embed-head .dialog-title {
  margin: 0;
  font-size: 1rem;
  font-weight: 650;
}
.kb-embed-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 14px 18px;
}
.kb-embed-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 18px;
  border-top: 1px solid var(--border-soft);
}
.kb-embed-foot-spacer {
  flex: 1;
}

.kb-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.kb-field-label {
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-2);
}
.kb-field-input {
  padding: 8px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  color: var(--text-1);
  font-size: 0.8125rem;
  font-family: inherit;
}
.kb-field-input:focus {
  outline: none;
  border-color: color-mix(in srgb, var(--brand-500) 55%, transparent);
  box-shadow: var(--shadow-focus);
}

.kb-test-msg {
  margin: 0;
  font-size: 0.75rem;
  color: var(--text-2);
}
.kb-test-msg[data-ok='1'] {
  color: var(--c-green);
}
.kb-test-msg[data-ok='0'] {
  color: var(--c-red);
}
</style>
