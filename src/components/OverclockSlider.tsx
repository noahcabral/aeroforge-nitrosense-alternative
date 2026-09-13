type OverclockSliderProps = {
  label: string
  unit: string
  value: number
  min: number
  max: number
  step: number
  disabled?: boolean
  statusLabel?: string
  note?: string
  onChange: (value: number) => void
}

export function OverclockSlider({
  label,
  unit,
  value,
  min,
  max,
  step,
  disabled = false,
  statusLabel,
  note,
  onChange,
}: OverclockSliderProps) {
  return (
    <label className={`power-setting ${disabled ? 'is-disabled' : ''}`}>
      <div className="power-setting__header">
        <div className="power-setting__title">
          <span>{label}</span>
          {statusLabel && <em className="power-setting__badge">{statusLabel}</em>}
        </div>
        <strong>
          {value > 0 && unit !== 'C' ? '+' : ''}
          {value}
          {unit}
        </strong>
      </div>

      <input
        className="power-setting__slider"
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
      />

      <input
        className="power-setting__input"
        type="number"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
      />

      {note && <small className="power-setting__note">{note}</small>}
    </label>
  )
}

