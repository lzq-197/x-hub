<script setup lang="ts">
import { computed, inject, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { BrainCircuit, RefreshCw, Settings2, Send, X } from 'lucide-vue-next'
import AppSelect from './AppSelect.vue'
import ConfirmDialog from './ConfirmDialog.vue'
import KbSessionSidebar from './KbSessionSidebar.vue'
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
  type KbMessage,
  type KbProject,
  type KbSession,
  type KbStatus,
} from '../api/tauri'
import { renderMarkdown } from '../utils/markdownHtml'
import {
  stripClarification,
  confusionHint,
  groupCitationsByNote,
  linkifyCiteRefs,
} from '../utils/kbAskDecorate'
import { kbAskInFlight } from '../utils/kbAskFlight'

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

// ---- 会话 / 消息 ----
type UiMessage = {
  id: number | string
  role: 'user' | 'assistant'
  content: string
  citations: Citation[]
  html: string
  banner: string | null
  error: string
  streaming: boolean
}

const projects = ref<KbProject[]>([])
const sessions = ref<KbSession[]>([])
const activeSessionId = ref<number | null>(null)
const messages = ref<UiMessage[]>([])
const asking = ref(false)
const clearOpen = ref(false)
const messagesEl = ref<HTMLElement | null>(null)
let askFlightPollTimer: ReturnType<typeof setInterval> | null = null

const expandedSnips = ref<Set<string>>(new Set())
const flashCite = ref<string | null>(null)
let flashTimer: ReturnType<typeof setTimeout> | null = null

function snipKey(msgId: number | string, index: number) {
  return `${msgId}:${index}`
}

function citeGroupsFor(cites: Citation[]) {
  return groupCitationsByNote(cites)
}

function toggleSnip(msgId: number | string, index: number) {
  const key = snipKey(msgId, index)
  const next = new Set(expandedSnips.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  expandedSnips.value = next
}

function onAnswerClick(e: MouseEvent) {
  const t = e.target as HTMLElement | null
  const btn = t?.closest?.('button.kb-ref') as HTMLElement | null
  if (!btn) return
  const n = Number(btn.getAttribute('data-ref'))
  if (!Number.isFinite(n)) return
  const root = btn.closest('.kb-msg') as HTMLElement | null
  const el = (root ?? document).querySelector(`[data-cite-index="${n}"]`) as HTMLElement | null
  el?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  const msgId = root?.getAttribute('data-msg-id') ?? ''
  flashCite.value = snipKey(msgId, n)
  if (flashTimer) clearTimeout(flashTimer)
  flashTimer = setTimeout(() => {
    flashCite.value = null
  }, 1200)
}

function decorateContent(
  raw: string,
  citations: Citation[],
  opts: { linkify: boolean; heuristic: boolean; userQuestion?: string },
): { html: string; banner: string | null } {
  const { banner, body } = stripClarification(raw)
  let msg = banner
  if (!msg && opts.heuristic && opts.userQuestion) {
    msg = confusionHint(
      opts.userQuestion,
      citations.map((c) => ({ note_title: c.note_title, heading: c.heading })),
    )
  }
  const html = renderMarkdown(body)
  if (opts.linkify) {
    return { html: linkifyCiteRefs(html, citations.map((c) => c.index)), banner: msg }
  }
  return { html, banner: msg }
}

function parseCitations(raw: string | null): Citation[] {
  if (!raw) return []
  try {
    const v = JSON.parse(raw) as unknown
    return Array.isArray(v) ? (v as Citation[]) : []
  } catch {
    return []
  }
}

function parseMsg(m: KbMessage, prevUserQuestion?: string): UiMessage {
  const role = m.role === 'assistant' ? 'assistant' : 'user'
  const citations = role === 'assistant' ? parseCitations(m.citations_json) : []
  const base: UiMessage = {
    id: m.id,
    role,
    content: m.content,
    citations,
    html: '',
    banner: null,
    error: '',
    streaming: false,
  }
  if (role === 'assistant') {
    const d = decorateContent(m.content, citations, {
      linkify: true,
      heuristic: true,
      userQuestion: prevUserQuestion,
    })
    base.html = d.html
    base.banner = d.banner
  }
  return base
}

function mapMessages(rows: KbMessage[]): UiMessage[] {
  const out: UiMessage[] = []
  let prevUser = ''
  for (const row of rows) {
    const ui = parseMsg(row, prevUser || undefined)
    out.push(ui)
    if (ui.role === 'user') prevUser = ui.content
  }
  return out
}

async function scrollMessagesBottom() {
  await nextTick()
  const el = messagesEl.value
  if (el) el.scrollTop = el.scrollHeight
}

async function refreshSessionLists() {
  sessions.value = await tauriApi.listKbSessions()
  projects.value = await tauriApi.listKbProjects()
}

function stopAskFlightPoll() {
  if (askFlightPollTimer) {
    clearInterval(askFlightPollTimer)
    askFlightPollTimer = null
  }
}

/** After remount: restore asking UI + poll messages until the in-flight ask leaves the set. */
function syncAskingFromInFlight(sessionId: number) {
  stopAskFlightPoll()
  if (!kbAskInFlight.has(sessionId)) return
  asking.value = true
  askFlightPollTimer = setInterval(async () => {
    if (!kbAskInFlight.has(sessionId)) {
      stopAskFlightPoll()
      asking.value = false
      if (activeSessionId.value === sessionId) {
        try {
          messages.value = mapMessages(await tauriApi.listKbMessages(sessionId))
          await refreshSessionLists()
          await scrollMessagesBottom()
        } catch {
          /* ignore */
        }
      }
      return
    }
    if (activeSessionId.value !== sessionId) return
    try {
      messages.value = mapMessages(await tauriApi.listKbMessages(sessionId))
    } catch {
      /* ignore */
    }
  }, 500)
}

async function openSession(id: number) {
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  activeSessionId.value = id
  await tauriApi.setKbActiveSession(id)
  messages.value = mapMessages(await tauriApi.listKbMessages(id))
  expandedSnips.value = new Set()
  flashCite.value = null
  const s = sessions.value.find((x) => x.id === id)
  if (s?.model_name) selectedModel.value = displayModelName(s.model_name) || selectedModel.value
  await scrollMessagesBottom()
  syncAskingFromInFlight(id)
}

async function bootstrapSessions() {
  if (!isTauri()) return
  projects.value = await tauriApi.listKbProjects()
  sessions.value = await tauriApi.listKbSessions()
  const cfg = await tauriApi.getUiConfig()
  const want = cfg.kb_active_session_id ?? null
  const ok = want != null && sessions.value.some((s) => s.id === want)
  if (ok) await openSession(want!)
  else if (sessions.value.length) {
    const recent = sessions.value.find((s) => !s.pinned && s.project_id == null)
    await openSession((recent ?? sessions.value[0]!).id)
  } else {
    activeSessionId.value = null
    messages.value = []
  }
}

async function onNewSession() {
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  if (!isTauri()) return
  try {
    const s = await tauriApi.createKbSession({
      modelName: selectedModel.value || null,
    })
    await refreshSessionLists()
    await openSession(s.id)
  } catch (e) {
    showToast(`新建失败：${String(e)}`)
  }
}

async function onRenameSession(id: number, title: string) {
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  try {
    await tauriApi.renameKbSession(id, title)
    await refreshSessionLists()
  } catch (e) {
    showToast(`重命名失败：${String(e)}`)
  }
}

async function onPinSession(id: number, pinned: boolean) {
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  try {
    await tauriApi.pinKbSession(id, pinned)
    await refreshSessionLists()
  } catch (e) {
    showToast(`操作失败：${String(e)}`)
  }
}

async function onMoveSession(id: number, projectId: number | null) {
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  try {
    await tauriApi.moveKbSessionToProject(id, projectId)
    await refreshSessionLists()
  } catch (e) {
    showToast(`移动失败：${String(e)}`)
  }
}

async function onDeleteSession(id: number) {
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  try {
    await tauriApi.deleteKbSession(id)
    await refreshSessionLists()
    if (activeSessionId.value === id) {
      if (sessions.value.length) {
        const recent = sessions.value.find((s) => !s.pinned && s.project_id == null)
        await openSession((recent ?? sessions.value[0]!).id)
      } else {
        activeSessionId.value = null
        messages.value = []
        await tauriApi.setKbActiveSession(null)
      }
    }
  } catch (e) {
    showToast(`删除失败：${String(e)}`)
  }
}

async function onCreateProject(name: string) {
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  try {
    await tauriApi.createKbProject(name)
    await refreshSessionLists()
  } catch (e) {
    showToast(`创建项目失败：${String(e)}`)
  }
}

async function onRenameProject(id: number, name: string) {
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  try {
    await tauriApi.renameKbProject(id, name)
    await refreshSessionLists()
  } catch (e) {
    showToast(`重命名失败：${String(e)}`)
  }
}

async function onDeleteProject(id: number) {
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  try {
    await tauriApi.deleteKbProject(id)
    await refreshSessionLists()
  } catch (e) {
    showToast(`删除项目失败：${String(e)}`)
  }
}

function onClearAllRequest() {
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  clearOpen.value = true
}

async function onClearConfirm() {
  clearOpen.value = false
  if (asking.value) {
    showToast('生成中，请稍候')
    return
  }
  try {
    await tauriApi.clearKbSessions()
    await refreshSessionLists()
    activeSessionId.value = null
    messages.value = []
    await tauriApi.setKbActiveSession(null)
  } catch (e) {
    showToast(`清空失败：${String(e)}`)
  }
}

// ---- 问答 ----
const models = ref<ChatModelConfig[]>([])
const accountLoggedIn = ref(false)
const selectedModel = ref('')
const askTopK = ref(6)
const question = ref('')
const inputEl = ref<HTMLTextAreaElement | null>(null)

/** Autosize like ChatPanel: grow with content, clamp ~6–8 lines. */
function autosize() {
  const el = inputEl.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = Math.min(el.scrollHeight, 160) + 'px'
}

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

function patchStreamingAssistant(patch: Partial<UiMessage>) {
  const list = messages.value
  const last = list[list.length - 1]
  if (!last || last.role !== 'assistant' || !last.streaming) return
  Object.assign(last, patch)
  // trigger reactivity for array item mutation
  messages.value = list.slice()
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

  let sessionId = activeSessionId.value
  if (sessionId == null) {
    try {
      const s = await tauriApi.createKbSession({ modelName: selectedModel.value })
      await refreshSessionLists()
      activeSessionId.value = s.id
      await tauriApi.setKbActiveSession(s.id)
      sessionId = s.id
    } catch (e) {
      showToast(`创建会话失败：${String(e)}`)
      return
    }
  }

  if (kbAskInFlight.has(sessionId)) {
    showToast('该会话正在生成中')
    return
  }
  kbAskInFlight.add(sessionId)

  asking.value = true
  question.value = ''
  void nextTick(() => autosize())
  messages.value.push({
    id: `u-${Date.now()}`,
    role: 'user',
    content: q,
    citations: [],
    html: '',
    banner: null,
    error: '',
    streaming: false,
  })
  messages.value.push({
    id: `a-${Date.now()}`,
    role: 'assistant',
    content: '',
    citations: [],
    html: '',
    banner: null,
    error: '',
    streaming: true,
  })
  await scrollMessagesBottom()

  let askFailed = false
  let askErrorMsg = ''
  try {
    await tauriApi.kbAsk(sessionId, q, selectedModel.value, askTopK.value, (e: KbAskEvent) => {
      if (e.type === 'chunk') {
        const last = messages.value[messages.value.length - 1]
        const next = (last?.content ?? '') + e.content
        const d = decorateContent(next, [], { linkify: false, heuristic: false })
        patchStreamingAssistant({ content: next, html: d.html, banner: d.banner })
        void scrollMessagesBottom()
      } else if (e.type === 'done') {
        const d = decorateContent(e.answer, e.citations, {
          linkify: true,
          heuristic: true,
          userQuestion: q,
        })
        patchStreamingAssistant({
          content: e.answer,
          citations: e.citations,
          html: d.html,
          banner: d.banner,
          streaming: false,
          error: '',
        })
        if (e.kbStatus) status.value = e.kbStatus
      } else if (e.type === 'error') {
        askFailed = true
        askErrorMsg = e.message
        const partial = e.partial ?? ''
        if (partial) {
          const d = decorateContent(partial, [], {
            linkify: true,
            heuristic: true,
            userQuestion: q,
          })
          patchStreamingAssistant({
            content: partial,
            html: d.html,
            banner: d.banner,
            error: e.message,
            streaming: false,
          })
        } else {
          patchStreamingAssistant({ error: e.message, streaming: false })
        }
      }
    })
    // 始终以库为准回刷：失败时库中只有 user、无 assistant，避免乐观气泡与 DB 长期漂移
    messages.value = mapMessages(await tauriApi.listKbMessages(sessionId))
    if (askFailed && askErrorMsg) showToast(askErrorMsg)
    await refreshSessionLists()
    await scrollMessagesBottom()
  } catch (e) {
    const msg = String(e)
    try {
      messages.value = mapMessages(await tauriApi.listKbMessages(sessionId))
      await refreshSessionLists()
    } catch {
      patchStreamingAssistant({ error: msg, streaming: false })
    }
    showToast(msg)
  } finally {
    kbAskInFlight.delete(sessionId)
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
    await bootstrapSessions()
  }
  schedulePoll()
})

onUnmounted(() => {
  if (pollTimer) clearTimeout(pollTimer)
  if (flashTimer) clearTimeout(flashTimer)
  stopAskFlightPoll()
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

      <div class="kb-body">
        <KbSessionSidebar
          :sessions="sessions"
          :projects="projects"
          :active-id="activeSessionId"
          :disabled="asking"
          @new="onNewSession"
          @select="openSession"
          @rename="onRenameSession"
          @pin="onPinSession"
          @move="onMoveSession"
          @delete="onDeleteSession"
          @clear-all="onClearAllRequest"
          @create-project="onCreateProject"
          @rename-project="onRenameProject"
          @delete-project="onDeleteProject"
        />

        <div class="kb-chat">
          <div ref="messagesEl" class="kb-messages">
            <div v-if="!messages.length" class="kb-empty">基于你的笔记提问</div>
            <div
              v-for="m in messages"
              :key="m.id"
              class="kb-msg"
              :data-role="m.role"
              :data-msg-id="String(m.id)"
            >
              <div v-if="m.role === 'user'" class="kb-bubble kb-bubble-user">
                {{ m.content }}
              </div>
              <div v-else class="kb-bubble kb-bubble-assistant">
                <div v-if="m.streaming && !m.content && !m.error" class="kb-answer-pending">
                  正在检索并生成…
                </div>
                <template v-else>
                  <p v-if="m.banner" class="kb-clarify" role="status">{{ m.banner }}</p>
                  <div
                    v-if="m.html"
                    class="kb-md md-body"
                    @click="onAnswerClick"
                    v-html="m.html"
                  />
                  <div v-else-if="!m.error" class="kb-answer-pending">暂无内容</div>
                </template>

                <p v-if="m.error" class="kb-answer-err">
                  {{ m.error }}
                  <button class="kb-link" type="button" @click="openEmbedSettings">检查嵌入设置</button>
                </p>

                <p v-if="!m.streaming && m.citations.length === 0 && m.content && !m.error" class="kb-nohit">
                  知识库中未找到直接相关内容，以下为模型通用回答。
                </p>

                <div v-if="m.citations.length" class="kb-citations">
                  <h3 class="kb-cite-heading">来源</h3>
                  <div
                    v-for="g in citeGroupsFor(m.citations)"
                    :key="g.note_id"
                    class="kb-cite-group"
                  >
                    <button
                      type="button"
                      class="kb-cite-group-head"
                      @click="emit('open-note', g.note_id)"
                    >
                      <span class="kb-cite-title">{{ g.note_title || '无标题笔记' }}</span>
                      <span class="kb-cite-path">
                        <template v-if="g.folder_path">{{ g.folder_path }}</template>
                        <template v-if="g.folder_path"> · </template>
                        {{ g.items.length }} 个片段
                      </span>
                    </button>
                    <div
                      v-for="c in g.items"
                      :key="c.index"
                      class="kb-cite"
                      :data-cite-index="c.index"
                      :data-flash="flashCite === snipKey(m.id, c.index) ? '1' : undefined"
                    >
                      <div class="kb-cite-row">
                        <button
                          type="button"
                          class="kb-cite-open"
                          title="打开笔记"
                          @click="emit('open-note', c.note_id)"
                        >
                          <span class="kb-cite-idx">[{{ c.index }}]</span>
                          <span v-if="c.heading" class="kb-cite-heading-text">{{ c.heading }}</span>
                        </button>
                        <button
                          v-if="c.snippet"
                          type="button"
                          class="kb-cite-chevron"
                          :title="expandedSnips.has(snipKey(m.id, c.index)) ? '收起摘要' : '展开摘要'"
                          :aria-expanded="expandedSnips.has(snipKey(m.id, c.index))"
                          @click="toggleSnip(m.id, c.index)"
                        >
                          {{ expandedSnips.has(snipKey(m.id, c.index)) ? '▾' : '▸' }}
                        </button>
                      </div>
                      <p
                        v-if="expandedSnips.has(snipKey(m.id, c.index)) && c.snippet"
                        class="kb-cite-snip"
                      >
                        {{ c.snippet }}
                      </p>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <div class="kb-composer-dock">
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

            <div class="kb-composer-box">
              <textarea
                ref="inputEl"
                v-model="question"
                class="kb-input"
                rows="1"
                placeholder="基于你的笔记提问…（Enter 发送，Shift+Enter 换行）"
                :disabled="asking"
                @input="autosize"
                @keydown="onAskKeydown"
              />
              <div class="kb-composer-foot">
                <span v-if="asking" class="kb-composer-hint">生成中…</span>
                <button
                  class="kb-send"
                  type="button"
                  :disabled="asking || !question.trim() || modelOptions.length === 0"
                  :title="asking ? '生成中…' : '发送'"
                  :aria-label="asking ? '生成中' : '发送'"
                  @click="submitAsk"
                >
                  <Send :size="16" :stroke-width="2.2" aria-hidden="true" />
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <ConfirmDialog
      :visible="clearOpen"
      title="清空全部会话？"
      message="将删除所有知识库对话记录，项目文件夹会保留为空。"
      confirm-text="清空"
      tone="danger"
      @confirm="onClearConfirm"
      @cancel="clearOpen = false"
    />

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

.kb-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 240px minmax(0, 1fr);
}

.kb-chat {
  display: flex;
  flex-direction: column;
  min-height: 0;
  min-width: 0;
  border-left: 1px solid var(--border-soft);
}

.kb-messages {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.kb-empty {
  margin: auto;
  font-size: 0.875rem;
  color: var(--text-3);
  text-align: center;
}

.kb-msg {
  display: flex;
  flex-direction: column;
  max-width: 100%;
}
.kb-msg[data-role='user'] {
  align-items: flex-end;
}
.kb-msg[data-role='assistant'] {
  align-items: stretch;
}

.kb-bubble-user {
  max-width: 85%;
  padding: 10px 14px;
  border-radius: var(--radius-lg);
  background: color-mix(in srgb, var(--brand-500) 16%, var(--bg-card-soft));
  color: var(--text-1);
  font-size: 0.8125rem;
  line-height: 1.55;
  white-space: pre-wrap;
  word-break: break-word;
}

.kb-bubble-assistant {
  padding: 12px 14px;
  border-radius: var(--radius-lg);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.kb-composer-dock {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 16px 16px;
  border-top: 1px solid var(--border-soft);
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

/* Doubao-style: large frosted box, borderless autosize textarea, brand send pill */
.kb-composer-box {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px 14px 10px;
  border-radius: var(--radius-lg);
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  box-shadow: var(--frost-edge), var(--shadow-card);
  transition: border-color 0.18s, box-shadow 0.18s;
}
.kb-composer-box:focus-within {
  border-color: color-mix(in srgb, var(--brand-500) 45%, transparent);
  box-shadow: var(--frost-edge), var(--shadow-focus);
}

.kb-input {
  width: 100%;
  min-width: 0;
  min-height: 40px;
  max-height: 160px;
  resize: none;
  padding: 2px 2px 0;
  border: 0;
  border-radius: 0;
  background: transparent;
  color: var(--text-1);
  font-size: 0.8125rem;
  line-height: 1.55;
  font-family: inherit;
}
.kb-input:focus {
  outline: none;
  box-shadow: none;
}
.kb-input:disabled {
  opacity: 0.65;
}
.kb-input::placeholder {
  color: var(--text-4);
}

.kb-composer-foot {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  min-height: 36px;
}

.kb-composer-hint {
  margin-right: auto;
  font-size: 0.71875rem;
  color: var(--text-3);
}

.kb-send {
  flex-shrink: 0;
  width: 36px;
  height: 36px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: var(--radius-pill);
  background: var(--brand-500);
  color: var(--text-on-accent);
  cursor: pointer;
  transition: opacity 0.15s, transform 0.15s, filter 0.15s;
}
.kb-send:hover:not(:disabled) {
  filter: brightness(1.06);
}
.kb-send:active:not(:disabled) {
  transform: scale(0.96);
}
.kb-send:disabled {
  opacity: 0.45;
  cursor: default;
}
.kb-send:focus-visible {
  outline: none;
  box-shadow: var(--shadow-focus);
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

.kb-clarify {
  margin: 0;
  padding: 8px 12px;
  border-radius: 8px;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-1);
  background: color-mix(in srgb, var(--c-orange) 16%, transparent);
  border: 1px solid color-mix(in srgb, var(--c-orange) 35%, transparent);
}

.kb-md {
  font-size: 0.8125rem;
  line-height: 1.65;
  color: var(--text-1);
  word-break: break-word;
}
.kb-md :deep(h1),
.kb-md :deep(h2),
.kb-md :deep(h3) {
  font-weight: 650;
  margin: 0.85em 0 0.35em;
}
.kb-md :deep(h1) { font-size: 16px; }
.kb-md :deep(h2) { font-size: 15px; }
.kb-md :deep(h3) { font-size: 14px; }
.kb-md :deep(p) { margin: 0.4em 0; font-size: 13px; line-height: 1.55; }
.kb-md :deep(ul),
.kb-md :deep(ol) { margin: 0.4em 0; padding-left: 1.35em; font-size: 13px; }
.kb-md :deep(code) {
  font-size: 12px;
  padding: 0.1em 0.35em;
  border-radius: 4px;
  background: var(--bg-card-soft);
}
.kb-md :deep(pre) {
  margin: 0.4em 0;
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
.kb-md :deep(button.kb-ref) {
  display: inline;
  padding: 0 2px;
  margin: 0;
  border: none;
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  color: var(--brand-600);
  border-radius: 4px;
  cursor: pointer;
  font: inherit;
  font-size: 0.92em;
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
  gap: 4px;
  padding-top: 4px;
  border-top: 1px solid var(--border-soft);
}

.kb-cite-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 10px;
}

.kb-cite-group-head {
  display: flex;
  flex-direction: column;
  gap: 2px;
  text-align: left;
  border: none;
  background: transparent;
  cursor: pointer;
  padding: 4px 0;
  color: inherit;
  font: inherit;
}
.kb-cite-group-head:hover .kb-cite-title {
  color: var(--brand-600);
}

.kb-cite {
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
}
.kb-cite[data-flash='1'] {
  outline: 2px solid color-mix(in srgb, var(--accent) 55%, transparent);
  border-radius: 8px;
}

.kb-cite-row {
  display: flex;
  gap: 4px;
  align-items: baseline;
  width: 100%;
  padding: 2px 4px;
}
.kb-cite-open {
  display: flex;
  gap: 8px;
  align-items: baseline;
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  cursor: pointer;
  text-align: left;
  padding: 4px 6px;
  color: inherit;
  font: inherit;
  border-radius: 6px;
}
.kb-cite-open:hover {
  background: color-mix(in srgb, var(--brand-50) 40%, transparent);
}

.kb-cite-idx {
  flex-shrink: 0;
  font-size: 0.75rem;
  font-weight: 700;
  color: var(--brand-500);
  font-variant-numeric: tabular-nums;
}

.kb-cite-heading-text {
  font-size: 0.8125rem;
  color: var(--text-1);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.kb-cite-chevron {
  border: none;
  background: transparent;
  color: var(--text-3);
  font-size: 11px;
  flex-shrink: 0;
  cursor: pointer;
  padding: 4px 6px;
  border-radius: 6px;
  line-height: 1;
}
.kb-cite-chevron:hover {
  background: color-mix(in srgb, var(--brand-50) 40%, transparent);
  color: var(--text-2);
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
  margin: 0;
  padding: 0 6px 8px 28px;
  font-size: 0.75rem;
  color: var(--text-2);
  line-height: 1.45;
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
