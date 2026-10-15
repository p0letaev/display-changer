export interface Toast {
  id: number
  kind: 'info' | 'success' | 'error'
  text: string
}

export const useToastStore = defineStore('toasts', () => {
  const items = ref<Toast[]>([])
  let counter = 0

  function push(kind: Toast['kind'], text: string, timeoutMs = kind === 'error' ? 7000 : 3500) {
    const id = ++counter
    items.value.push({ id, kind, text })
    setTimeout(() => dismiss(id), timeoutMs)
  }

  function dismiss(id: number) {
    items.value = items.value.filter(t => t.id !== id)
  }

  return {
    items,
    dismiss,
    info: (text: string) => push('info', text),
    success: (text: string) => push('success', text),
    error: (text: string) => push('error', text),
  }
})
