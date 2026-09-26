type OverclockSliderProps = {
  label: string
  unit: string
  value: number
  min: number
  max: number
  step: number
  disabled?: boolean
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
  onChange,
}: OverclockSliderProps) {
  return (
    <label className={`power-setting ${disabled ? 'is-disabled' : ''}`}>
      <div className="power-setting__header">
        <div className="power-setting__title">
          <span>{label}</span>
        </div>
        <strong>
          {value > 0 ? '+' : ''}
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
        aria-label={`${label} (${unit})`}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </label>
  )
}

