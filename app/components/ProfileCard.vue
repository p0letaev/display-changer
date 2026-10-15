<script setup lang="ts">
import { onClickOutside } from '@vueuse/core'
import type { ProfileEntry } from '~/types/display'

const props = defineProps<{ profile: ProfileEntry }>()

const store = useDisplayStore()
const previewing = computed(() => store.previewedProfile?.id === props.profile.id)

const menuOpen = ref(false)
const menu = ref<HTMLElement | null>(null)
onClickOutside(menu, () => { menuOpen.value = false })

const renaming = ref(false)
const nameInput = ref<HTMLInputElement | null>(null)
const draftName = ref('')

async function startRename() {
  menuOpen.value = false
  draftName.value = props.profile.name
  renaming.value = true
  await nextTick()
  nameInput.value?.select()
}

async function finishRename() {
  if (!renaming.value) return
  renaming.value = false
  const name = draftName.value.trim()
  if (name && name !== props.profile.name) await store.renameProfile(props.profile.id, name)
}

const confirmingDelete = ref(false)

async function remove() {
  if (!confirmingDelete.value) {
    confirmingDelete.value = true
    return
  }
  menuOpen.value = false
  await store.deleteProfile(props.profile.id)
}

watch(menuOpen, (open) => { if (!open) confirmingDelete.value = false })

function overwrite() {
  menuOpen.value = false
  store.overwriteProfile(props.profile.id)
}
</script>

<template>
  <article
    class="card"
    :class="{ previewing, active: profile.active }"
    tabindex="0"
    @click="store.previewProfile(profile.id)"
    @keydown.enter.self="store.previewProfile(profile.id)"
  >
    <div class="head">
      <div class="preview">
        <MiniLayout :layout="profile.layout" />
      </div>

      <div class="title">
        <input
          v-if="renaming"
          ref="nameInput"
          v-model="draftName"
          class="rename"
          maxlength="60"
          @click.stop
          @keydown.enter="finishRename"
          @keydown.esc="renaming = false"
          @blur="finishRename"
        >
        <h3 v-else :title="profile.name" @dblclick.stop="startRename">
          {{ profile.name }}
        </h3>
        <span v-if="profile.autoApply" class="auto" title="Применяется автоматически при подключении этих мониторов">авто</span>
        <span v-if="profile.active" class="dot" title="Сейчас применён" />
      </div>

      <div class="actions">
        <button
          class="apply"
          :disabled="profile.active || !!store.pending || store.busy"
          :title="profile.active ? 'Уже применён' : 'Применить профиль'"
          @click.stop="store.applyProfile(profile.id)"
        >
          ▶
        </button>
        <div ref="menu" class="menu-wrap">
          <button class="more" title="Ещё" @click.stop="menuOpen = !menuOpen">
            ⋯
          </button>
          <div v-if="menuOpen" class="menu" @click.stop>
            <button
              :title="'Применять, когда подключены именно эти мониторы (включите автопереключение в настройках)'"
              @click="store.setAutoApply(profile.id, !profile.autoApply); menuOpen = false"
            >
              {{ profile.autoApply ? '✓ ' : '' }}Применять автоматически
            </button>
            <button @click="overwrite">
              Сохранить раскладку из редактора
            </button>
            <button @click="startRename">
              Переименовать
            </button>
            <button class="danger" @click="remove">
              {{ confirmingDelete ? 'Точно удалить?' : 'Удалить' }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <HotkeyInput
      class="hotkey-row"
      :profile-id="profile.id"
      :hotkey="profile.hotkey"
      :registration-error="profile.hotkeyError"
    />
  </article>
</template>

<style scoped>
.card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
  border-radius: var(--radius);
  border: 1px solid transparent;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s;
}

.card:hover {
  background: var(--surface-2);
}

.card:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

.card.previewing {
  background: var(--accent-soft);
  border-color: var(--accent);
}

.head {
  display: grid;
  grid-template-columns: 48px 1fr auto;
  gap: 10px;
  align-items: center;
}

.preview {
  height: 30px;
}

.hotkey-row {
  padding-left: 0;
}

.title {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rename {
  width: 100%;
  font: inherit;
  font-weight: 600;
  color: var(--text);
  background: var(--bg);
  border: 1px solid var(--accent);
  border-radius: 6px;
  padding: 1px 6px;
}

.auto {
  flex: none;
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 999px;
  background: var(--accent-soft);
  color: var(--accent);
}

.dot {
  flex: none;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #3ecf8e;
  box-shadow: 0 0 0 3px rgb(62 207 142 / 0.2);
}

.actions {
  display: flex;
  gap: 4px;
}

.actions button {
  width: 28px;
  height: 28px;
  padding: 0;
  display: grid;
  place-items: center;
}

.apply:not(:disabled) {
  color: var(--accent);
}

.menu-wrap {
  position: relative;
}

.menu {
  position: absolute;
  right: 0;
  top: 32px;
  z-index: 10;
  display: flex;
  flex-direction: column;
  min-width: 230px;
  padding: 4px;
  border-radius: var(--radius);
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: 0 10px 30px rgb(0 0 0 / 0.3);
}

.menu button {
  width: auto;
  height: auto;
  display: block;
  text-align: left;
  padding: 7px 10px;
  border: 0;
  background: transparent;
}

.menu button:hover {
  background: var(--surface-2);
}

.menu .danger {
  color: var(--danger);
}
</style>
