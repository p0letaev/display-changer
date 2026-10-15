import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  ApplyOutcome,
  BackendInfo,
  DisplayChanged,
  DisplayState,
  HotkeyFailed,
  Layout,
  Profile,
  ProfileEntry,
  Resolved,
} from '~/types/display'

/** Typed wrappers over the Tauri commands in src-tauri/src/commands.rs. */
export function useDisplayApi() {
  return {
    available: isTauri(),

    backendInfo: () => invoke<BackendInfo>('backend_info'),
    getState: () => invoke<DisplayState>('get_display_state'),
    validateLayout: (layout: Layout) => invoke<void>('validate_layout', { layout }),
    applyLayout: (layout: Layout) => invoke<ApplyOutcome>('apply_layout', { layout }),
    confirmApply: (token: number) => invoke<boolean>('confirm_apply', { token }),
    revertApply: (token: number) => invoke<DisplayState>('revert_apply', { token }),

    listProfiles: () => invoke<ProfileEntry[]>('list_profiles'),
    previewProfile: (id: string) => invoke<Resolved>('preview_profile', { id }),
    applyProfile: (id: string) => invoke<ApplyOutcome>('apply_profile', { id }),
    createProfile: (name: string, layout: Layout) =>
      invoke<Profile>('create_profile', { name, layout }),
    updateProfile: (id: string, patch: { name?: string, layout?: Layout }) =>
      invoke<Profile>('update_profile', { id, name: patch.name ?? null, layout: patch.layout ?? null }),
    deleteProfile: (id: string) => invoke<void>('delete_profile', { id }),
    setProfileHotkey: (id: string, hotkey: string | null) =>
      invoke<Profile>('set_profile_hotkey', { id, hotkey }),
    setHotkeysPaused: (paused: boolean) => invoke<void>('set_hotkeys_paused', { paused }),

    onDisplayChanged: (cb: (e: DisplayChanged) => void) =>
      listen<DisplayChanged>('display://changed', e => cb(e.payload)),
    onHotkeyFailed: (cb: (e: HotkeyFailed) => void) =>
      listen<HotkeyFailed>('display://hotkey-failed', e => cb(e.payload)),
  }
}
