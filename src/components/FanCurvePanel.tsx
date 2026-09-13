import {
  buildCurvePath,
  chartHeight,
  chartPadding,
  chartWidth,
  pointToChart,
  speedMax,
  speedMin,
  tempMax,
  tempMin,
  type CurvePoint,
  type CurveTarget,
} from '../lib/curves'

type FanCurvePanelProps = {
  title: string
  target: CurveTarget
  points: CurvePoint[]
  editable: boolean
  chartRef: (node: SVGSVGElement | null) => void
  onPointDown: (index: number) => void
  syncLockEnabled: boolean
  onSyncLockToggle: () => void
  onSecondaryAction: () => void
  secondaryLabel: string
}

export function FanCurvePanel({
  title,
  target,
  points,
  editable,
  chartRef,
  onPointDown,
  syncLockEnabled,
  onSyncLockToggle,
  onSecondaryAction,
  secondaryLabel,
}: FanCurvePanelProps) {
  return (
    <section className="fan-curve-card">
      <div className="fan-curve-card__header">
        <h3>{title}</h3>
        <span>{target.toUpperCase()} thermal zone</span>
      </div>

      <div className="fan-curve-card__chart-wrap">
        <svg
          ref={chartRef}
          className={`curve-chart ${editable ? 'is-editable' : ''}`}
          viewBox={`0 0 ${chartWidth} ${chartHeight}`}
          aria-label={`${target.toUpperCase()} fan curve editor`}
        >
          {[speedMin, 25, 50, 75, 100].map((speed) => {
            const y =
              chartHeight -
              chartPadding -
              ((speed - speedMin) / (speedMax - speedMin)) * (chartHeight - chartPadding * 2)

            return (
              <g key={`${target}-speed-${speed}`}>
                <line
                  className="curve-chart__grid"
                  x1={chartPadding}
                  x2={chartWidth - chartPadding}
                  y1={y}
                  y2={y}
                />
              </g>
            )
          })}

          {[30, 45, 60, 75, 90].map((temp) => {
            const x =
              chartPadding +
              ((temp - tempMin) / (tempMax - tempMin)) * (chartWidth - chartPadding * 2)

            return (
              <g key={`${target}-temp-${temp}`}>
                <line
                  className="curve-chart__grid curve-chart__grid--vertical"
                  x1={x}
                  x2={x}
                  y1={chartPadding}
                  y2={chartHeight - chartPadding}
                />
              </g>
            )
          })}

          <path className="curve-chart__glow" d={buildCurvePath(points)} />
          <path className="curve-chart__path" d={buildCurvePath(points)} />

          {points.map((point, index) => {
            const { x, y } = pointToChart(point)
            return (
              <g key={`${target}-${index}`}>
                <circle className="curve-chart__halo" cx={x} cy={y} r="14" />
                <circle
                  className="curve-chart__point"
                  cx={x}
                  cy={y}
                  r="8"
                  onPointerDown={(event) => {
                    if (!editable) {
                      return
                    }
                    event.preventDefault()
                    event.currentTarget.setPointerCapture(event.pointerId)
                    onPointDown(index)
                  }}
                />
              </g>
            )
          })}
        </svg>

        <div className="fan-curve-card__axis-note fan-curve-card__axis-note--left">
          fan %
        </div>
        <div className="fan-curve-card__axis-note fan-curve-card__axis-note--right">
          temp C
        </div>
        <div className="fan-curve-card__axis-note fan-curve-card__axis-note--bottom-left">
          30C
        </div>
        <div className="fan-curve-card__axis-note fan-curve-card__axis-note--bottom-right">
          90C
        </div>
      </div>

      <div className="fan-curve-card__points">
        {points.map((point, index) => (
          <div className="fan-curve-chip" key={`${target}-chip-${index}`}>
            <span>P{index + 1}</span>
            <strong>{point.temp}C</strong>
            <strong>{point.speed}%</strong>
          </div>
        ))}
      </div>

      <div className="fan-curve-actions">
        <button
          className={`button button--toggle ${syncLockEnabled ? 'is-active' : ''}`}
          onClick={onSyncLockToggle}
          aria-pressed={syncLockEnabled}
        >
          Sync Lock
        </button>
        <button className="button" onClick={onSecondaryAction}>
          {secondaryLabel}
        </button>
      </div>
    </section>
  )
}
