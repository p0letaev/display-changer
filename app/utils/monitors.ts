import type { MonitorId, OutputConfig } from '~/types/display'

export interface SplitPart {
  /** 1-based, in the backend's output order. */
  part: number
  total: number
  /** Connectors of the other parts. */
  siblings: string[]
}

/**
 * Outputs that are inputs of one physical monitor (picture-by-picture):
 * they report the same EDID, serial included. Without a serial two identical
 * monitors look the same, so those are never grouped.
 */
export function splitParts(outputs: OutputConfig[]): Map<string, SplitPart> {
  const key = (id: MonitorId) => id.serial ? `${id.vendor}:${id.product}:${id.serial}` : null
  const groups = new Map<string, string[]>()
  for (const o of outputs) {
    const k = key(o.id)
    if (k) groups.set(k, [...groups.get(k) ?? [], o.id.connector])
  }
  const parts = new Map<string, SplitPart>()
  for (const connectors of groups.values()) {
    if (connectors.length < 2) continue
    connectors.forEach((c, i) => parts.set(c, {
      part: i + 1,
      total: connectors.length,
      siblings: connectors.filter(other => other !== c),
    }))
  }
  return parts
}
