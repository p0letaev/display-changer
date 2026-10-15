<script setup lang="ts">
import type { Mode, MonitorInfo, OutputConfig, Rotation } from '~/types/display'

const props = defineProps<{
  output: OutputConfig
  monitor: MonitorInfo | null
}>()

const store = useDisplayStore()
const caps = computed(() => store.caps)
const connector = computed(() => props.output.id.connector)

const diagonal = computed(() => {
  const size = props.monitor?.physicalSize
  if (!size) return null
  return `${(Math.hypot(size.widthMm, size.heightMm) / 25.4).toFixed(1)}″`
})

const sameRes = (a: Mode, b: Mode) => a.width === b.width && a.height === b.height
const resKey = (m: Mode) => `${m.width}x${m.height}`

const resolutions = computed(() => {
  const seen = new Map<string, Mode>()
  for (const m of props.monitor?.modes ?? []) if (!seen.has(resKey(m))) seen.set(resKey(m), m)
  return [...seen.values()].sort((a, b) => b.width * b.height - a.width * a.height)
})

const refreshRates = computed(() =>
  (props.monitor?.modes ?? [])
    .filter(m => sameRes(m, props.output.mode))
    .sort((a, b) => b.refreshMhz - a.refreshMhz))

function onResolution(key: string) {
  const modes = (props.monitor?.modes ?? []).filter(m => resKey(m) === key)
  // Keep the refresh rate if the new resolution supports it, else take the highest.
  const mode = modes.find(m => m.refreshMhz === props.output.mode.refreshMhz)
    ?? modes.sort((a, b) => b.refreshMhz - a.refreshMhz)[0]
  if (mode) store.setMode(connector.value, mode)
}

function onRefresh(mhz: string) {
  const mode = refreshRates.value.find(m => m.refreshMhz === Number(mhz))
  if (mode) store.setMode(connector.value, mode)
}

const scales = computed(() => {
  const c = caps.value
  const fromMonitor = props.monitor?.scales ?? []
  let list = fromMonitor.length
    ? [...fromMonitor]
    : !c?.scale ? [1] : c.fractionalScale ? [1, 1.25, 1.5, 1.75, 2, 2.5, 3] : [1, 2, 3]
  if (!list.includes(props.output.scale)) list = [...list, props.output.scale].sort((a, b) => a - b)
  return list
})

const rotations: { value: Rotation, label: string, icon: string }[] = [
  { value: 'normal', label: rotationLabels.normal, icon: '▭' },
  { value: 'left', label: rotationLabels.left, icon: '↺' },
  { value: 'inverted', label: rotationLabels.inverted, icon: '⟲' },
  { value: 'right', label: rotationLabels.right, icon: '↻' },
]

const isPreferred = (m: Mode) => !!props.monitor?.preferredMode && sameRes(props.monitor.preferredMode, m)
const canDisable = computed(() =>
  !!caps.value?.disable && (!props.output.enabled || store.issues.enabledCount > 1))
const rect = computed(() => logicalRect(props.output))
</script>

<template>
  <aside class="panel">
    <header>
      <h2>{{ monitor?.name ?? connector }}</h2>
      <p class="meta">
        <span class="badge mono">{{ connector }}</span>
        <span v-if="output.primary && output.enabled" class="badge primary">★ Основной</span>
        <span v-if="monitor?.builtin" class="badge">Встроенный</span>
        <span v-if="diagonal" class="badge">{{ diagonal }}</span>
      </p>
    </header>

    <div class="row">
      <label class="switch">
        <input
          type="checkbox"
          :checked="output.enabled"
          :disabled="!canDisable"
          @change="store.setEnabled(connector, ($event.target as HTMLInputElement).checked)"
        >
        <span class="track"><span class="thumb" /></span>
        Включён
      </label>
      <button
        v-if="caps?.primary"
        class="ghost"
        :disabled="!output.enabled || output.primary"
        @click="store.setPrimary(connector)"
      >
        {{ output.primary ? '★ Основной' : 'Сделать основным' }}
      </button>
    </div>

    <fieldset :disabled="!output.enabled">
      <label class="field">
        <span>Разрешение</span>
        <select :value="resKey(output.mode)" @change="onResolution(($event.target as HTMLSelectElement).value)">
          <option v-for="m in resolutions" :key="resKey(m)" :value="resKey(m)">
            {{ formatResolution(m) }}{{ isPreferred(m) ? ' (рекомендуется)' : '' }}
          </option>
        </select>
      </label>

      <label class="field">
        <span>Частота обновления</span>
        <select :value="output.mode.refreshMhz" @change="onRefresh(($event.target as HTMLSelectElement).value)">
          <option v-for="m in refreshRates" :key="m.refreshMhz" :value="m.refreshMhz">
            {{ formatRefresh(m.refreshMhz) }}
          </option>
        </select>
      </label>

      <div class="field">
        <span>Масштаб</span>
        <div class="chips">
          <button
            v-for="s in scales"
            :key="s"
            :class="{ on: output.scale === s }"
            :disabled="!caps?.scale && s !== 1"
            @click="store.setScale(connector, s)"
          >
            {{ formatScale(s) }}
          </button>
        </div>
      </div>

      <div class="field">
        <span>Поворот</span>
        <div class="chips">
          <button
            v-for="r in rotations"
            :key="r.value"
            :class="{ on: output.rotation === r.value }"
            :title="r.label"
            :disabled="(!caps?.rotation || !monitor?.rotations.includes(r.value)) && r.value !== 'normal'"
            @click="store.setRotation(connector, r.value)"
          >
            <span class="icon">{{ r.icon }}</span>
          </button>
        </div>
      </div>
    </fieldset>

    <dl>
      <dt>Позиция</dt>
      <dd>{{ output.position.x }}, {{ output.position.y }}</dd>
      <dt>Логический размер</dt>
      <dd>{{ rect.width }} × {{ rect.height }}</dd>
    </dl>

    <p v-if="output.id.vendor" class="id">
      {{ output.id.vendor }} · 0x{{ output.id.product.toString(16).padStart(4, '0') }}
      <template v-if="output.id.serial">
        · {{ output.id.serial }}
      </template>
    </p>
  </aside>
</template>

<style scoped>
.panel {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

h2 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
}

.meta {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin: 8px 0 0;
}

.badge {
  font-size: 12px;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--surface-2);
  color: var(--text-muted);
}

.badge.mono {
  font-family: var(--mono);
}

.badge.primary {
  color: var(--primary-badge);
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

fieldset {
  display: flex;
  flex-direction: column;
  gap: 14px;
  margin: 0;
  padding: 0;
  border: 0;
  min-width: 0;
}

fieldset:disabled {
  opacity: 0.45;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.field > span {
  color: var(--text-muted);
  font-size: 12px;
}

select {
  appearance: none;
  font: inherit;
  color: var(--text);
  background-color: var(--surface-2);
  background-image: linear-gradient(45deg, transparent 50%, var(--text-muted) 50%),
    linear-gradient(135deg, var(--text-muted) 50%, transparent 50%);
  background-position: calc(100% - 16px) 50%, calc(100% - 11px) 50%;
  background-size: 5px 5px;
  background-repeat: no-repeat;
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 7px 28px 7px 10px;
  cursor: pointer;
}

select option {
  background: var(--surface);
  color: var(--text);
}

select:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}

.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.chips button {
  padding: 5px 10px;
  font-variant-numeric: tabular-nums;
}

.chips button.on {
  background: var(--accent-soft);
  border-color: var(--accent);
  color: var(--accent);
}

.icon {
  display: inline-block;
  width: 18px;
  text-align: center;
}

.ghost {
  background: transparent;
}

.switch {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  cursor: pointer;
}

.switch input {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}

.track {
  position: relative;
  width: 36px;
  height: 20px;
  border-radius: 999px;
  background: var(--surface-2);
  border: 1px solid var(--border);
  transition: background 0.15s;
}

.thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--text-muted);
  transition: transform 0.15s, background 0.15s;
}

.switch input:checked + .track {
  background: var(--accent);
  border-color: var(--accent);
}

.switch input:checked + .track .thumb {
  transform: translateX(16px);
  background: #fff;
}

.switch input:disabled + .track {
  opacity: 0.5;
}

.switch input:focus-visible + .track {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

dl {
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 6px 16px;
  margin: 0;
  font-size: 13px;
}

dt {
  color: var(--text-muted);
}

dd {
  margin: 0;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.id {
  margin: 0;
  font: 12px var(--mono);
  color: var(--text-muted);
}
</style>
