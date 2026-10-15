import { formatTelemetryValue } from '../lib/format'

type HomeFanReadoutCardProps = {
  label: string
  value: number | null
  detail: string
}

export function HomeFanReadoutCard({ label, value, detail }: HomeFanReadoutCardProps) {
  return (
    <div className="home-fan-card">
      <div className="home-fan-card__visual" aria-hidden="true">
        <div className="home-fan-card__hub" />
      </div>
      <div className="home-fan-card__copy">
        <span className="eyebrow">{label}</span>
        <strong>{value == null ? 'Offline' : formatTelemetryValue(value)}</strong>
        <small>{value == null ? '' : 'RPM'}</small>
        <p>{detail}</p>
      </div>
    </div>
  )
}

