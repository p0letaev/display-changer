<script setup lang="ts">
import { TransitionPresets, useTransition } from '@vueuse/core'
import type { OutputConfig, Position } from '~/types/display'

const props = defineProps<{
  outputs: OutputConfig[]
  selected: string | null
  overlapping: Set<string>
  detached: Set<string>
  editable: boolean
}>()

const emit = defineEmits<{
  select: [connector: string | null]
  move: [connector: string, position: Position]
  drop: [connector: string]
}>()

/** Snap distance in screen pixels. */
const SNAP_PX = 14

const svg = ref<SVGSVGElement | null>(null)
const drag = ref<{ connector: string, dx: number, dy: number, moved: boolean } | null>(null)
const guides = ref<Guide[]>([])


const disabled = computed(() => props.outputs.filter(o => !o.enabled))

// The SVG works in logical pixels. The viewBox fits the desktop, stays frozen
// while dragging (so the scene does not rescale under the cursor) and then
// animates to the new bounds.
const fitted = computed<number[]>(() => {
  const box = boundingBox(props.outputs.filter(o => o.enabled).map(logicalRect))
  if (!box) return [0, 0, 1920, 1080]
  const pad = Math.max(box.width, box.height) * 0.15
  return [box.x - pad, box.y - pad, box.width + pad * 2, box.height + pad * 2]
})
const frozen = ref<number[] | null>(null)
const animated = useTransition(computed(() => frozen.value ?? fitted.value), {
  duration: 240,
  transition: TransitionPresets.easeOutCubic,
})
const viewBox = computed(() => animated.value.join(' '))
const unit = computed(() => Math.max(fitted.value[2]!, fitted.value[3]!) / 130)

// Text scales with each monitor's short side so labels fit portrait screens
// too, capped relative to the scene so a lone monitor is not all digits.
const items = computed(() =>
  props.outputs
    .map((output, index) => {
      const rect = logicalRect(output)
      return { output, index, rect, s: Math.min(rect.width, rect.height, unit.value * 45) / 100 }
    })
    .filter(item => item.output.enabled))

function toLogical(e: PointerEvent): DOMPoint {
  const ctm = svg.value!.getScreenCTM()!
  return new DOMPoint(e.clientX, e.clientY).matrixTransform(ctm.inverse())
}

function onPointerDown(e: PointerEvent, output: OutputConfig) {
  emit('select', output.id.connector)
  if (!props.editable || e.button !== 0) return
  svg.value!.setPointerCapture(e.pointerId)
  const p = toLogical(e)
  drag.value = {
    connector: output.id.connector,
    dx: p.x - output.position.x,
    dy: p.y - output.position.y,
    moved: false,
  }
  frozen.value = [...fitted.value]
}

function onPointerMove(e: PointerEvent) {
  const d = drag.value
  if (!d) return
  const output = props.outputs.find(o => o.id.connector === d.connector)
  if (!output) return
  const p = toLogical(e)
  const moving = { ...logicalRect(output), x: p.x - d.dx, y: p.y - d.dy }
  const others = items.value.filter(i => i.output !== output).map(i => i.rect)
  const threshold = SNAP_PX / svg.value!.getScreenCTM()!.a
  const result = snapPosition(moving, others, threshold)
  guides.value = result.guides
  d.moved = true
  emit('move', d.connector, result.pos)
}

function onPointerUp(e: PointerEvent) {
  const d = drag.value
  if (!d) return
  if (svg.value?.hasPointerCapture(e.pointerId)) svg.value.releasePointerCapture(e.pointerId)
  drag.value = null
  guides.value = []
  if (d.moved) emit('drop', d.connector)
  frozen.value = null
}
</script>

<template>
  <div class="canvas">
    <svg
      ref="svg"
      :viewBox="viewBox"
      preserveAspectRatio="xMidYMid meet"
      :class="{ dragging: drag }"
      @pointerdown.self="emit('select', null)"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
    >
      <g
        v-for="{ output, index, rect, s } in items"
        :key="output.id.connector"
        class="monitor"
        :class="{
          selected: output.id.connector === selected,
          active: drag?.connector === output.id.connector,
          invalid: overlapping.has(output.id.connector) || detached.has(output.id.connector),
          editable,
        }"
        :style="{ transform: `translate(${rect.x}px, ${rect.y}px)` }"
        @pointerdown="onPointerDown($event, output)"
      >
        <rect
          :x="unit * 0.35"
          :y="unit * 0.35"
          :width="rect.width - unit * 0.7"
          :height="rect.height - unit * 0.7"
          :rx="unit * 1.1"
        />
        <text class="index" :x="rect.width / 2" :y="rect.height / 2 - s * 6" :font-size="s * 30">
          {{ index + 1 }}
        </text>
        <text class="caption" :x="rect.width / 2" :y="rect.height / 2 + s * 16" :font-size="s * 7.5">
          {{ output.id.connector }}
        </text>
        <text class="caption" :x="rect.width / 2" :y="rect.height / 2 + s * 26" :font-size="s * 7.5">
          {{ formatResolution(output.mode) }} · {{ formatRefresh(output.mode.refreshMhz) }}
        </text>
        <text v-if="output.primary" class="star" :x="unit * 2.4" :y="unit * 3.4" :font-size="unit * 2.4">
          ★
        </text>
      </g>

      <line
        v-for="(g, i) in guides"
        :key="i"
        class="guide"
        :x1="g.x1"
        :y1="g.y1"
        :x2="g.x2"
        :y2="g.y2"
      />
    </svg>

    <div v-if="disabled.length" class="disabled">
      <span>Отключены:</span>
      <button
        v-for="o in disabled"
        :key="o.id.connector"
        :class="{ active: o.id.connector === selected }"
        @click="emit('select', o.id.connector)"
      >
        {{ o.id.connector }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.canvas {
  position: relative;
  height: 100%;
  display: flex;
  flex-direction: column;
}

svg {
  flex: 1;
  width: 100%;
  min-height: 0;
  touch-action: none;
}

.monitor {
  cursor: pointer;
  transition: transform 0.22s cubic-bezier(0.2, 0.8, 0.2, 1);
}

.monitor.editable {
  cursor: grab;
}

.monitor.active {
  cursor: grabbing;
  transition: none;
}

.monitor rect {
  fill: var(--monitor);
  stroke: var(--monitor-border);
  stroke-width: 1.5px;
  vector-effect: non-scaling-stroke;
  transition: fill 0.15s, stroke 0.15s;
}

.monitor:hover rect {
  stroke: var(--accent);
}

.monitor.selected rect {
  fill: var(--accent-soft);
  stroke: var(--accent);
  stroke-width: 2.5px;
}

.monitor.active rect {
  filter: drop-shadow(0 6px 16px rgb(0 0 0 / 0.35));
}

.monitor.invalid rect {
  stroke: var(--danger);
  stroke-dasharray: 6 4;
}

text {
  text-anchor: middle;
  dominant-baseline: central;
  pointer-events: none;
}

.index {
  fill: var(--text);
  font-weight: 600;
  opacity: 0.85;
}

.caption {
  fill: var(--text-muted);
  font-family: var(--mono);
}

.star {
  fill: var(--primary-badge);
}

.guide {
  stroke: var(--accent);
  stroke-width: 1px;
  stroke-dasharray: 4 4;
  vector-effect: non-scaling-stroke;
  pointer-events: none;
}

.disabled {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  padding: 8px 4px 0;
  color: var(--text-muted);
}

.disabled button.active {
  border-color: var(--accent);
}
</style>
