<script setup lang="ts">
const toasts = useToastStore()
</script>

<template>
  <TransitionGroup tag="div" name="toast" class="toasts" aria-live="polite">
    <div
      v-for="t in toasts.items"
      :key="t.id"
      class="toast"
      :class="t.kind"
      @click="toasts.dismiss(t.id)"
    >
      {{ t.text }}
    </div>
  </TransitionGroup>
</template>

<style scoped>
.toasts {
  position: fixed;
  right: 16px;
  bottom: 16px;
  z-index: 50;
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-width: 380px;
}

.toast {
  padding: 10px 14px;
  border-radius: var(--radius);
  background: var(--surface);
  border: 1px solid var(--border);
  border-left-width: 3px;
  box-shadow: 0 8px 24px rgb(0 0 0 / 0.25);
  white-space: pre-line;
  cursor: pointer;
}

.toast.success {
  border-left-color: #3ecf8e;
}

.toast.error {
  border-left-color: var(--danger);
}

.toast.info {
  border-left-color: var(--accent);
}

.toast-enter-active,
.toast-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(16px);
}
</style>
