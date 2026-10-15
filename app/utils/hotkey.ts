// Hotkeys are stored in global-hotkey syntax: modifiers + KeyboardEvent.code,
// e.g. "Ctrl+Alt+Shift+Digit1". The crate parses `code` names directly.

const MODIFIER_CODES = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/
const SUPPORTED_CODES = /^(Key[A-Z]|Digit\d|Numpad\d|F([1-9]|1\d|2[0-4])|Arrow(Up|Down|Left|Right)|Home|End|PageUp|PageDown|Insert|Delete|Space|Minus|Equal|BracketLeft|BracketRight|Backslash|Semicolon|Quote|Comma|Period|Slash|Backquote)$/

export type RecordResult =
  | { type: 'pending', modifiers: string[] }
  | { type: 'done', accelerator: string }
  | { type: 'invalid', reason: string }

export function recordKey(e: KeyboardEvent): RecordResult {
  const modifiers = [
    e.ctrlKey && 'Ctrl',
    e.altKey && 'Alt',
    e.shiftKey && 'Shift',
    e.metaKey && 'Super',
  ].filter(Boolean) as string[]

  if (MODIFIER_CODES.test(e.code)) return { type: 'pending', modifiers }
  if (!SUPPORTED_CODES.test(e.code)) return { type: 'invalid', reason: `Клавиша ${e.code} не поддерживается` }
  const isFunctionKey = /^F\d+$/.test(e.code)
  if (modifiers.length === 0 && !isFunctionKey) {
    return { type: 'invalid', reason: 'Нужен хотя бы один модификатор: Ctrl, Alt, Shift или Super' }
  }
  return { type: 'done', accelerator: [...modifiers, e.code].join('+') }
}

const KEY_LABELS: Record<string, string> = {
  ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→',
  Minus: '-', Equal: '=', BracketLeft: '[', BracketRight: ']', Backslash: '\\',
  Semicolon: ';', Quote: '\'', Comma: ',', Period: '.', Slash: '/', Backquote: '`',
}

/** "Ctrl+Alt+Digit1" -> ["Ctrl", "Alt", "1"] for rendering as key caps. */
export function acceleratorKeys(accelerator: string): string[] {
  return accelerator.split('+').map((part) => {
    if (KEY_LABELS[part]) return KEY_LABELS[part]
    return part.replace(/^Key/, '').replace(/^Digit/, '').replace(/^Numpad/, 'Num ')
  })
}
