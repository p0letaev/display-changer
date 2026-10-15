import type { Settings, Theme } from '~/types/display'

function applyTheme(theme: Theme) {
  const root = document.documentElement
  if (theme === 'system') delete root.dataset.theme
  else root.dataset.theme = theme
}

export const useSettingsStore = defineStore('settings', () => {
  const api = useDisplayApi()
  const toasts = useToastStore()
  const settings = ref<Settings | null>(null)
  const saving = ref(false)

  async function load() {
    if (!api.available) return
    try {
      settings.value = await api.getSettings()
      applyTheme(settings.value.theme)
    }
    catch (e) {
      toasts.error(describeError(e))
    }
  }

  async function update(patch: Partial<Settings>) {
    if (!settings.value) return
    const next = { ...settings.value, ...patch }
    saving.value = true
    try {
      settings.value = await api.updateSettings(next)
      applyTheme(settings.value.theme)
    }
    catch (e) {
      toasts.error(describeError(e))
    }
    finally {
      saving.value = false
    }
  }

  return { settings, saving, load, update }
})
