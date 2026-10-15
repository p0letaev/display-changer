// Mirrors crates/display-core/src/model.rs (serde camelCase). Keep in sync.

export interface MonitorId {
  vendor: string
  product: number
  serial: string | null
  connector: string
}

export interface Mode {
  width: number
  height: number
  refreshMhz: number
}

export type Rotation = 'normal' | 'left' | 'inverted' | 'right'

export interface Position {
  x: number
  y: number
}

export interface PhysicalSize {
  widthMm: number
  heightMm: number
}

export interface MonitorInfo {
  id: MonitorId
  name: string
  builtin: boolean
  physicalSize: PhysicalSize | null
  modes: Mode[]
  preferredMode: Mode | null
  rotations: Rotation[]
  /** Allowed scales; empty means any value the capabilities permit. */
  scales: number[]
  /** Accepts arbitrary modes (virtual outputs); `modes` are suggestions. */
  customModes: boolean
}

export interface OutputConfig {
  id: MonitorId
  enabled: boolean
  position: Position
  mode: Mode
  scale: number
  rotation: Rotation
  primary: boolean
  /** Connector of the output this one repeats; mirrors have no own place. */
  mirrorOf: string | null
}

export interface Layout {
  outputs: OutputConfig[]
}

export interface DisplayState {
  monitors: MonitorInfo[]
  layout: Layout
}

export interface Capabilities {
  scale: boolean
  fractionalScale: boolean
  rotation: boolean
  primary: boolean
  disable: boolean
  origin: 'topLeft' | 'primary'
  /** False on Windows: the desktop is in physical pixels regardless of scale. */
  scaledLayout: boolean
  mirror: boolean
}

export interface BackendInfo {
  name: string
  capabilities: Capabilities
}

export type LayoutIssue =
  | { type: 'noEnabledOutputs' }
  | { type: 'primaryCount'; count: number }
  | { type: 'unknownMonitor'; connector: string }
  | { type: 'duplicateOutput'; connector: string }
  | { type: 'unsupportedMode'; connector: string; mode: Mode }
  | { type: 'invalidScale'; connector: string; scale: number }
  | { type: 'rotationUnsupported'; connector: string }
  | { type: 'disableUnsupported'; connector: string }
  | { type: 'overlap'; a: string; b: string }
  | { type: 'detached'; connector: string }
  | { type: 'mirrorUnsupported'; connector: string }
  | { type: 'invalidMirrorSource'; connector: string; sourceName: string }
  | { type: 'mirrorPrimary'; connector: string }

/** Mirrors display_core::Error and src-tauri/src/error.rs. */
export type BackendError =
  | { kind: 'invalidLayout'; detail: LayoutIssue[] }
  | { kind: 'unsupported'; detail: string }
  | { kind: 'backend'; detail: string }
  | { kind: 'storage'; detail: string }
  | { kind: 'hotkey'; detail: string }
  | { kind: 'notFound'; detail: string }

// Mirrors crates/display-core/src/profile.rs.

export interface Profile {
  id: string
  name: string
  hotkey: string | null
  layout: Layout
  autoApply: boolean
}

export interface ProfileEntry extends Profile {
  active: boolean
  hotkeyError: string | null
}

export type ProfileWarning =
  | { type: 'missingMonitor'; connector: string; vendor: string }
  | { type: 'modeSubstituted'; connector: string; wanted: Mode; used: Mode }
  | { type: 'extraMonitor'; connector: string; disabled: boolean }
  | { type: 'primaryReassigned'; connector: string }
  | { type: 'mirrorDropped'; connector: string }

export interface Resolved {
  layout: Layout
  warnings: ProfileWarning[]
}

// Mirrors src-tauri/src/apply.rs.

export interface ApplyOutcome {
  state: DisplayState
  token: number
  timeoutSecs: number
}

export interface DisplayChanged {
  state: DisplayState
  reason: 'reverted' | 'quick' | 'auto' | 'monitors' | 'external' | 'reset'
  profileName: string | null
  /** Whether a change still waits for confirmation. */
  pending: boolean
}

export interface QuickFailed {
  profileName: string
  error: string
}

// Mirrors src-tauri/src/settings.rs.

export type Theme = 'system' | 'light' | 'dark'

export interface Settings {
  confirmTimeoutSecs: number
  resetCombo: boolean
  closeToTray: boolean
  startMinimized: boolean
  autostart: boolean
  autoSwitch: boolean
  theme: Theme
}
