import type {
  ApplyOutcome,
  BackendInfo,
  DisplayState,
  Layout,
  Mode,
  OutputConfig,
  Position,
  ProfileEntry,
  ProfileWarning,
  Rotation,
} from '~/types/display'

/** Where the draft came from: the live desktop or a profile preview. */
type DraftSource =
  | { kind: 'current' }
  | { kind: 'profile', id: string, warnings: ProfileWarning[] }

interface PendingApply {
  token: number
  deadline: number
}

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value))

export const useDisplayStore = defineStore('display', () => {
  const api = useDisplayApi()
  const toasts = useToastStore()

  const info = ref<BackendInfo | null>(null)
  /** Last state reported by the backend. */
  const state = ref<DisplayState | null>(null)
  /** Layout being edited; applied only on demand. */
  const draft = ref<Layout | null>(null)
  const source = ref<DraftSource>({ kind: 'current' })
  const profiles = ref<ProfileEntry[]>([])
  const pending = ref<PendingApply | null>(null)
  const selected = ref<string | null>(null)
  const loading = ref(false)
  const busy = ref(false)
  const error = ref<string | null>(null)

  const caps = computed(() => info.value?.capabilities ?? null)
  const outputs = computed(() => draft.value?.outputs ?? [])
  const selectedOutput = computed(() => outputs.value.find(o => o.id.connector === selected.value) ?? null)
  const selectedMonitor = computed(() =>
    state.value?.monitors.find(m => m.id.connector === selected.value) ?? null)
  const dirty = computed(() =>
    !!state.value && !!draft.value && !sameLayout(draft.value, state.value.layout))
  const issues = computed(() => liveIssues(draft.value ?? { outputs: [] }))
  const issueMessages = computed(() => {
    const i = issues.value
    const list: string[] = []
    if (i.enabledCount === 0) list.push('Нет ни одного включённого монитора')
    if (caps.value?.primary && i.enabledCount > 0 && i.primaryCount !== 1) list.push('Выбери основной монитор')
    if (i.overlapping.size) list.push(`Перекрываются: ${[...i.overlapping].join(', ')}`)
    if (i.detached.size) list.push(`Оторваны от остальных: ${[...i.detached].join(', ')}`)
    return list
  })
  const canApply = computed(() =>
    dirty.value && issueMessages.value.length === 0 && !pending.value && !busy.value)
  const previewedProfile = computed(() => {
    const s = source.value
    return s.kind === 'profile' ? profiles.value.find(p => p.id === s.id) ?? null : null
  })

  // --- loading -------------------------------------------------------------

  let subscribed = false

  async function load() {
    if (!api.available) {
      error.value = 'Приложение запущено вне Tauri. Используй `npm run tauri dev`.'
      return
    }
    loading.value = true
    error.value = null
    try {
      const [backendInfo, displayState] = await Promise.all([api.backendInfo(), api.getState()])
      info.value = backendInfo
      setScaledLayout(backendInfo.capabilities.scaledLayout)
      setState(displayState)
      await reloadProfiles()
      if (!subscribed) {
        subscribed = true
        await subscribe()
      }
    }
    catch (e) {
      error.value = describeError(e)
    }
    finally {
      loading.value = false
    }
  }

  async function subscribe() {
    await api.onDisplayChanged((e) => {
      switch (e.reason) {
        case 'reverted':
          pending.value = null
          toasts.info('Изменения отменены, вернули прежние настройки')
          break
        case 'quick':
          toasts.success(`Профиль «${e.profileName}» применён`)
          break
        case 'auto':
          toasts.success(`Мониторы изменились — применён профиль «${e.profileName}»`)
          break
        case 'monitors':
          toasts.info('Набор подключённых мониторов изменился')
          break
        case 'reset':
          toasts.info('Сброс: вернули прежнюю раскладку')
          break
        case 'external':
          // Changed by the system or the CLI; keep the user's unsaved edits.
          if (dirty.value) {
            if (!e.pending) pending.value = null
            state.value = e.state
            reloadProfiles()
            return
          }
          break
      }
      // The backend is the authority on whether a change awaits confirmation:
      // a hotkey or reset supersedes a pending apply without a revert event.
      if (!e.pending) pending.value = null
      setState(e.state)
      reloadProfiles()
    })
    await api.onQuickFailed(e => toasts.error(`«${e.profileName}»: ${e.error}`))
  }

  async function reloadProfiles() {
    try {
      profiles.value = await api.listProfiles()
    }
    catch (e) {
      toasts.error(describeError(e))
    }
  }

  function setState(next: DisplayState) {
    state.value = next
    resetDraft()
  }

  function resetDraft() {
    if (!state.value) return
    draft.value = clone(state.value.layout)
    source.value = { kind: 'current' }
    const exists = draft.value.outputs.some(o => o.id.connector === selected.value)
    if (!exists) selected.value = draft.value.outputs.find(o => o.primary)?.id.connector ?? null
  }

  function select(connector: string | null) {
    selected.value = connector
  }

  // --- editing -------------------------------------------------------------

  function find(connector: string): OutputConfig | undefined {
    return draft.value?.outputs.find(o => o.id.connector === connector)
  }

  /** Live position update while dragging; no layout repair yet. */
  function moveOutput(connector: string, pos: Position) {
    const o = find(connector)
    if (o) o.position = pos
  }

  /**
   * Called on drop: the moved monitor slides to the nearest free edge, then
   * anything it left hanging is re-attached around it.
   */
  function finishMove(connector: string) {
    const o = find(connector)
    if (!o || !draft.value) return
    const others = draft.value.outputs.filter(x => x.enabled && x !== o).map(logicalRect)
    o.position = settle(logicalRect(o), others)
    repair(draft.value.outputs, [connector])
  }

  /**
   * Changes the monitor's size and shifts neighbours to its right and below
   * by the same amount, so the rest of the layout keeps its shape.
   */
  function resize(connector: string, mutate: (o: OutputConfig) => void) {
    const o = find(connector)
    if (!o || !draft.value) return
    const before = logicalRect(o)
    mutate(o)
    const after = logicalRect(o)
    const dw = after.width - before.width
    const dh = after.height - before.height
    for (const other of draft.value.outputs) {
      if (other === o || !other.enabled) continue
      if (other.position.x >= before.x + before.width) other.position.x += dw
      if (other.position.y >= before.y + before.height) other.position.y += dh
    }
    repair(draft.value.outputs, [connector])
  }

  function setMode(connector: string, mode: Mode) {
    resize(connector, (o) => { o.mode = { ...mode } })
  }

  function setScale(connector: string, scale: number) {
    resize(connector, (o) => { o.scale = scale })
  }

  function setRotation(connector: string, rotation: Rotation) {
    resize(connector, (o) => { o.rotation = rotation })
  }

  function setPrimary(connector: string) {
    for (const o of outputs.value) o.primary = o.id.connector === connector
  }

  /**
   * Makes `connector` repeat `source`, or (with null) a separate screen again.
   * A mirror takes the source's resolution when it has one.
   */
  function setMirror(connector: string, source: string | null) {
    const o = find(connector)
    if (!o || !draft.value) return
    if (source) {
      const src = find(source)
      if (!src) return
      if (o.primary) {
        o.primary = false
        src.primary = true
      }
      // Whatever mirrored this output now mirrors the new source.
      for (const m of outputs.value) if (m.mirrorOf === connector) m.mirrorOf = source
      o.mirrorOf = source
      o.enabled = true
      o.position = { ...src.position }
      const monitor = state.value?.monitors.find(m => m.id.connector === connector)
      const same = monitor?.modes.find(m => m.width === src.mode.width && m.height === src.mode.height)
      if (same) o.mode = { ...same }
    }
    else {
      o.mirrorOf = null
      const box = boundingBox(outputs.value.filter(x => isPlaced(x) && x !== o).map(logicalRect))
      o.position = box ? { x: box.x + box.width, y: box.y } : { x: 0, y: 0 }
    }
    repair(draft.value.outputs, source ? [] : [connector])
  }

  function setEnabled(connector: string, enabled: boolean) {
    const o = find(connector)
    if (!o || !draft.value || o.enabled === enabled) return
    if (enabled) {
      const box = boundingBox(outputs.value.filter(isPlaced).map(logicalRect))
      o.position = box ? { x: box.x + box.width, y: box.y } : { x: 0, y: 0 }
      o.enabled = true
      if (!outputs.value.some(x => x.enabled && x.primary)) o.primary = true
    }
    else {
      if (isPlaced(o) && issues.value.enabledCount <= 1) return
      o.enabled = false
      // Mirrors of a switched-off screen become screens of their own.
      for (const m of outputs.value) {
        if (m.mirrorOf === connector) {
          m.mirrorOf = null
          const box = boundingBox(outputs.value.filter(x => isPlaced(x) && x !== m).map(logicalRect))
          m.position = box ? { x: box.x + box.width, y: box.y } : { x: 0, y: 0 }
        }
      }
      o.mirrorOf = null
      if (o.primary) {
        o.primary = false
        const next = outputs.value.find(isPlaced)
        if (next) next.primary = true
      }
    }
    repair(draft.value.outputs)
  }

  // --- applying ------------------------------------------------------------

  async function run<T>(action: () => Promise<T>): Promise<T | undefined> {
    busy.value = true
    try {
      return await action()
    }
    catch (e) {
      toasts.error(describeError(e))
      return undefined
    }
    finally {
      busy.value = false
    }
  }

  function afterApply(outcome: ApplyOutcome) {
    setState(outcome.state)
    pending.value = { token: outcome.token, deadline: Date.now() + outcome.timeoutSecs * 1000 }
    reloadProfiles()
  }

  async function applyDraft() {
    if (!draft.value) return
    const layout = clone(draft.value)
    const outcome = await run(() => api.applyLayout(layout))
    if (outcome) afterApply(outcome)
  }

  async function confirmApply() {
    const p = pending.value
    if (!p) return
    const kept = await run(() => api.confirmApply(p.token))
    pending.value = null
    if (kept) toasts.success('Настройки сохранены')
  }

  /** The backend emits `display://changed` on revert; the listener updates state. */
  async function revertApply() {
    const p = pending.value
    if (p) await run(() => api.revertApply(p.token))
  }

  // --- profiles ------------------------------------------------------------

  async function previewProfile(id: string) {
    const resolved = await run(() => api.previewProfile(id))
    if (!resolved) return
    draft.value = resolved.layout
    source.value = { kind: 'profile', id, warnings: resolved.warnings }
  }

  async function applyProfile(id: string) {
    const outcome = await run(() => api.applyProfile(id))
    if (outcome) afterApply(outcome)
  }

  async function createProfile(name: string) {
    if (!draft.value) return
    const layout = clone(draft.value)
    const profile = await run(() => api.createProfile(name, layout))
    if (!profile) return
    await reloadProfiles()
    source.value = { kind: 'profile', id: profile.id, warnings: [] }
    toasts.success(`Профиль «${name}» сохранён`)
  }

  async function overwriteProfile(id: string) {
    if (!draft.value) return
    const layout = clone(draft.value)
    const profile = await run(() => api.updateProfile(id, { layout }))
    if (!profile) return
    await reloadProfiles()
    toasts.success(`Профиль «${profile.name}» обновлён`)
  }

  async function renameProfile(id: string, name: string) {
    if (await run(() => api.updateProfile(id, { name }))) await reloadProfiles()
  }

  async function deleteProfile(id: string) {
    const name = profiles.value.find(p => p.id === id)?.name
    const ok = await run(async () => {
      await api.deleteProfile(id)
      return true
    })
    if (!ok) return
    if (source.value.kind === 'profile' && source.value.id === id) resetDraft()
    await reloadProfiles()
    toasts.info(`Профиль «${name}» удалён`)
  }

  /** Returns an error message, or null on success. */
  async function setHotkey(id: string, hotkey: string | null): Promise<string | null> {
    try {
      await api.setProfileHotkey(id, hotkey)
      await reloadProfiles()
      return null
    }
    catch (e) {
      return describeError(e)
    }
  }

  async function setAutoApply(id: string, autoApply: boolean) {
    if (await run(() => api.updateProfile(id, { autoApply }))) await reloadProfiles()
  }

  async function exportProfiles() {
    const path = await run(() => api.exportProfiles())
    if (path) toasts.success(`Профили сохранены в ${path}`)
  }

  async function importProfiles() {
    const count = await run(() => api.importProfiles())
    if (count) {
      await reloadProfiles()
      toasts.success(`Импортировано профилей: ${count}`)
    }
  }

  async function resetLayout() {
    await run(() => api.resetLayout())
  }

  async function identify() {
    await run(() => api.identifyMonitors())
  }

  async function pauseHotkeys(paused: boolean) {
    try {
      await api.setHotkeysPaused(paused)
    }
    catch (e) {
      toasts.error(describeError(e))
    }
  }

  return {
    info, state, draft, source, profiles, pending, selected, loading, busy, error,
    caps, outputs, selectedOutput, selectedMonitor, dirty, issues, issueMessages, canApply,
    previewedProfile,
    load, resetDraft, select,
    moveOutput, finishMove, setMode, setScale, setRotation, setPrimary, setEnabled, setMirror, resetLayout,
    applyDraft, confirmApply, revertApply,
    previewProfile, applyProfile, createProfile, overwriteProfile, renameProfile, deleteProfile,
    setHotkey, pauseHotkeys, setAutoApply, exportProfiles, importProfiles, identify,
  }
})
