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
  if (!open) {
    return null
  }

  return (
    <div className="shortcuts-overlay" role="dialog" aria-modal="true" aria-label="Keyboard shortcuts">
      <button className="shortcuts-overlay__backdrop" type="button" aria-label="Close shortcuts" onClick={onClose} />
      <section className="shortcuts-overlay__panel">
        <header>
          <div>
            <span className="eyebrow">Keyboard</span>
            <strong>AeroForge Shortcuts</strong>
          </div>
          <button className="button button--ghost" type="button" onClick={onClose}>
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
        <p>Press <kbd>?</kbd> anytime to reopen this list. Shortcuts are ignored while typing in fields.</p>
      </section>
    </div>
  )
}
