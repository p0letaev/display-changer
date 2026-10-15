<script setup lang="ts">
import { onClickOutside } from '@vueuse/core'

const store = useDisplayStore()
const menuOpen = ref(false)
const menu = ref<HTMLElement | null>(null)
onClickOutside(menu, () => { menuOpen.value = false })

const creating = ref(false)
const name = ref('')
const input = ref<HTMLInputElement | null>(null)

async function startCreate() {
  name.value = ''
  creating.value = true
  await nextTick()
  input.value?.focus()
}

async function create() {
  const value = name.value.trim()
  if (!value) return
  creating.value = false
  await store.createProfile(value)
}
</script>

<template>
  <nav class="profiles">
    <header>
      <h2>Профили</h2>
      <div ref="menu" class="header-actions">
        <button class="add" title="Импорт и экспорт" @click="menuOpen = !menuOpen">
          ⋯
        </button>
        <button class="add" title="Сохранить раскладку из редактора как профиль" @click="startCreate">
          +
        </button>
        <div v-if="menuOpen" class="menu">
          <button @click="store.importProfiles(); menuOpen = false">
            Импортировать из файла…
          </button>
          <button :disabled="!store.profiles.length" @click="store.exportProfiles(); menuOpen = false">
            Экспортировать в файл…
          </button>
        </div>
      </div>
    </header>

    <form v-if="creating" class="create" @submit.prevent="create">
      <input
        ref="input"
        v-model="name"
        placeholder="Название профиля"
        maxlength="60"
        @keydown.esc="creating = false"
      >
      <div class="create-actions">
        <button type="button" @click="creating = false">
          Отмена
        </button>
        <button type="submit" class="primary" :disabled="!name.trim()">
          Сохранить
        </button>
      </div>
      <p class="hint">
        Сохранится раскладка, которая сейчас в редакторе.
      </p>
    </form>

    <div class="list">
      <ProfileCard v-for="p in store.profiles" :key="p.id" :profile="p" />
      <p v-if="!store.profiles.length && !creating" class="empty">
        Профилей пока нет. Настрой мониторы и нажми «+», чтобы сохранить раскладку.
      </p>
    </div>

    <footer class="hint">
      Клик по профилю — предпросмотр в редакторе, ▶ — применить.
      Двойной клик по названию — переименовать.
    </footer>
  </nav>
</template>

<style scoped>
.profiles {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 16px 8px;
}

h2 {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--text-muted);
}

.header-actions {
  position: relative;
  display: flex;
  gap: 4px;
}

.menu {
  position: absolute;
  right: 0;
  top: 34px;
  z-index: 10;
  display: flex;
  flex-direction: column;
  min-width: 220px;
  padding: 4px;
  border-radius: var(--radius);
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: 0 10px 30px rgb(0 0 0 / 0.3);
}

.menu button {
  text-align: left;
  border: 0;
  background: transparent;
  padding: 7px 10px;
}

.menu button:hover:not(:disabled) {
  background: var(--surface-2);
}

.add {
  width: 28px;
  height: 28px;
  padding: 0;
  font-size: 18px;
  line-height: 1;
}

.create {
  margin: 0 12px 8px;
  padding: 10px;
  border-radius: var(--radius);
  background: var(--surface-2);
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.create input {
  font: inherit;
  color: var(--text);
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 7px 10px;
}

.create input:focus {
  outline: none;
  border-color: var(--accent);
}

.create-actions {
  display: flex;
  justify-content: flex-end;
  gap: 6px;
}

.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}

.list {
  flex: 1;
  overflow-y: auto;
  padding: 0 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.empty,
.hint {
  font-size: 12px;
  color: var(--text-muted);
  margin: 0;
}

.empty {
  padding: 8px;
}

footer {
  padding: 12px 16px;
  border-top: 1px solid var(--border);
}
</style>
