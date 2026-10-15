import { describe, expect, it } from 'vitest'
import {
  KEYBOARD_SHORTCUT_ENTRIES,
  isEditableKeyboardTarget,
  parseControlShortcut,
} from './keyboard'

type KeyInit = {
  key: string
  ctrlKey?: boolean
  shiftKey?: boolean
  altKey?: boolean
  metaKey?: boolean
  defaultPrevented?: boolean
  target?: EventTarget | null
}

/** Minimal stand-in: parseControlShortcut only reads these fields. */
function keyEvent(init: KeyInit): KeyboardEvent {
  return {
    key: init.key,
    ctrlKey: init.ctrlKey ?? false,
    shiftKey: init.shiftKey ?? false,
    altKey: init.altKey ?? false,
    metaKey: init.metaKey ?? false,
    defaultPrevented: init.defaultPrevented ?? false,
    target: init.target ?? null,
  } as unknown as KeyboardEvent
}

function element(tagName: string, isContentEditable = false) {
  return { tagName, isContentEditable } as unknown as EventTarget
}

describe('isEditableKeyboardTarget', () => {
  it('treats form fields and contenteditable hosts as editable', () => {
    expect(isEditableKeyboardTarget(element('INPUT'))).toBe(true)
    expect(isEditableKeyboardTarget(element('textarea'))).toBe(true)
    expect(isEditableKeyboardTarget(element('SELECT'))).toBe(true)
    expect(isEditableKeyboardTarget(element('DIV', true))).toBe(true)
  })

  it('treats everything else as a shortcut surface', () => {
    expect(isEditableKeyboardTarget(element('DIV'))).toBe(false)
    expect(isEditableKeyboardTarget(element('BUTTON'))).toBe(false)
    expect(isEditableKeyboardTarget(null)).toBe(false)
    expect(isEditableKeyboardTarget({} as EventTarget)).toBe(false)
  })
})

describe('parseControlShortcut', () => {
  it('yields to text fields', () => {
    expect(parseControlShortcut(keyEvent({ key: '1', target: element('INPUT') }))).toBeNull()
    expect(
      parseControlShortcut(keyEvent({ key: 's', ctrlKey: true, target: element('TEXTAREA') })),
    ).toBeNull()
  })

  it('maps digits to tabs', () => {
    expect(parseControlShortcut(keyEvent({ key: '1' }))).toEqual({ type: 'tab', tab: 'home' })
    expect(parseControlShortcut(keyEvent({ key: '5' }))).toEqual({ type: 'tab', tab: 'debug' })
    expect(parseControlShortcut(keyEvent({ key: '6' }))).toBeNull()
  })

  it('maps QWERT to power profiles regardless of case', () => {
    expect(parseControlShortcut(keyEvent({ key: 'q' }))).toEqual({
      type: 'power',
      profile: 'battery-guard',
    })
    expect(parseControlShortcut(keyEvent({ key: 'R' }))).toEqual({
      type: 'power',
      profile: 'turbo',
    })
  })

  it('maps ASDF and F1-F4 to fan profiles', () => {
    expect(parseControlShortcut(keyEvent({ key: 's' }))).toEqual({ type: 'fan', profile: 'cool' })
    expect(parseControlShortcut(keyEvent({ key: 'F4' }))).toEqual({
      type: 'fan',
      profile: 'custom',
    })
  })

  it('handles the chorded diagnostics and backup shortcuts', () => {
    expect(parseControlShortcut(keyEvent({ key: 'C', ctrlKey: true, shiftKey: true }))).toEqual({
      type: 'copy-diagnostics',
    })
    expect(parseControlShortcut(keyEvent({ key: 'e', ctrlKey: true, shiftKey: true }))).toEqual({
      type: 'export-diagnostics',
    })
    expect(parseControlShortcut(keyEvent({ key: 'b', ctrlKey: true, shiftKey: true }))).toEqual({
      type: 'restore-balanced-auto',
    })
    expect(parseControlShortcut(keyEvent({ key: 's', ctrlKey: true }))).toEqual({
      type: 'export-settings',
    })
  })

  it('does not treat Ctrl+letter as a bare profile shortcut', () => {
    expect(parseControlShortcut(keyEvent({ key: 'q', ctrlKey: true }))).toBeNull()
    expect(parseControlShortcut(keyEvent({ key: 'a', ctrlKey: true }))).toBeNull()
  })

  it('does not treat Shift+letter as a bare profile shortcut', () => {
    expect(parseControlShortcut(keyEvent({ key: 'Q', shiftKey: true }))).toBeNull()
  })

  it('handles Escape and help', () => {
    expect(parseControlShortcut(keyEvent({ key: 'Escape' }))).toEqual({ type: 'dismiss-toast' })
    expect(parseControlShortcut(keyEvent({ key: '?' }))).toEqual({ type: 'help' })
    expect(parseControlShortcut(keyEvent({ key: '/', shiftKey: true }))).toEqual({ type: 'help' })
  })

  it('ignores events with Alt or Meta, or that were already handled', () => {
    expect(parseControlShortcut(keyEvent({ key: '1', altKey: true }))).toBeNull()
    expect(parseControlShortcut(keyEvent({ key: '1', metaKey: true }))).toBeNull()
    expect(parseControlShortcut(keyEvent({ key: '1', defaultPrevented: true }))).toBeNull()
  })

  it('documents every shortcut family in the help entries', () => {
    const text = KEYBOARD_SHORTCUT_ENTRIES.map((entry) => entry.keys).join(' ')
    for (const keys of ['1–5', 'Q W E R T', 'A S D F', 'Ctrl+Shift+C', 'Ctrl+S', 'Esc', '?']) {
      expect(text).toContain(keys)
    }
  })
})
