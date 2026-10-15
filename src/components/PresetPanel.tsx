import type { ControlSnapshot } from '../lib/backend'

type FanControlPreset = ControlSnapshot['fanPresets'][number]
type PowerControlPreset = ControlSnapshot['powerPresets'][number]
type QuickControlPreset = ControlSnapshot['quickPresets'][number]
type PresetCollection = 'fan' | 'power' | 'quick'

type PresetPanelProps = {
  fanPresets: FanControlPreset[]
  powerPresets: PowerControlPreset[]
  quickPresets: QuickControlPreset[]
  onSaveFan: () => void
  onSavePower: () => void
  onSaveQuick: () => void
  onApplyFan: (preset: FanControlPreset) => void
  onApplyPower: (preset: PowerControlPreset) => void
  onApplyQuick: (preset: QuickControlPreset) => void
  onRename: (collection: PresetCollection, id: string) => void
  onDuplicate: (collection: PresetCollection, id: string) => void
  onDelete: (collection: PresetCollection, id: string) => void
  onExport?: () => void
  onImport?: () => void
}

export function PresetPanel({
  fanPresets,
  powerPresets,
  quickPresets,
  onSaveFan,
  onSavePower,
  onSaveQuick,
  onApplyFan,
  onApplyPower,
  onApplyQuick,
  onRename,
  onDuplicate,
  onDelete,
  onExport,
  onImport,
}: PresetPanelProps) {
  const fanName = (id: string | null) =>
    fanPresets.find((preset) => preset.id === id)?.name ?? 'No fan preset'
  const powerName = (id: string | null) =>
    powerPresets.find((preset) => preset.id === id)?.name ?? 'No power preset'

  return (
    <section className="panel preset-panel">
      <div className="preset-panel__header">
        <div>
          <span className="eyebrow">Presets</span>
          <strong>Save and switch your favorite setups</strong>
        </div>
        <div className="preset-panel__save-actions">
          {onImport ? (
            <button className="button button--ghost" onClick={onImport} type="button">
              Import
            </button>
          ) : null}
          {onExport ? (
            <button className="button button--ghost" onClick={onExport} type="button">
              Export
            </button>
          ) : null}
          <button className="button button--ghost" onClick={onSaveFan} type="button">
            Save Fan
          </button>
          <button className="button button--ghost" onClick={onSavePower} type="button">
            Save Power
          </button>
          <button className="button" onClick={onSaveQuick} type="button">
            Save Combo
          </button>
        </div>
      </div>

      <div className="preset-grid">
        <PresetList
          title="Combos"
          emptyLabel="Save a combo to switch fan and power together."
          presets={quickPresets}
          describe={(preset) => `${fanName(preset.fanPresetId)} / ${powerName(preset.powerPresetId)}`}
          onApply={onApplyQuick}
          onRename={(id) => onRename('quick', id)}
          onDuplicate={(id) => onDuplicate('quick', id)}
          onDelete={(id) => onDelete('quick', id)}
        />
        <PresetList
          title="Fan"
          emptyLabel="Save Cool, Custom, Auto, or Max fan setups."
          presets={fanPresets}
          describe={(preset) => `${preset.profileId} fan profile`}
          onApply={onApplyFan}
          onRename={(id) => onRename('fan', id)}
          onDuplicate={(id) => onDuplicate('fan', id)}
          onDelete={(id) => onDelete('fan', id)}
        />
        <PresetList
          title="Power"
          emptyLabel="Save power modes and CPU min/max settings."
          presets={powerPresets}
          describe={(preset) =>
            `${preset.profileId} mode, CPU ${preset.processorState.minPercent}-${preset.processorState.maxPercent}%`
          }
          onApply={onApplyPower}
          onRename={(id) => onRename('power', id)}
          onDuplicate={(id) => onDuplicate('power', id)}
          onDelete={(id) => onDelete('power', id)}
        />
      </div>
    </section>
  )
}

type PresetListProps<TPreset extends { id: string; name: string }> = {
  title: string
  emptyLabel: string
  presets: TPreset[]
  describe: (preset: TPreset) => string
  onApply: (preset: TPreset) => void
  onRename: (id: string) => void
  onDuplicate: (id: string) => void
  onDelete: (id: string) => void
}

function PresetList<TPreset extends { id: string; name: string }>({
  title,
  emptyLabel,
  presets,
  describe,
  onApply,
  onRename,
  onDuplicate,
  onDelete,
}: PresetListProps<TPreset>) {
  return (
    <div className="preset-list">
      <div className="preset-list__header">
        <strong>{title}</strong>
        <span>{presets.length}</span>
      </div>

      {presets.length === 0 ? (
        <p className="preset-list__empty">{emptyLabel}</p>
      ) : (
        presets.map((preset) => (
          <article className="preset-item" key={preset.id}>
            <div>
              <strong>{preset.name}</strong>
              <small>{describe(preset)}</small>
            </div>
            <div className="preset-item__actions">
              <button type="button" onClick={() => onApply(preset)}>
                Apply
              </button>
              <button type="button" onClick={() => onRename(preset.id)}>
                Rename
              </button>
              <button type="button" onClick={() => onDuplicate(preset.id)}>
                Copy
              </button>
              <button type="button" onClick={() => onDelete(preset.id)}>
                Delete
              </button>
            </div>
          </article>
        ))
      )}
    </div>
  )
}
