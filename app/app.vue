<script setup lang="ts">
const store = useDisplayStore()
const settings = useSettingsStore()
const showSettings = ref(false)

// Identify windows get their number injected by Rust before the page loads.
const identify = (window as { __IDENTIFY__?: { number: number, name: string } }).__IDENTIFY__

onMounted(() => {
  if (identify) {
    // Let the rounded badge float over the desktop where transparency works.
    document.documentElement.style.background = 'transparent'
    document.body.style.background = 'transparent'
    return
  }
  settings.load()
  store.load()
})

const sourceLabel = computed(() => {
  if (store.previewedProfile) return `Профиль «${store.previewedProfile.name}»`
  return store.dirty ? 'Изменённая раскладка' : 'Текущие настройки'
})

const warnings = computed(() =>
  store.source.kind === 'profile' ? store.source.warnings.map(describeWarning) : [])

/** Previewed profile differs from what is saved in it (edited after loading). */
const canUpdateProfile = computed(() => {
  const p = store.previewedProfile
  return !!p && !!store.draft && !sameLayout(store.draft, p.layout) && store.issueMessages.length === 0
})
</script>

<template>
  <div v-if="identify" class="identify">
    <strong>{{ identify.number }}</strong>
    <span>{{ identify.name }}</span>
  </div>

  <div v-else class="app">
    <header class="topbar">
      <h1>Display Changer</h1>
      <span v-if="store.info" class="backend" :class="{ mock: store.info.name === 'mock' }">
        {{ store.info.name === 'mock' ? 'демо-данные' : store.info.name }}
      </span>
      <div class="spacer" />
      <button
        :disabled="!store.state || store.busy"
        title="Вернуть раскладку, которая была до последнего изменения (Пробел + Alt + →)"
        @click="store.resetLayout()"
      >
        Сброс
      </button>
      <button :disabled="!store.state" title="Показать номер на каждом мониторе" @click="store.identify()">
        Определить
      </button>
      <button :disabled="store.loading || !!store.pending" @click="store.load()">
        Обновить
      </button>
      <button class="icon-btn" title="Настройки" aria-label="Настройки" @click="showSettings = true">
        ⚙
      </button>
    </header>

    <p v-if="store.error" class="error">
      {{ store.error }}
    </p>

    <main v-else-if="store.state && store.draft" class="workspace">
      <aside class="profiles">
        <ProfileList />
      </aside>

      <section class="stage">
        <ConfirmBar />
        <div class="canvas-wrap">
          <LayoutCanvas
            :outputs="store.outputs"
            :selected="store.selected"
            :overlapping="store.issues.overlapping"
            :detached="store.issues.detached"
            :editable="!store.pending"
            @select="store.select"
            @move="store.moveOutput"
            @drop="store.finishMove"
          />
        </div>

        <footer class="editor-bar">
          <div class="status">
            <span class="source" :class="{ changed: store.dirty }">{{ sourceLabel }}</span>
            <ul v-if="store.issueMessages.length" class="issues">
              <li v-for="m in store.issueMessages" :key="m">
                {{ m }}
              </li>
            </ul>
            <ul v-if="warnings.length" class="warnings">
              <li v-for="w in warnings" :key="w">
                {{ w }}
              </li>
            </ul>
          </div>
          <div class="buttons">
            <button
              v-if="store.dirty || store.previewedProfile"
              :disabled="!!store.pending"
              @click="store.resetDraft()"
            >
              Сбросить
            </button>
            <button
              v-if="canUpdateProfile"
              :disabled="store.busy"
              @click="store.overwriteProfile(store.previewedProfile!.id)"
            >
              Обновить профиль
            </button>
            <button class="primary" :disabled="!store.canApply" @click="store.applyDraft()">
              Применить
            </button>
          </div>
        </footer>
      </section>

      <aside class="sidebar">
        <MonitorPanel
          v-if="store.selectedOutput"
          :output="store.selectedOutput"
          :monitor="store.selectedMonitor"
        />
        <p v-else class="hint">
          Выбери монитор на схеме, чтобы настроить его. Мониторы можно перетаскивать — они прилипают к краям друг друга.
        </p>
      </aside>
    </main>

    <SettingsDialog v-if="showSettings" @close="showSettings = false" />
    <ToastHost />
  </div>
</template>

<style scoped>
.identify {
  height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  border-radius: 28px;
  background: rgb(17 20 27 / 0.88);
  border: 2px solid var(--accent);
  color: #fff;
  user-select: none;
}

.identify strong {
  font-size: 160px;
  line-height: 1;
  font-weight: 700;
}

.identify span {
  margin-top: 8px;
  font: 18px var(--mono);
  opacity: 0.75;
}

.icon-btn {
  width: 34px;
  padding: 6px 0;
  font-size: 16px;
}

.app {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.topbar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
  border-bottom: 1px solid var(--border);
  background: var(--surface);
}

h1 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}

.backend {
  font: 12px var(--mono);
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--surface-2);
  color: var(--text-muted);
}

.backend.mock {
  color: var(--primary-badge);
}

.spacer {
  flex: 1;
}

.workspace {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 270px 1fr 300px;
}

.profiles {
  min-height: 0;
  border-right: 1px solid var(--border);
  background: var(--surface);
}

.stage {
  position: relative;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.canvas-wrap {
  flex: 1;
  min-height: 0;
  padding: 16px;
}

.editor-bar {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 16px;
  padding: 12px 16px;
  border-top: 1px solid var(--border);
}

.status {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  font-size: 13px;
}

.source {
  color: var(--text-muted);
}

.source.changed {
  color: var(--text);
}

.issues,
.warnings {
  margin: 0;
  padding: 0;
  list-style: none;
  font-size: 12px;
}

.issues {
  color: var(--danger);
}

.warnings {
  color: var(--primary-badge);
}

.buttons {
  display: flex;
  gap: 8px;
  flex: none;
}

.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
  padding-inline: 18px;
}

.sidebar {
  padding: 18px;
  border-left: 1px solid var(--border);
  background: var(--surface);
  overflow-y: auto;
}

.hint {
  color: var(--text-muted);
  font-size: 13px;
}

.error {
  margin: 20px;
  padding: 12px 16px;
  border-radius: var(--radius);
  border: 1px solid var(--danger);
  color: var(--danger);
  white-space: pre-line;
}
</style>
