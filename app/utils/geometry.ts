// Layout geometry for the editor. Overlap and adjacency rules mirror
// crates/display-core/src/layout.rs; the backend re-validates before applying.

import type { Layout, OutputConfig, Position } from '~/types/display'

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

/** A snapping guide line in logical coordinates. */
export interface Guide {
  x1: number
  y1: number
  x2: number
  y2: number
}

// Mirrors Capabilities::scaled_layout; set once the backend is known.
let scaledLayout = true

export function setScaledLayout(value: boolean) {
  scaledLayout = value
}

export function logicalRect(output: OutputConfig): Rect {
  const swap = output.rotation === 'left' || output.rotation === 'right'
  const w = swap ? output.mode.height : output.mode.width
  const h = swap ? output.mode.width : output.mode.height
  const scale = scaledLayout ? output.scale : 1
  return {
    x: output.position.x,
    y: output.position.y,
    width: Math.round(w / scale),
    height: Math.round(h / scale),
  }
}

export function boundingBox(rects: Rect[]): Rect | null {
  if (rects.length === 0) return null
  const x0 = Math.min(...rects.map(r => r.x))
  const y0 = Math.min(...rects.map(r => r.y))
  const x1 = Math.max(...rects.map(r => r.x + r.width))
  const y1 = Math.max(...rects.map(r => r.y + r.height))
  return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
}

/** Interiors intersect; shared edges do not count. */
export function overlaps(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width
    && a.y < b.y + b.height && b.y < a.y + a.height
}

/** Share an edge or a corner without overlapping. */
export function touches(a: Rect, b: Rect): boolean {
  return !overlaps(a, b)
    && a.x <= b.x + b.width && b.x <= a.x + a.width
    && a.y <= b.y + b.height && b.y <= a.y + a.height
}

const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi)

/**
 * Pulls `moving` to nearby edges and centers of `others` while dragging.
 * `threshold` is in logical pixels.
 */
export function snapPosition(moving: Rect, others: Rect[], threshold: number): { pos: Position, guides: Guide[] } {
  let best = { dx: Infinity, x: moving.x, guideX: 0 }
  let bestY = { dy: Infinity, y: moving.y, guideY: 0 }

  for (const o of others) {
    // [candidate left edge of moving, coordinate of the aligned line]
    const xs: [number, number][] = [
      [o.x, o.x], [o.x + o.width, o.x + o.width],
      [o.x - moving.width, o.x], [o.x + o.width - moving.width, o.x + o.width],
      [o.x + (o.width - moving.width) / 2, o.x + o.width / 2],
    ]
    for (const [x, line] of xs) {
      const d = Math.abs(x - moving.x)
      if (d <= threshold && d < best.dx) best = { dx: d, x, guideX: line }
    }
    const ys: [number, number][] = [
      [o.y, o.y], [o.y + o.height, o.y + o.height],
      [o.y - moving.height, o.y], [o.y + o.height - moving.height, o.y + o.height],
      [o.y + (o.height - moving.height) / 2, o.y + o.height / 2],
    ]
    for (const [y, line] of ys) {
      const d = Math.abs(y - moving.y)
      if (d <= threshold && d < bestY.dy) bestY = { dy: d, y, guideY: line }
    }
  }

  const pos = { x: Math.round(best.x), y: Math.round(bestY.y) }
  const snapped = { ...moving, ...pos }
  const all = boundingBox([snapped, ...others])!
  const guides: Guide[] = []
  if (best.dx !== Infinity) guides.push({ x1: best.guideX, y1: all.y, x2: best.guideX, y2: all.y + all.height })
  if (bestY.dy !== Infinity) guides.push({ x1: all.x, y1: bestY.guideY, x2: all.x + all.width, y2: bestY.guideY })
  return { pos, guides }
}

/**
 * Nearest position for `moving` that touches at least one of `others`
 * without overlapping any. Returns the input position if it is already fine.
 */
export function settle(moving: Rect, others: Rect[]): Position {
  if (others.length === 0) return { x: moving.x, y: moving.y }
  const fits = (r: Rect) => !others.some(o => overlaps(r, o))
  if (fits(moving) && others.some(o => touches(moving, o))) return { x: moving.x, y: moving.y }

  const { width: w, height: h } = moving
  const candidates: Position[] = []
  for (const o of others) {
    const y = clamp(moving.y, o.y - h, o.y + o.height)
    const x = clamp(moving.x, o.x - w, o.x + o.width)
    candidates.push(
      { x: o.x - w, y },
      { x: o.x + o.width, y },
      { x, y: o.y - h },
      { x, y: o.y + o.height },
    )
  }
  const dist = (p: Position) => Math.hypot(p.x - moving.x, p.y - moving.y)
  const valid = candidates.filter(p => fits({ ...moving, ...p })).sort((a, b) => dist(a) - dist(b))
  if (valid[0]) return valid[0]

  const box = boundingBox(others)!
  return { x: box.x + box.width, y: box.y }
}

/**
 * Makes the layout contiguous and overlap-free. Outputs listed in `anchors`
 * (then the primary) keep their place; the rest are attached one by one,
 * nearest first. Mutates `outputs`.
 */
export function repair(outputs: OutputConfig[], anchors: string[] = []): void {
  const enabled = outputs.filter(o => o.enabled)
  if (enabled.length < 2) return

  const order = [
    ...anchors.map(c => enabled.find(o => o.id.connector === c)).filter(Boolean) as OutputConfig[],
    ...enabled.filter(o => o.primary && !anchors.includes(o.id.connector)),
  ]
  if (order.length === 0) order.push(enabled[0]!)
  const first = logicalRect(order[0]!)
  const center = (r: Rect) => ({ x: r.x + r.width / 2, y: r.y + r.height / 2 })
  const c0 = center(first)
  const rest = enabled
    .filter(o => !order.includes(o))
    .sort((a, b) => {
      const ca = center(logicalRect(a))
      const cb = center(logicalRect(b))
      return Math.hypot(ca.x - c0.x, ca.y - c0.y) - Math.hypot(cb.x - c0.x, cb.y - c0.y)
    })

  const placed: Rect[] = [first]
  for (const o of [...order.slice(1), ...rest]) {
    const pos = settle(logicalRect(o), placed)
    o.position = pos
    placed.push(logicalRect(o))
  }
}

export interface LiveIssues {
  overlapping: Set<string>
  detached: Set<string>
  primaryCount: number
  enabledCount: number
}

/** Geometry problems for highlighting while editing. */
export function liveIssues(layout: Layout): LiveIssues {
  const enabled = layout.outputs.filter(o => o.enabled)
  const rects = enabled.map(logicalRect)
  const overlapping = new Set<string>()
  for (let i = 0; i < rects.length; i++) {
    for (let j = i + 1; j < rects.length; j++) {
      if (overlaps(rects[i]!, rects[j]!)) {
        overlapping.add(enabled[i]!.id.connector)
        overlapping.add(enabled[j]!.id.connector)
      }
    }
  }
  const reached = new Set<number>(rects.length ? [0] : [])
  const stack = rects.length ? [0] : []
  while (stack.length) {
    const i = stack.pop()!
    rects.forEach((r, j) => {
      if (!reached.has(j) && touches(rects[i]!, r)) {
        reached.add(j)
        stack.push(j)
      }
    })
  }
  const detached = new Set(enabled.filter((_, i) => !reached.has(i)).map(o => o.id.connector))
  return {
    overlapping,
    detached,
    primaryCount: enabled.filter(o => o.primary).length,
    enabledCount: enabled.length,
  }
}

/** Same desktop up to translation; mirrors display_core::profile::same_layout. */
export function sameLayout(a: Layout, b: Layout): boolean {
  if (a.outputs.length !== b.outputs.length) return false
  const origin = (l: Layout) => boundingBox(l.outputs.filter(o => o.enabled).map(logicalRect)) ?? { x: 0, y: 0 }
  const oa = origin(a)
  const ob = origin(b)
  return a.outputs.every((x) => {
    const y = b.outputs.find(o => o.id.connector === x.id.connector)
    if (!y || x.enabled !== y.enabled) return false
    if (!x.enabled) return true
    return x.mode.width === y.mode.width && x.mode.height === y.mode.height
      && x.mode.refreshMhz === y.mode.refreshMhz
      && Math.abs(x.scale - y.scale) < 1e-6
      && x.rotation === y.rotation && x.primary === y.primary
      && x.position.x - oa.x === y.position.x - ob.x
      && x.position.y - oa.y === y.position.y - ob.y
  })
}
