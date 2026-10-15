import { useEffect, useRef } from 'react'

export type ShortcutEntry = {
  keys: string
  action: string
}

type ShortcutsOverlayProps = {
  open: boolean
  entries: ShortcutEntry[]
  onClose: () => void
}

export function ShortcutsOverlay({ open, entries, onClose }: ShortcutsOverlayProps) {
  const closeRef = useRef<HTMLButtonElement>(null)
  // App passes a fresh arrow each render; a ref keeps focus from jumping on every tick.
  const onCloseRef = useRef(onClose)
  useEffect(() => {
    onCloseRef.current = onClose
  })

  useEffect(() => {
    if (!open) {
      return
    }
    const opener = document.activeElement as HTMLElement | null
    closeRef.current?.focus()
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        onCloseRef.current()
      } else if (event.key === 'Tab') {
        // Close is the only focusable control, so Tab stays on it.
        event.preventDefault()
        closeRef.current?.focus()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      opener?.focus?.()
    }
  }, [open])

  if (!open) {
    return null
  }

  return (
    <div className="shortcuts-overlay" role="dialog" aria-modal="true" aria-labelledby="shortcuts-overlay-title">
      <button
        className="shortcuts-overlay__backdrop"
        type="button"
        aria-hidden="true"
        tabIndex={-1}
        onClick={onClose}
      />
      <section className="shortcuts-overlay__panel">
        <header>
          <div>
            <span className="eyebrow">Keyboard</span>
            <strong id="shortcuts-overlay-title">AeroForge Shortcuts</strong>
          </div>
          <button ref={closeRef} className="button button--ghost" type="button" onClick={onClose}>
            Close
          </button>
        </header>
        <ul>
          {entries.map((entry) => (
            <li key={`${entry.keys}-${entry.action}`}>
              <kbd>{entry.keys}</kbd>
              <span>{entry.action}</span>
            </li>
          ))}
        </ul>
        <p>
          Press <kbd>?</kbd> anytime to reopen this list, or <kbd>Esc</kbd> to close it. Shortcuts are ignored
          while typing in fields.
        </p>
      </section>
    </div>
  )
}
