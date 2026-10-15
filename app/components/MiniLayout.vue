<script setup lang="ts">
import type { Layout } from '~/types/display'

const props = defineProps<{ layout: Layout }>()

const rects = computed(() =>
  props.layout.outputs.filter(isPlaced).map(o => ({ rect: logicalRect(o), primary: o.primary })))

const viewBox = computed(() => {
  const box = boundingBox(rects.value.map(r => r.rect))
  if (!box) return '0 0 16 9'
  const pad = Math.max(box.width, box.height) * 0.04
  return `${box.x - pad} ${box.y - pad} ${box.width + pad * 2} ${box.height + pad * 2}`
})

const gap = computed(() => {
  const box = boundingBox(rects.value.map(r => r.rect))
  return box ? Math.max(box.width, box.height) / 60 : 0
})
</script>

<template>
  <svg class="mini" :viewBox="viewBox" preserveAspectRatio="xMidYMid meet">
    <rect
      v-for="(r, i) in rects"
      :key="i"
      :class="{ primary: r.primary }"
      :x="r.rect.x + gap / 2"
      :y="r.rect.y + gap / 2"
      :width="r.rect.width - gap"
      :height="r.rect.height - gap"
      :rx="gap"
    />
  </svg>
</template>

<style scoped>
.mini {
  display: block;
  width: 100%;
  height: 100%;
}

rect {
  fill: var(--monitor);
  stroke: var(--monitor-border);
  stroke-width: 1px;
  vector-effect: non-scaling-stroke;
}

rect.primary {
  fill: var(--accent-soft);
  stroke: var(--accent);
}
</style>
