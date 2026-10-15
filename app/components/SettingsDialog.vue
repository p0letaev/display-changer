<script setup lang="ts">
import { onKeyStroke } from '@vueuse/core'
import type { Theme } from '~/types/display'

const emit = defineEmits<{ close: [] }>()
const store = useSettingsStore()
const s = computed(() => store.settings)

onKeyStroke('Escape', () => emit('close'))

const themes: { value: Theme, label: string }[] = [
  { value: 'system', label: 'Как в системе' },
  { value: 'light', label: 'Светлая' },
  { value: 'dark', label: 'Тёмная' },
]

function onTimeout(e: Event) {
  const value = Number((e.target as HTMLInputElement).value)
  if (Number.isFinite(value)) store.update({ confirmTimeoutSecs: value })
}
</script>

<template>
  <div class="backdrop" @pointerdown.self="emit('close')">
    <section class="dialog" role="dialog" aria-labelledby="settings-title">
      <header>
        <h2 id="settings-title">
          Настройки
        </h2>
        <button class="close" aria-label="Закрыть" @click="emit('close')">
          ×
        </button>
      </header>

      <div v-if="s" class="body">
        <h3>Применение</h3>
        <label class="row">
          <span>
            Время на подтверждение
            <small>Если не нажать «Оставить», вернутся прежние настройки</small>
          </span>
          <span class="number">
            <input
              type="number"
              min="5"
              max="120"
              step="5"
              :value="s.confirmTimeoutSecs"
              @change="onTimeout"
            > с
          </span>
        </label>
        <label class="row">
          <span>
            Подтверждать профили из трея и по хоткею
            <small>По умолчанию они применяются сразу — они сохранены с рабочих настроек</small>
          </span>
          <input type="checkbox" :checked="s.confirmQuickApply" @change="store.update({ confirmQuickApply: ($event.target as HTMLInputElement).checked })">
        </label>
        <label class="row">
          <span>
            Автоматически переключать профили
            <small>При подключении мониторов применяется профиль с пометкой «авто» для этого набора</small>
          </span>
          <input type="checkbox" :checked="s.autoSwitch" @change="store.update({ autoSwitch: ($event.target as HTMLInputElement).checked })">
        </label>

        <h3>Запуск и окно</h3>
        <label class="row">
          <span>Запускать при входе в систему <small>Стартует свёрнутым в трей</small></span>
          <input type="checkbox" :checked="s.autostart" @change="store.update({ autostart: ($event.target as HTMLInputElement).checked })">
        </label>
        <label class="row">
          <span>Запускать свёрнутым в трей</span>
          <input type="checkbox" :checked="s.startMinimized" @change="store.update({ startMinimized: ($event.target as HTMLInputElement).checked })">
        </label>
        <label class="row">
          <span>Закрытие окна сворачивает в трей <small>Выйти можно из меню трея</small></span>
          <input type="checkbox" :checked="s.closeToTray" @change="store.update({ closeToTray: ($event.target as HTMLInputElement).checked })">
        </label>

        <h3>Оформление</h3>
        <div class="row">
          <span>Тема</span>
          <div class="segmented">
            <button
              v-for="t in themes"
              :key="t.value"
              :class="{ on: s.theme === t.value }"
              @click="store.update({ theme: t.value })"
            >
              {{ t.label }}
            </button>
          </div>
        </div>

        <h3>Командная строка</h3>
        <p class="hint">
          <code>display-changer apply "Название"</code> применяет профиль без окна —
          на Wayland так назначают хоткеи в настройках композитора.
          <code>display-changer list</code> показывает профили.
        </p>
      </div>
    </section>
  </div>
</template>

<style scoped>
.backdrop {
  position: fixed;
  inset: 0;
  z-index: 40;
  display: grid;
  place-items: center;
  background: rgb(0 0 0 / 0.45);
}

.dialog {
  width: min(560px, calc(100vw - 32px));
  max-height: calc(100vh - 48px);
  overflow-y: auto;
  border-radius: 14px;
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: 0 20px 60px rgb(0 0 0 / 0.4);
}

header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border);
}

h2 {
  margin: 0;
  font-size: 16px;
}

.close {
  width: 30px;
  height: 30px;
  padding: 0;
  font-size: 18px;
  background: transparent;
  border-color: transparent;
}

.body {
  padding: 4px 20px 20px;
}

h3 {
  margin: 18px 0 6px;
  font-size: 12px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--text-muted);
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 9px 0;
  cursor: pointer;
}

.row small {
  display: block;
  font-size: 12px;
  color: var(--text-muted);
}

input[type='checkbox'] {
  flex: none;
  width: 18px;
  height: 18px;
  accent-color: var(--accent);
  cursor: pointer;
}

.number {
  flex: none;
  color: var(--text-muted);
}

.number input {
  width: 64px;
  font: inherit;
  color: var(--text);
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 5px 8px;
}

.segmented {
  display: flex;
  gap: 4px;
}

.segmented button.on {
  background: var(--accent-soft);
  border-color: var(--accent);
  color: var(--accent);
}

.hint {
  margin: 4px 0 0;
  font-size: 13px;
  color: var(--text-muted);
}

code {
  font: 12px var(--mono);
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--surface-2);
  color: var(--text);
}
</style>
