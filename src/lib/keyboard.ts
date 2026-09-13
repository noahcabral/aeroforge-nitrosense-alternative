export type ControlTabId = 'home' | 'power' | 'fans' | 'personal' | 'debug'
export type PowerProfileShortcutId =
  | 'battery-guard'
  | 'balanced'
  | 'performance'
  | 'turbo'
  | 'custom'
export type FanProfileShortcutId = 'auto' | 'cool' | 'max' | 'custom'

const TAB_BY_DIGIT: Record<string, ControlTabId> = {
  '1': 'home',
  '2': 'power',
  '3': 'fans',
  '4': 'personal',
  '5': 'debug',
}

const POWER_BY_KEY: Record<string, PowerProfileShortcutId> = {
  q: 'battery-guard',
  w: 'balanced',
  e: 'performance',
  r: 'turbo',
  t: 'custom',
}

const FAN_BY_KEY: Record<string, FanProfileShortcutId> = {
  a: 'auto',
  s: 'cool',
  d: 'max',
  f: 'custom',
}

/**
 * True when keystrokes on this target belong to a text field rather than to the
 * global shortcut map. Duck-typed rather than `instanceof HTMLElement` so it also
 * works for elements from another realm and outside a browser.
 */
export function isEditableKeyboardTarget(target: EventTarget | null) {
  if (!target || typeof target !== 'object' || !('tagName' in target)) {
    return false
  }

  const element = target as { tagName: unknown; isContentEditable?: unknown }
  const tag = typeof element.tagName === 'string' ? element.tagName.toUpperCase() : ''
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') {
    return true
  }

  return element.isContentEditable === true
}

export type ShortcutAction =
  | { type: 'tab'; tab: ControlTabId }
  | { type: 'power'; profile: PowerProfileShortcutId }
  | { type: 'fan'; profile: FanProfileShortcutId }
  | { type: 'copy-diagnostics' }
  | { type: 'export-diagnostics' }
  | { type: 'export-settings' }
  | { type: 'restore-balanced-auto' }
  | { type: 'dismiss-toast' }
  | { type: 'help' }

/** Parse global AeroForge keyboard shortcuts. Returns null when the event is not handled. */
export function parseControlShortcut(event: KeyboardEvent): ShortcutAction | null {
  if (event.defaultPrevented || event.altKey || event.metaKey) {
    return null
  }

  if (isEditableKeyboardTarget(event.target)) {
    return null
  }

  const lower = event.key.toLowerCase()

  if (event.ctrlKey && event.shiftKey && (lower === 'd' || lower === 'c')) {
    return { type: 'copy-diagnostics' }
  }

  if (event.ctrlKey && event.shiftKey && lower === 'e') {
    return { type: 'export-diagnostics' }
  }

  if (event.ctrlKey && event.shiftKey && (lower === 'b' || lower === 'r')) {
    return { type: 'restore-balanced-auto' }
  }

  if (event.ctrlKey && !event.shiftKey && lower === 's') {
    return { type: 'export-settings' }
  }

  if (event.ctrlKey) {
    return null
  }

  if (event.key === 'Escape') {
    return { type: 'dismiss-toast' }
  }

  if (event.key === '?' || (event.shiftKey && event.key === '/')) {
    return { type: 'help' }
  }

  if (event.shiftKey) {
    return null
  }

  const tab = TAB_BY_DIGIT[event.key]
  if (tab) {
    return { type: 'tab', tab }
  }

  const power = POWER_BY_KEY[lower]
  if (power) {
    return { type: 'power', profile: power }
  }

  const fan = FAN_BY_KEY[lower]
  if (fan) {
    return { type: 'fan', profile: fan }
  }

  switch (event.key) {
    case 'F1':
      return { type: 'fan', profile: 'auto' }
    case 'F2':
      return { type: 'fan', profile: 'cool' }
    case 'F3':
      return { type: 'fan', profile: 'max' }
    case 'F4':
      return { type: 'fan', profile: 'custom' }
    default:
      return null
  }
}

export const KEYBOARD_SHORTCUT_ENTRIES: { keys: string; action: string }[] = [
  { keys: '1–5', action: 'Switch Home / Power / Fans / Settings / Debug' },
  { keys: 'Q W E R T', action: 'Quiet / Balanced / Performance / Turbo / Custom power' },
  { keys: 'A S D F', action: 'Auto / Cool / Max / Custom fans' },
  { keys: 'F1–F4', action: 'Fan Auto / Cool / Max / Custom' },
  { keys: 'Ctrl+Shift+C', action: 'Copy diagnostics report' },
  { keys: 'Ctrl+Shift+E', action: 'Export diagnostics JSON' },
  { keys: 'Ctrl+Shift+B', action: 'Restore Balanced power + Auto fans' },
  { keys: 'Ctrl+S', action: 'Export settings backup JSON' },
  { keys: 'Esc', action: 'Dismiss newest toast' },
  { keys: '?', action: 'Show this shortcut list' },
]

export const KEYBOARD_SHORTCUT_HINT =
  'Shortcuts: 1-5 tabs · Q/W/E/R/T power · A/S/D/F fans · Ctrl+Shift+C diagnostics · Ctrl+S backup · ? help'
