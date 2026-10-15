<script setup lang="ts">
const store = useDisplayStore()
const now = ref(Date.now())
const timer = setInterval(() => { now.value = Date.now() }, 250)
onUnmounted(() => clearInterval(timer))

// The backend reverts on its own timer; this is only the visible countdown.
const secondsLeft = computed(() => {
  const p = store.pending
  return p ? Math.max(0, Math.ceil((p.deadline - now.value) / 1000)) : 0
})
</script>

<template>
  <Transition name="slide">
    <div v-if="store.pending" class="confirm" role="alertdialog" aria-live="assertive">
      <div class="text">
        <strong>Оставить новые настройки?</strong>
        <span>Без подтверждения вернём прежние через {{ secondsLeft }} с</span>
      </div>
      <button :disabled="store.busy" @click="store.revertApply()">
        Вернуть
      </button>
      <button class="primary" :disabled="store.busy" @click="store.confirmApply()">
        Оставить
      </button>
    </div>
  </Transition>
</template>

<style scoped>
.confirm {
  position: absolute;
  left: 50%;
  top: 16px;
  transform: translateX(-50%);
  z-index: 20;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 14px 12px 18px;
  border-radius: 12px;
  background: var(--surface);
  border: 1px solid var(--accent);
  box-shadow: 0 12px 40px rgb(0 0 0 / 0.35);
}

.text {
  display: flex;
  flex-direction: column;
  margin-right: 8px;
  white-space: nowrap;
}

.text span {
  font-size: 12px;
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}

.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}

.slide-enter-active,
.slide-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}

.slide-enter-from,
.slide-leave-to {
  opacity: 0;
  transform: translate(-50%, -12px);
}
</style>
