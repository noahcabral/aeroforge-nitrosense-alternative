import { clamp } from '../lib/curves'
import { formatTelemetryValue } from '../lib/format'

type HomeTemperatureDialProps = {
  label: string
  identity?: string
  value: number | null
  details?: Array<{
    label: string
    value: number | null
    suffix?: string
    displayValue?: string
  }>
}

export function HomeTemperatureDial({
  label,
  identity,
  value,
  details = [],
}: HomeTemperatureDialProps) {
  const dialSize = 132
  const dialStroke = 16
  const dialRadius = (dialSize - dialStroke) / 2
  const safeValue = value == null ? 0 : clamp(value, 0, 100)
  const greenLength = clamp(safeValue, 0, 50)
  const orangeLength = clamp(safeValue - 50, 0, 20)
  const redLength = clamp(safeValue - 70, 0, 30)

  return (
    <div className={`home-temp-card ${details.length > 0 ? 'home-temp-card--detailed' : ''}`}>
      <div className="home-temp-card__heading">
        <span className="eyebrow">{label}</span>
        {identity && <strong>{identity}</strong>}
      </div>
      <div className="home-temp-card__visual">
        <svg
          className="home-temp-card__ring"
          viewBox={`0 0 ${dialSize} ${dialSize}`}
          aria-hidden="true"
        >
          <circle
            className="home-temp-card__track"
            cx={dialSize / 2}
            cy={dialSize / 2}
            r={dialRadius}
            pathLength="100"
          />
          <circle
            className="home-temp-card__segment home-temp-card__segment--green"
            cx={dialSize / 2}
            cy={dialSize / 2}
            r={dialRadius}
            pathLength="100"
            style={{
              strokeDasharray: `${greenLength} ${100 - greenLength}`,
              strokeDashoffset: '0',
            }}
          />
          <circle
            className="home-temp-card__segment home-temp-card__segment--orange"
            cx={dialSize / 2}
            cy={dialSize / 2}
            r={dialRadius}
            pathLength="100"
            style={{
              strokeDasharray: `${orangeLength} ${100 - orangeLength}`,
              strokeDashoffset: '-50',
            }}
          />
          <circle
            className="home-temp-card__segment home-temp-card__segment--red"
            cx={dialSize / 2}
            cy={dialSize / 2}
            r={dialRadius}
            pathLength="100"
            style={{
              strokeDasharray: `${redLength} ${100 - redLength}`,
              strokeDashoffset: '-70',
            }}
          />
        </svg>
        <div className="home-temp-card__content">
          <strong>
            {formatTelemetryValue(value)}
            {value == null ? '' : <span className="home-temp-card__unit"> C</span>}
          </strong>
        </div>
      </div>
      {details.length > 0 && (
        <div className="home-temp-card__details">
          {details.map((detail) => (
            <div className="home-temp-card__detail-row" key={detail.label}>
              <span>{detail.label}</span>
              <strong>
                {detail.displayValue ?? formatTelemetryValue(detail.value)}
                {detail.displayValue != null || detail.value == null ? '' : detail.suffix ?? ''}
              </strong>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

