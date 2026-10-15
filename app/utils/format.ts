import type { BackendError, LayoutIssue, Mode, ProfileWarning, Rotation } from '~/types/display'

export function describeWarning(w: ProfileWarning): string {
  switch (w.type) {
    case 'missingMonitor': return `${w.connector} (${w.vendor}) не подключён — пропущен`
    case 'modeSubstituted': return `${w.connector}: ${formatResolution(w.wanted)} @ ${formatRefresh(w.wanted.refreshMhz)} недоступен, будет ${formatResolution(w.used)} @ ${formatRefresh(w.used.refreshMhz)}`
    case 'extraMonitor': return w.disabled
      ? `${w.connector} нет в профиле — будет отключён`
      : `${w.connector} нет в профиле — останется включённым`
    case 'primaryReassigned': return `Основным станет ${w.connector}`
    case 'mirrorDropped': return `${w.connector}: нечего повторять — будет отдельным экраном`
  }
}

export function formatRefresh(mhz: number): string {
  const hz = mhz / 1000
  return `${Number.isInteger(hz) ? hz : hz.toFixed(2)} Гц`
}

export function formatResolution(mode: Mode): string {
  return `${mode.width} × ${mode.height}`
}

export function formatScale(scale: number): string {
  return `${Math.round(scale * 100)} %`
}

export const rotationLabels: Record<Rotation, string> = {
  normal: 'Без поворота',
  left: '90° влево',
  inverted: '180°',
  right: '90° вправо',
}

function describeIssue(issue: LayoutIssue): string {
  switch (issue.type) {
    case 'noEnabledOutputs': return 'Нет ни одного включённого монитора'
    case 'primaryCount': return `Основной монитор должен быть ровно один (сейчас ${issue.count})`
    case 'unknownMonitor': return `${issue.connector}: монитор не подключён`
    case 'duplicateOutput': return `${issue.connector}: указан дважды`
    case 'unsupportedMode': return `${issue.connector}: режим ${formatResolution(issue.mode)} @ ${formatRefresh(issue.mode.refreshMhz)} не поддерживается`
    case 'invalidScale': return `${issue.connector}: масштаб ${formatScale(issue.scale)} недоступен`
    case 'rotationUnsupported': return `${issue.connector}: поворот не поддерживается`
    case 'disableUnsupported': return `${issue.connector}: нельзя отключить`
    case 'overlap': return `${issue.a} перекрывает ${issue.b}`
    case 'detached': return `${issue.connector}: оторван от остальных мониторов`
    case 'mirrorUnsupported': return `${issue.connector}: повтор экрана здесь не поддерживается`
    case 'invalidMirrorSource': return `${issue.connector}: нельзя повторять ${issue.sourceName}`
    case 'mirrorPrimary': return `${issue.connector}: повторяющий монитор не может быть основным`
  }
}

function isBackendError(e: unknown): e is BackendError {
  return typeof e === 'object' && e !== null && 'kind' in e
}

export function describeError(e: unknown): string {
  if (isBackendError(e)) {
    switch (e.kind) {
      case 'invalidLayout': return e.detail.map(describeIssue).join('\n')
      case 'unsupported': return `Не поддерживается: ${e.detail}`
      case 'backend': return `Ошибка бэкенда: ${e.detail}`
      case 'storage': return `Не удалось сохранить профили: ${e.detail}`
      case 'hotkey': return `Хоткей не назначен: ${e.detail}`
      case 'notFound': return 'Профиль не найден'
    }
  }
  return e instanceof Error ? e.message : String(e)
}
