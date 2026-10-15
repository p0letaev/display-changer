<script setup lang="ts">
const props = defineProps<{
  profileId: string
  hotkey: string | null
  /** Registration error reported by the backend for the saved hotkey. */
  registrationError: string | null
}>()

const store = useDisplayStore()
const button = ref<HTMLButtonElement | null>(null)
const recording = ref(false)
const held = ref<string[]>([])
const message = ref<string | null>(null)

async function start() {
  if (recording.value) return
  message.value = null
  held.value = []
  recording.value = true
  // Otherwise pressing an already bound combination would switch profiles.
  await store.pauseHotkeys(true)
}

async function stop() {
  if (!recording.value) return
  recording.value = false
  held.value = []
  await store.pauseHotkeys(false)
}

async function save(hotkey: string | null) {
  recording.value = false
  held.value = []
  message.value = await store.setHotkey(props.profileId, hotkey)
  if (message.value) await store.pauseHotkeys(false)
  button.value?.blur()
}

function onKeydown(e: KeyboardEvent) {
  if (!recording.value) return
  e.preventDefault()
  e.stopPropagation()
  const plain = !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey
  if (plain && e.code === 'Escape') return stop()
  if (plain && (e.code === 'Backspace' || e.code === 'Delete')) return save(null)

  const result = recordKey(e)
  if (result.type === 'pending') held.value = result.modifiers
  else if (result.type === 'invalid') message.value = result.reason
  else save(result.accelerator)
}

function onKeyup(e: KeyboardEvent) {
  if (!recording.value) return
  held.value = [
    e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Super',
  ].filter(Boolean) as string[]
}

// Messages from the last attempt fade out; registration errors stay visible.
let clearTimer: ReturnType<typeof setTimeout> | undefined
watch(message, (m) => {
  clearTimeout(clearTimer)
  if (m) clearTimer = setTimeout(() => { message.value = null }, 6000)
})
onUnmounted(() => clearTimeout(clearTimer))

const error = computed(() => message.value ?? props.registrationError)
</script>

<template>
  <div class="hotkey">
    <button
      ref="button"
      class="field"
      :class="{ recording, empty: !hotkey, failed: !!registrationError && !recording }"
      :title="recording ? 'Esc — отмена, Backspace — убрать хоткей' : 'Назначить хоткей'"
      @click.stop="start"
      @keydown="onKeydown"
      @keyup="onKeyup"
      @blur="stop"
    >
      <template v-if="recording">
        <template v-if="held.length">
          <kbd v-for="k in held" :key="k">{{ k }}</kbd><span class="plus">+ …</span>
        </template>
        <span v-else class="hint">Нажми сочетание…</span>
      </template>
      <template v-else-if="hotkey">
        <kbd v-for="(k, i) in acceleratorKeys(hotkey)" :key="i">{{ k }}</kbd>
      </template>
      <span v-else class="hint">+ хоткей</span>
    </button>
    <button
      v-if="hotkey && !recording"
      class="clear"
      title="Убрать хоткей"
      @click.stop="save(null)"
    >
      ×
    </button>
    <p v-if="error" class="error">
      {{ error }}
    </p>
  </div>
</template>

<style scoped>
.hotkey {
  display: flex;
  flex-wrap: wrap;
  row-gap: 0;
  align-items: center;
  gap: 4px;
  min-width: 0;
}

.field {
  display: inline-flex;
  flex-wrap: nowrap;
  white-space: nowrap;
  align-items: center;
  gap: 3px;
  padding: 3px 6px;
  min-height: 26px;
  background: transparent;
  border-style: dashed;
}

.field:not(.empty) {
  border-style: solid;
}

.field.recording {
  border-color: var(--accent);
  border-style: solid;
  background: var(--accent-soft);
}

.field.failed {
  border-color: var(--danger);
}

kbd {
  font: 11px var(--mono);
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-bottom-width: 2px;
}

.hint,
.plus {
  font-size: 12px;
  color: var(--text-muted);
}

.clear {
  padding: 0 6px;
  line-height: 22px;
  background: transparent;
  border-color: transparent;
  color: var(--text-muted);
}

.error {
  flex-basis: 100%;
  margin: 2px 0 0;
  font-size: 12px;
  color: var(--danger);
}
</style>
