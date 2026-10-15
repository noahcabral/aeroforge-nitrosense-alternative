import type { ControlLayerStatus } from '../lib/backend'

type LayerStatusStripProps = {
  statuses: ControlLayerStatus[]
  emptyLabel: string
}

function formatLayerState(state: ControlLayerStatus['state']) {
  switch (state) {
    case 'applied':
      return 'Applied'
    case 'failed':
      return 'Failed'
    case 'skipped':
      return 'Skipped'
    case 'unavailable':
      return 'Unavailable'
    default:
      return 'Unknown'
  }
}

export function LayerStatusStrip({ statuses, emptyLabel }: LayerStatusStripProps) {
  if (statuses.length === 0) {
    return (
      <div className="layer-status-strip layer-status-strip--empty">
        <span>{emptyLabel}</span>
      </div>
    )
  }

  return (
    <div className="layer-status-strip">
      {statuses.map((status) => (
        <div
          key={status.id}
          className={`layer-status layer-status--${status.state}`}
          title={status.detail}
        >
          <span>{status.label}</span>
          <strong>{formatLayerState(status.state)}</strong>
        </div>
      ))}
    </div>
  )
}
